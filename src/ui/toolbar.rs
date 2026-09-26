//! # Selection toolbar: Copy / Save / OCR / Cancel
//!
//! Buttons dispatch the exact same actions as the keyboard through
//! `dispatch_action` — one action, two triggers, one pipeline.
//! Visibility is decided by the overlay: it only appears after the selection
//! is finalized ([`crate::model::selection::Selection::is_selected`]).

use gpui_kit::base::Button;
use gpui_kit::*;

use crate::actions::{CopySelection, OcrSelection, QuitOverlay, SaveSelection};
use crate::model::placement::toolbar_anchor;
use crate::ui::theme;

pub fn selection_toolbar(b: Bounds<Pixels>, ws: Size<Pixels>) -> impl IntoElement {
    let (x, y) = toolbar_anchor(&b, ws);

    div()
        .id("shotori-toolbar")
        .absolute()
        .left(px(x))
        .top(px(y))
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
        .child(toolbar_button("tb-ocr", "OCR", |window, cx| {
            window.dispatch_action(Box::new(OcrSelection), cx);
        }))
        .child(toolbar_button("tb-cancel", "Cancel", |window, cx| {
            window.dispatch_action(Box::new(QuitOverlay), cx);
        }))
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
