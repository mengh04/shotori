//! # Windows capture backend: one GDI `BitBlt` per monitor
//!
//! ## Coordinate space (the important part)
//!
//! The virtual-desktop coordinate space is **physical** pixels; monitor
//! origins (`rcMonitor`) and sizes are exact device pixels regardless of
//! DPI — but only in a **per-monitor-DPI-aware** process, which is why
//! [`enable_per_monitor_dpi_awareness`] runs before anything else.
//!
//! The session's global "logical" space is therefore defined as:
//! origin = the monitor's physical origin, size = physical ÷ effective
//! scale (a hybrid space, but one that tiles without gaps or overlaps —
//! unlike dividing every origin by its own scale, which makes mixed-DPI
//! monitors overlap in logical coordinates). Local overlay coordinates
//! then satisfy `local = (physical − monitor origin) ÷ scale`, exactly
//! the identity the session's local↔global math already assumes on
//! Wayland.
//!
//! ## Misc
//!
//! - `CAPTUREBLT` includes layered windows in the freeze (the toolbar
//!   and menus of other apps) — parity with what a compositor
//!   photographs on Wayland.
//! - The cursor is **not** captured (GDI never blits it); screencopy
//!   parity again — the overlay's crosshair is the only pointer.
//! - GDI delivers pixels already in displayed orientation, so
//!   [`Transform::Normal`] always holds (a portrait-rotated monitor's
//!   `rcMonitor` is already portrait).
//! - GDI misses exclusive-fullscreen hardware overlays in rare cases;
//!   DXGI Desktop Duplication is the upgrade path if that ever bites.

use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC,
    GetDIBits, GetMonitorInfoW, HMONITOR, MONITORINFO, MONITORINFOEXW, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};

use super::{Capture, Transform};

/// Make the process per-monitor-v2 DPI aware. Must run before any GDI
/// call or window opens: an unaware process sees virtualized
/// (pre-scaled) coordinates, monitor rects stop matching the physical
/// layout and captures come out at the wrong resolution. No-op (with a
/// logged failure) if the context was already set — e.g. by a manifest.
pub(crate) fn enable_per_monitor_dpi_awareness() {
    // BOOL/Result depending on metadata version: discard either way
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

struct Monitor {
    hmon: HMONITOR,
    rect: RECT,
    /// `\\.\DISPLAY1` style device path (leading prefix trimmed)
    name: String,
    /// effective DPI ÷ 96 (e.g. 1.5 for 144 DPI)
    scale: f32,
}

pub(super) fn capture_all() -> anyhow::Result<Vec<Capture>> {
    let monitors = enumerate_monitors();
    if monitors.is_empty() {
        anyhow::bail!("no monitor found");
    }

    // One capture per monitor; a single failure must not sink the rest
    let mut caps = Vec::new();
    for m in &monitors {
        match capture_one(m) {
            Ok(c) => caps.push(c),
            Err(e) => eprintln!("[shotori] capture failed for {}: {e:#}", m.name),
        }
    }
    if caps.is_empty() {
        anyhow::bail!("all monitor captures failed");
    }
    Ok(caps)
}

fn enumerate_monitors() -> Vec<Monitor> {
    struct Ctx {
        monitors: Vec<Monitor>,
    }
    unsafe extern "system" fn callback(
        hmon: HMONITOR,
        _hdc: windows::Win32::Graphics::Gdi::HDC,
        rect: *mut RECT,
        lparam: LPARAM,
    ) -> windows::core::BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        let rect = unsafe { *rect };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let name = if unsafe {
            GetMonitorInfoW(hmon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool()
        } {
            let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(0);
            String::from_utf16_lossy(&info.szDevice[..len])
                .trim_start_matches(r"\\.\")
                .to_owned()
        } else {
            String::new()
        };
        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;
        let scale = if unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }
            .is_ok()
        {
            dpi_x as f32 / 96.
        } else {
            1. // unreachable in practice on Win10+; keeps the math sane
        };
        ctx.monitors.push(Monitor {
            hmon,
            rect,
            name,
            scale,
        });
        windows::core::BOOL(1) // continue enumeration
    }

    let mut ctx = Ctx {
        monitors: Vec::new(),
    };
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(callback),
            LPARAM(&mut ctx as *mut Ctx as isize),
        );
    }
    ctx.monitors
}

/// Screenshot one monitor: blit its rectangle of the (virtual) screen
/// into a compatible bitmap, then read the bits back as top-down RGBA.
fn capture_one(m: &Monitor) -> anyhow::Result<Capture> {
    let w = m.rect.right - m.rect.left;
    let h = m.rect.bottom - m.rect.top;
    if w <= 0 || h <= 0 {
        anyhow::bail!("monitor {} has an empty rect", m.name);
    }

    let hdc_screen = unsafe { GetDC(None) };
    let hdc_mem = unsafe { CreateCompatibleDC(Some(hdc_screen)) };
    let bitmap = unsafe { CreateCompatibleBitmap(hdc_screen, w, h) };
    let old = unsafe { SelectObject(hdc_mem, bitmap.into()) };

    // CAPTUREBLT pulls in layered windows (other apps' overlays); the
    // cursor is never part of a BitBlt
    let blit = unsafe {
        BitBlt(
            hdc_mem,
            0,
            0,
            w,
            h,
            Some(hdc_screen),
            m.rect.left,
            m.rect.top,
            SRCCOPY | CAPTUREBLT,
        )
    };

    // Deselect before GetDIBits (docs: the bitmap must not be selected
    // into the DC it is read through)
    unsafe { SelectObject(hdc_mem, old) };

    let mut rgba = Vec::new();
    if blit.is_ok() {
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // negative = top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        rgba = vec![0u8; (w * h * 4) as usize];
        let lines = unsafe {
            GetDIBits(
                hdc_mem,
                bitmap,
                0,
                h as u32,
                Some(rgba.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        if lines != h {
            anyhow::bail!("GetDIBits copied {lines} of {h} rows on {}", m.name);
        }
        // 32bpp GDI is BGRA in memory → swizzle to RGBA
        for chunk in rgba.as_chunks_mut::<4>().0 {
            chunk.swap(0, 2);
            chunk[3] = 255; // the X byte is undefined; make it opaque
        }
    }

    // Cleanup regardless of success
    let _ = unsafe { DeleteObject(bitmap.into()) };
    let _ = unsafe { DeleteDC(hdc_mem) };
    unsafe { ReleaseDC(None, hdc_screen) };

    if !blit.is_ok() {
        anyhow::bail!("BitBlt failed on monitor {}", m.name);
    }
    let _ = m.hmon; // kept for diagnostics / future backends

    Ok(Capture {
        output_name: m.name.clone(),
        logical_pos: (m.rect.left, m.rect.top),
        // None = use the width÷scale fallback, which on Windows is the
        // TRUE logical size (effective DPI is exact, unlike wl_output's
        // integer-rounded scale) — no i32 rounding loss this way
        logical_size: None,
        scale: m.scale,
        transform: Transform::Normal,
        width: w as u32,
        height: h as u32,
        rgba,
    })
}
