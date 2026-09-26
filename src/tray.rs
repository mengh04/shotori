//! # Tray mode: a resident launcher behind a system tray icon
//!
//! `shotori tray` keeps a tray icon alive (Windows: Shell_NotifyIcon,
//! Linux: StatusNotifierItem, via gpui-tray) and starts a **separate**
//! `shotori gui` process on activation. The screenshot session keeps
//! its run-and-exit lifecycle — the tray never shares a process with
//! the overlay, so `cx.quit()` at the end of a capture cannot tear the
//! tray down, and a wedged overlay can't take the launcher with it.
//!
//! The menu is deliberately minimal: Screenshot, Quit. Every real
//! interaction lives in the overlay; the tray is only a way in.
//!
//! Quit mode is [`QuitMode::Explicit`]: the app has no windows, so the
//! default last-window-closed heuristic would exit immediately.

use std::process::Command;

use gpui_kit::*;
use gpui_tray::{Icon, Tray};

actions!(tray, [TakeScreenshot, QuitTray]);

/// Keeps the [`Tray`] alive for the process lifetime (gpui-tray removes
/// the native item on drop).
struct TrayGlobal(Tray);

impl Global for TrayGlobal {}

/// Run the tray app. Returns when the user quits from the menu.
pub fn run() {
    application().with_quit_mode(QuitMode::Explicit).run(|cx| {
        if let Err(e) = setup(cx) {
            eprintln!("[shotori] tray unavailable: {e:#}");
            eprintln!(
                "[shotori] hint: Linux needs a StatusNotifierItem host \
                     (waybar, KDE plasma, a GNOME appindicator extension)"
            );
            cx.quit();
        }
    });
}

fn setup(cx: &mut App) -> gpui_tray::Result<()> {
    cx.on_action(take_screenshot).on_action(quit_tray);

    let tray = Tray::builder()
        .icon(icon()?)
        .tooltip("Shotori — click to take a screenshot")
        .menu(|_| {
            vec![
                MenuItem::action("Screenshot", TakeScreenshot),
                MenuItem::separator(),
                MenuItem::action("Quit", QuitTray),
            ]
        })
        // Left-click / primary activation does the same as the menu item
        .on_activate(TakeScreenshot)
        .build(cx)?;
    cx.set_global(TrayGlobal(tray));
    println!("[shotori] tray running");
    Ok(())
}

fn take_screenshot(_: &TakeScreenshot, _: &mut App) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[shotori] tray: cannot locate own executable: {e}");
            return;
        }
    };
    let mut cmd = Command::new(exe);
    cmd.arg("gui");
    // Detached: the gui process outlives this call; dropping the handle
    // leaves it running (same pattern as the notify child).
    #[cfg(target_os = "windows")]
    suppress_console_window(&mut cmd);
    match cmd.spawn() {
        Ok(child) => {
            drop(child);
            println!("[shotori] tray: screenshot launched");
        }
        Err(e) => eprintln!("[shotori] tray: failed to launch screenshot: {e}"),
    }
}

fn quit_tray(_: &QuitTray, cx: &mut App) {
    let tray = cx.global::<TrayGlobal>().0.clone();
    if let Err(e) = tray.close(cx) {
        eprintln!("[shotori] tray: close failed: {e}");
    }
    cx.quit();
}

/// Only spawn the gui with CREATE_NO_WINDOW when this process itself has
/// no console (tray launched from Explorer/autostart): a fresh overlay
/// would flash an empty console window. When the tray runs from a
/// terminal, inherit it instead — the gui's `[shotori] …` logs stay
/// visible during development.
#[cfg(target_os = "windows")]
fn suppress_console_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    use windows::Win32::System::Console::GetConsoleWindow;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if unsafe { GetConsoleWindow() }.is_invalid() {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// A viewfinder frame with a center dot — drawn in code so no asset
/// shipping is needed (white on transparent; the shell recolors nothing,
/// so it stays legible on both light and dark trays).
fn icon() -> gpui_tray::Result<Icon> {
    const SIZE: i32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let put = |x: i32, y: i32, rgba: &mut Vec<u8>| {
        if (0..SIZE).contains(&x) && (0..SIZE).contains(&y) {
            let o = ((y * SIZE + x) * 4) as usize;
            rgba[o..o + 4].copy_from_slice(&[235, 240, 245, 255]);
        }
    };

    // Four L-shaped corners (a viewfinder frame)
    let (lo, hi) = (5, 26);
    let t = 3; // stroke thickness
    let arm = 9; // corner arm length
    for i in 0..arm {
        for k in 0..t {
            put(lo + i, lo + k, &mut rgba); // top-left horizontal
            put(lo + k, lo + i, &mut rgba); // top-left vertical
            put(hi - i, lo + k, &mut rgba); // top-right
            put(hi - k, lo + i, &mut rgba);
            put(lo + i, hi - k, &mut rgba); // bottom-left
            put(lo + k, hi - i, &mut rgba);
            put(hi - i, hi - k, &mut rgba); // bottom-right
            put(hi - k, hi - i, &mut rgba);
        }
    }

    // Center dot (radius 4)
    let (cx, cy, r) = ((lo + hi) / 2, (lo + hi) / 2, 4);
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            let (dx, dy) = (x - cx, y - cy);
            if dx * dx + dy * dy <= r * r {
                put(x, y, &mut rgba);
            }
        }
    }

    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32)
}
