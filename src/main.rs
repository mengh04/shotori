//! # Shotori entry point: assembly, keybindings, window creation
//!
//! Module layout (avoiding a god file):
//! - `capture`: screencopy capture (multi-output, dedicated wayland connection)
//! - `display`: capture ↔ gpui display matching and waiting
//! - `clipboard`: clipboard copy (zwlr_data_control + resident background daemon)
//! - `overlay`: overlay assembly (one window per screen)
//! - `hud` / `toolbar`: the overlay's visual pieces
//! - `selection` / `export` / `image_util`: pure logic
//! - `ocr`: selection OCR (rapidocr-core + PP-OCRv6 models, default feature)
//! - `theme`: visual constants

use gpui_kit::*;

use shotori::capture;
use shotori::clipboard;
use shotori::display;
use shotori::overlay::{CopySelection, Overlay, QuitOverlay, SaveSelection};
#[cfg(feature = "ocr")]
use shotori::overlay::OcrSelection;

fn main() {
    // Notification child: `shotori --notify <summary> <body>` (see notify.rs)
    if std::env::args().nth(1).as_deref() == Some(shotori::notify::NOTIFY_ARG) {
        std::process::exit(shotori::notify::notify_main());
    }

    // Clipboard daemon: the background resident process behind the copy
    // action (see the resident-offer model in clipboard.rs)
    if std::env::args().nth(1).as_deref() == Some(clipboard::DAEMON_ARG) {
        if let Err(e) = clipboard::daemon_main() {
            eprintln!("[shotori] clipboard daemon exiting: {e:#}");
            std::process::exit(1);
        }
        return;
    }

    // ① Freeze all screens (must complete before the overlays appear)
    let caps = match capture::capture_all_outputs() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[shotori] capture failed: {e:#}");
            std::process::exit(1);
        }
    };
    println!(
        "[shotori] frozen {} screen(s): {}",
        caps.len(),
        caps.iter()
            .map(|c| format!(
                "{} {}x{}{}",
                c.output_name,
                c.width,
                c.height,
                if c.rotated() { " (rotated)" } else { "" }
            ))
            .collect::<Vec<_>>()
            .join(" · ")
    );

    // ② Overlays (one per screen)
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::base::init(cx);

            // Keybindings are scoped by key_context
            cx.bind_keys([
                KeyBinding::new("escape", QuitOverlay, Some("ShotoriOverlay")),
                KeyBinding::new("enter", CopySelection, Some("ShotoriOverlay")),
                KeyBinding::new("ctrl-c", CopySelection, Some("ShotoriOverlay")),
                KeyBinding::new("ctrl-s", SaveSelection, Some("ShotoriOverlay")),
                // The ocr feature is on by default; slim builds
                // (--no-default-features) skip this binding
                #[cfg(feature = "ocr")]
                KeyBinding::new("ctrl-o", OcrSelection, Some("ShotoriOverlay")),
            ]);
            // Backstop: Esc still exits if the overlay somehow loses focus.
            // Note: dispatch_action inside a window does NOT bubble up here
            // (actions stop at the focus path — measured); the overlay's own
            // QuitOverlay handler is the real exit implementation; this line
            // only guards the rare focus-loss case
            cx.on_action(|_: &QuitOverlay, cx| cx.quit());

            // Window creation lives in an async task: displays() is always
            // empty during synchronous startup (upstream zed#46378) and
            // becomes usable after the first event-loop pass — match each
            // capture to a display_id and open windows here
            cx.spawn(async move |cx| {
                let targets = display::await_display_ids(caps, cx).await;
                cx.update(|cx| {
                    for (cap, did) in targets {
                        if did.is_none() {
                            eprintln!(
                                "[shotori] warning: {} matched no display, placement left to the compositor",
                                cap.output_name
                            );
                        }
                        cx.open_window(Overlay::window_options(did), |window, cx| {
                            cx.new(|cx| Overlay::new(cap, window, cx))
                        })
                        .expect("failed to open layer-shell window");
                    }
                });
            })
            .detach();
        });
}