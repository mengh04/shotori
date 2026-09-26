//! # Windows snap backend: EnumWindows + DWM uncloaking
//!
//! Unlike Wayland, every visible toplevel is enumerable here — the
//! tiled-window blind spots of the compositor IPC backends do not exist.
//! Filtering:
//! - cloaked windows (UWP suspended ghosts, windows on another virtual
//!   desktop) via `DwmGetWindowAttribute(DWMWA_CLOAKED)`
//! - minimized windows (`IsIconic` — their rect is the (-32000,-32000)
//!   parking orbit)
//! - shell furniture: taskbar and wallpaper host windows, by class name
//!
//! Coordinate conversion: `GetWindowRect` is physical, the session's
//! global space is "physical origin + logical extent" (see
//! `crate::platform::capture::windows` for why that hybrid tiles
//! correctly) — so the origin passes through unchanged and the size is
//! divided by the containing monitor's effective scale.

use gpui_kit::{point, px, size};

use super::SnapRect;

pub fn query() -> Option<Vec<SnapRect>> {
    struct Ctx {
        rects: Vec<SnapRect>,
        foreground: windows::Win32::Foundation::HWND,
    }
    unsafe extern "system" fn callback(
        hwnd: windows::Win32::Foundation::HWND,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::core::BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        if let Some(rect) = unsafe { inspect_window(hwnd, ctx.foreground) } {
            ctx.rects.push(rect);
        }
        windows::core::BOOL(1)
    }

    let mut ctx = Ctx {
        rects: Vec::new(),
        foreground: unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() },
    };
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(callback),
            windows::Win32::Foundation::LPARAM(&mut ctx as *mut Ctx as isize),
        );
    }
    if ctx.rects.is_empty() {
        None
    } else {
        Some(ctx.rects)
    }
}

/// Shell chrome that must never be a snap target
const SKIPPED_CLASSES: [&str; 5] = [
    "Progman",       // wallpaper host
    "WorkerW",       // wallpaper fallback
    "Shell_TrayWnd", // primary taskbar
    "Shell_SecondaryTrayWnd",
    "TaskListThumbnailWnd",
];

unsafe fn inspect_window(
    hwnd: windows::Win32::Foundation::HWND,
    foreground: windows::Win32::Foundation::HWND,
) -> Option<SnapRect> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsIconic,
        IsWindowVisible,
    };

    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }

        // Cloaked = suspended UWP ghost or window on another desktop
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return None;
        }

        // Shell furniture check (class name is short; stack buffer is ample)
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        if SKIPPED_CLASSES.contains(&class.as_str()) {
            return None;
        }

        // Title: empty-captioned system windows are not user content
        let title_len = GetWindowTextLengthW(hwnd);
        if title_len <= 0 {
            return None;
        }
        let mut title = vec![0u16; title_len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut title);
        let title = String::from_utf16_lossy(&title[..copied.max(0) as usize]);

        let mut rect = RECT::default();
        // DWM's VISIBLE bounds, not GetWindowRect: Win10/11 windows carry
        // an invisible resize frame (~7 px per side) that GetWindowRect
        // includes — snapping on it would frame a border of desktop
        // around every window (measured: Chrome/Firefox/UWP all report
        // GW=(334,164,1630,972) vs DWM=(341,164,1623,965)). Fullscreen
        // and borderless windows report identical rects either way.
        let visible = DwmGetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok();
        if !visible {
            rect = RECT::default();
            if !GetWindowRect(hwnd, &mut rect).is_ok() {
                return None;
            }
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 {
            return None;
        }

        // Physical → hybrid session space: origin unchanged, extent ÷ scale
        let scale = monitor_scale(hwnd).unwrap_or(1.);
        Some(SnapRect {
            bounds: gpui_kit::Bounds {
                origin: point(px(rect.left as f32), px(rect.top as f32)),
                size: size(px(w as f32 / scale), px(h as f32 / scale)),
            },
            app_id: title,
            focused: hwnd == foreground,
            recency: 0, // no per-window focus timestamps on this backend
        })
    }
}

/// Effective scale of the monitor a window sits on (fallback: 1.0)
unsafe fn monitor_scale(hwnd: windows::Win32::Foundation::HWND) -> Option<f32> {
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromWindow};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    unsafe {
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        if hmon.is_invalid() {
            return None;
        }
        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;
        if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() {
            Some(dpi_x as f32 / 96.)
        } else {
            None
        }
    }
}
