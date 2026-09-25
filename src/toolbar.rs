//! # Selection toolbar: Copy / Save / OCR / Cancel
//!
//! Buttons dispatch the exact same actions as the keyboard through
//! `dispatch_action` — one action, two triggers, one pipeline.
//! Visibility is decided by the overlay: it only appears after the selection
//! is finalized ([`crate::selection::Selection::is_selected`]).

use gpui_kit::base::Button;
use gpui_kit::*;

use crate::overlay::{CopySelection, OcrSelection, QuitOverlay, SaveSelection};
use crate::theme;

/// Toolbar: BELOW the selection, or — when the selection reaches the
/// bottom of the screen — INSIDE the box at its bottom-left corner. Never
/// above: the label owns the top zone, the toolbar the bottom zone, so
/// they cannot collide by construction (see hud::label_anchor).
/// [Copy][Save][OCR][Cancel]
const TB_W: f32 = 320.;
pub(crate) const TB_H: f32 = 40.;
/// Breathing room kept between the lowest element and the screen edge —
/// "fits at exactly zero margin" still looks glued on (measured).
const EDGE_B: f32 = 12.;

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

/// Toolbar placement (pure, tested): below the selection, or inside its
/// bottom-left corner when the screen ends first. Horizontally clamped.
pub(crate) fn toolbar_anchor(b: &Bounds<Pixels>, ws: Size<Pixels>) -> (f32, f32) {
    let inside = f32::from(b.bottom()) + TB_H + 8. + EDGE_B > f32::from(ws.height);
    let x = (f32::from(b.left()) + if inside { 12. } else { 0. })
        .clamp(8., (f32::from(ws.width) - TB_W - 8.).max(8.));
    let y = if inside {
        // inside, bottom-left corner (inset from the border)
        f32::from(b.bottom()) - TB_H - 8.
    } else {
        f32::from(b.bottom()) + 8.
    };
    (x, y)
}

#[cfg(test)]
mod tests {
    // The placement invariants (disjoint zones, on-screen) are swept in
    // hud::tests; here only the clamp itself.
    use super::TB_W;
    use gpui_kit::{px, size};

    #[test]
    fn toolbar_clamps_horizontally() {
        // a selection hugging the right edge: the toolbar pins into the screen
        let left: f32 = 1800.;
        let ws = size(px(1920.), px(1080.));
        let x = left.clamp(8., (f32::from(ws.width) - TB_W - 8.).max(8.));
        assert_eq!(x, 1920. - TB_W - 8.);
    }
}
