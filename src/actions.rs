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

gpui_kit::actions!([
    QuitOverlay,
    CopySelection,
    SaveSelection,
    OcrSelection,
    SelectScreen
]);

// In-canvas annotation actions (shared by the overlay's keybindings and
// the toolbar's tool buttons, like everything else in this module)
gpui_kit::actions!([
    ToggleRectangle,
    ToggleEllipse,
    ToggleLine,
    ToggleArrow,
    ToggleNumber,
    TogglePencil,
    ToggleHighlighter,
    ToggleMosaic,
    TogglePolyline,
    FinishPolyline,
    UndoAnnotation,
    RedoAnnotation
]);

/// Keybindings, scoped to the `ShotoriOverlay` key context. Bound once
/// during startup in `main`.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", QuitOverlay, Some("ShotoriOverlay")),
        KeyBinding::new("enter", CopySelection, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-a", SelectScreen, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-c", CopySelection, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-s", SaveSelection, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-o", OcrSelection, Some("ShotoriOverlay")),
    ]);
}

/// Annotation keybindings ("r" rectangle, "e" ellipse, "l" line, "a"
/// arrow, "m" mosaic, "h" highlighter, "b" pencil, "n" number, "p"
/// polyline; undo/redo; Enter finishes a polyline). Also scoped to
/// `ShotoriOverlay`, plus the PolylineDrawing sub-context.
pub fn init_annotation_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("r", ToggleRectangle, Some("ShotoriOverlay")),
        KeyBinding::new("e", ToggleEllipse, Some("ShotoriOverlay")),
        KeyBinding::new("l", ToggleLine, Some("ShotoriOverlay")),
        KeyBinding::new("a", ToggleArrow, Some("ShotoriOverlay")),
        KeyBinding::new("m", ToggleMosaic, Some("ShotoriOverlay")),
        KeyBinding::new("h", ToggleHighlighter, Some("ShotoriOverlay")),
        KeyBinding::new("b", TogglePencil, Some("ShotoriOverlay")),
        KeyBinding::new("n", ToggleNumber, Some("ShotoriOverlay")),
        KeyBinding::new("p", TogglePolyline, Some("ShotoriOverlay")),
        KeyBinding::new("enter", FinishPolyline, Some("PolylineDrawing")),
        KeyBinding::new("ctrl-z", UndoAnnotation, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-y", RedoAnnotation, Some("ShotoriOverlay")),
        KeyBinding::new("ctrl-shift-z", RedoAnnotation, Some("ShotoriOverlay")),
    ]);
}
