//! # Overlay window fixup (Windows): client-origin alignment
//!
//! **The bug this corrects** (measured on Win11, gpui-pre-windows
//! 0.3.6): `WindowKind::PopUp` windows are created with
//! `WINDOW_STYLE(0)`, which the system treats as an overlapped window
//! and gives an invisible resize frame (`GetWindowRect −
//! GetClientRect` = 16×8 px: 8 left/right, 4 top/bottom). Two upstream
//! mechanisms then disagree about that frame:
//!
//! 1. `WM_NCCALCSIZE` under `hide_title_bar` (true for `titlebar:
//!    None`) pulls the **client top back to the window top** — the
//!    "self-drawn title bar" trick, which makes the top inset 0
//!    (asymmetric!)
//! 2. `calculate_window_rect` compensates the frame **symmetrically**:
//!    the window rectangle becomes `(−8, −4, w+8, h+4)` so that *with
//!    symmetric insets* the client would land exactly on the target
//!
//! Combined, the client rect ends up at `(0, −4, 1920, 1080)` for a
//! `(0, 0)` target: the whole rendered surface floats 4 px too high
//! (and, at other frame sizes/DPIs, by whatever `top inset` the frame
//! has). The 8 px horizontal insets stay symmetric, so only the
//! vertical direction is off.
//!
//! **The fix** refuses to model the frame at all: measure where the
//! client origin actually is (`ClientToScreen`), diff it against the
//! monitor's physical origin (which is exactly [`Capture::logical_pos`]
//! on Windows — see `capture/windows.rs`), and translate the window by
//! the difference with `SetWindowPos`. Measuring beats computing: it
//! stays correct at any DPI/frame size, and it is a no-op the day the
//! upstream asymmetry is fixed.
//!
//! Runs once per overlay window inside `Overlay::new`, before the first
//! frame is drawn — the shift is invisible.

use gpui_kit::Window;
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};

use crate::platform::capture::Capture;

/// Translate the overlay window so its client origin lands on the
/// monitor's physical origin. Best-effort: any failure just leaves the
/// window where the platform put it (a 4 px offset is cosmetic).
pub(crate) fn align_client_to_monitor(window: &Window, cap: &Capture) {
    let hwnd = match native_hwnd(window) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        let mut client_origin = POINT { x: 0, y: 0 };
        if !ClientToScreen(hwnd, &mut client_origin).as_bool() {
            return;
        }
        let (dx, dy) = (
            cap.logical_pos.0 - client_origin.x,
            cap.logical_pos.1 - client_origin.y,
        );
        if dx == 0 && dy == 0 {
            return; // already aligned (upstream fixed, or frameless)
        }
        let mut rect = RECT::default();
        if !GetWindowRect(hwnd, &mut rect).is_ok() {
            return;
        }
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left + dx,
            rect.top + dy,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        println!("[shotori] overlay realigned by ({dx},{dy}) — Windows frame inset compensation");
    }
}

fn native_hwnd(window: &Window) -> Option<HWND> {
    // gpui's own inherent `Window::window_handle()` (an AnyWindowHandle)
    // shadows the raw-window-handle trait method — call the trait head-on
    let raw = HasWindowHandle::window_handle(window).ok()?.as_raw();
    match raw {
        raw_window_handle::RawWindowHandle::Win32(handle) => {
            Some(HWND(handle.hwnd.get() as *mut core::ffi::c_void))
        }
        _ => None,
    }
}
