//! # Selection toolbar: Copy / Save / OCR / Cancel
//!
//! Buttons dispatch the exact same actions as the keyboard through
//! `dispatch_action` — one action, two triggers, one pipeline.
//! Visibility is decided by the overlay: it only appears after the selection
//! is finalized ([`crate::selection::Selection::is_selected`]).

use gpui_kit::base::Button;
use gpui_kit::*;

#[cfg(feature = "ocr")]
use crate::overlay::OcrSelection;
use crate::overlay::{CopySelection, QuitOverlay, SaveSelection};
use crate::theme;

/// Toolbar: placed 8px below the selection's bottom-left corner (above the
/// selection when there's no room below), horizontally clamped into the screen.
/// [Copy][Save][OCR][Cancel]
pub fn selection_toolbar(b: Bounds<Pixels>, ws: Size<Pixels>) -> impl IntoElement {
    // Rough size estimate (buttons + gaps + padding), good enough for now
    #[cfg(feature = "ocr")]
    const TB_W: f32 = 320.;
    #[cfg(not(feature = "ocr"))]
    const TB_W: f32 = 245.;
    const TB_H: f32 = 40.;
    let y = if f32::from(b.bottom()) + TB_H + 8. <= f32::from(ws.height) {
        b.bottom() + px(8.)
    } else {
        b.top() - px(TB_H) - px(8.)
    };
    let x = f32::from(b.left()).clamp(8., f32::from(ws.width) - TB_W - 8.);

    div()
        .id("shotori-toolbar")
        .absolute()
        .left(px(x))
        .top(y)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_lg()
        .bg(rgba(theme::CHIP_BG))
        .border_1()
        .border_color(rgba(theme::ACCENT))
        // Key: clicks inside the toolbar must not bubble to the root node —
        // otherwise pressing a button would start a new selection
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(toolbar_button("tb-copy", "Copy", |window, cx| {
            window.dispatch_action(Box::new(CopySelection), cx);
        }))
        .child(toolbar_button("tb-save", "Save", |window, cx| {
            window.dispatch_action(Box::new(SaveSelection), cx);
        }))
        // .children() takes an IntoIterator — Option works as "0 or 1 child",
        // letting the OCR button compile away in slim builds
        .children(ocr_button())
        .child(toolbar_button("tb-cancel", "Cancel", |window, cx| {
            window.dispatch_action(Box::new(QuitOverlay), cx);
        }))
}

/// The OCR button; `None` when the `ocr` feature is compiled out
/// (`.children()` takes an IntoIterator, so Option works as "0 or 1 child").
#[cfg(feature = "ocr")]
fn ocr_button() -> Option<impl IntoElement> {
    Some(toolbar_button("tb-ocr", "OCR", |window, cx| {
        window.dispatch_action(Box::new(OcrSelection), cx);
    }))
}
#[cfg(not(feature = "ocr"))]
fn ocr_button() -> Option<&'static str> {
    None
}

/// Hand-drawn toolbar button: the base `Button` provides behavior (click /
/// focus / hover state machine / accessibility), we only paint the skin —
/// the standard move for the gpui-base custom-drawing route.
fn toolbar_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Button::new(id)
        .on_click(move |_, window, cx| on_click(window, cx))
        .px_3()
        .py_1()
        .rounded(px(6.))
        .text_size(px(13.))
        .text_color(rgba(theme::BTN_TEXT))
        .hover(|s| s.bg(rgba(theme::BTN_HOVER_BG)))
        .child(label)
}
