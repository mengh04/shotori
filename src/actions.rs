//! # Shared actions: the vocabulary of every selection operation
//!
//! Actions are the contract between three consumers: the keybindings
//! (bound once in `main`), the toolbar buttons (`ui::toolbar` dispatches
//! the exact same actions) and the handlers (`ui::overlay`). Defining
//! them here — not inside the overlay — keeps that triangle acyclic.
//!
//! The OCR-setup dialog keeps its own two actions local to
//! `ui::ocr_setup`; they are dialog-internal.

use gpui_kit::*;

gpui_kit::actions!([QuitOverlay, CopySelection, SaveSelection, OcrSelection]);

/// Keybindings, scoped to the `ShotoriOverlay` key context. Bound once
/// during startup in `main`.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", QuitOverlay, Some("ShotoriOverlay")),
        KeyBinding::new("enter", CopySelection, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-c", CopySelection, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-s", SaveSelection, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-o", OcrSelection, Some("ShotoriOverlay")),
    ]);
}
