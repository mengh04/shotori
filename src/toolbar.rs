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

/// Toolbar: placed 8px below the selection's bottom-left corner, above the
/// selection when there's no room below, and INSIDE the selection box when
/// neither fits (a selection spanning the screen height). Horizontally
/// clamped into the screen.
/// [Copy][Save][OCR][Cancel]
pub fn selection_toolbar(b: Bounds<Pixels>, ws: Size<Pixels>) -> impl IntoElement {
    // Rough size estimate (buttons + gaps + padding), good enough for now
    const TB_W: f32 = 320.;
    const TB_H: f32 = 40.;
    let (x, y) = toolbar_anchor(&b, ws, TB_W, TB_H);

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

/// Toolbar geometry, pure for tests: below the selection, above it, or —
/// when the selection spans the screen height — inside it, pinned to the
/// selection's top edge. Off-screen is never an option: the buttons are
/// the only mouse-driven exit.
fn toolbar_anchor(b: &Bounds<Pixels>, ws: Size<Pixels>, tb_w: f32, tb_h: f32) -> (f32, f32) {
    let x = f32::from(b.left()).clamp(8., (f32::from(ws.width) - tb_w - 8.).max(8.));
    let y = if f32::from(b.bottom()) + tb_h + 8. <= f32::from(ws.height) {
        f32::from(b.bottom()) + 8.
    } else if f32::from(b.top()) >= tb_h + 8. {
        f32::from(b.top()) - tb_h - 8.
    } else {
        f32::from(b.top()) + 6.
    };
    (x, y)
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

#[cfg(test)]
mod tests {
    // Explicit imports (same reason as selection.rs: avoid gpui's test macro
    // shadowing the built-in #[test])
    use super::toolbar_anchor;
    use gpui_kit::{Bounds, Pixels, point, px, size};

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    fn ws(w: f32, h: f32) -> gpui_kit::Size<Pixels> {
        size(px(w), px(h))
    }

    #[test]
    fn toolbar_prefers_below() {
        let (x, y) = toolbar_anchor(&bounds(50., 100., 300., 200.), ws(1920., 1080.), 320., 40.);
        assert_eq!((x, y), (50., 300. + 8.));
    }

    #[test]
    fn toolbar_flips_above_near_the_bottom() {
        // bottom + 40 + 8 > 1080 → above
        let (_, y) = toolbar_anchor(&bounds(50., 600., 300., 460.), ws(1920., 1080.), 320., 40.);
        assert_eq!(y, 600. - 40. - 8.);
    }

    #[test]
    fn toolbar_goes_inside_when_spanning_the_screen() {
        // full-height selection: neither above nor below exists
        let (_, y) = toolbar_anchor(&bounds(50., 0., 300., 1080.), ws(1920., 1080.), 320., 40.);
        assert_eq!(y, 6.);
    }

    #[test]
    fn toolbar_clamps_horizontally() {
        let (x, _) = toolbar_anchor(
            &bounds(1800., 100., 100., 200.),
            ws(1920., 1080.),
            320.,
            40.,
        );
        assert_eq!(x, 1920. - 320. - 8.);
    }
}
