//! # Overlay HUD: pure visual elements tied to the selection
//!
//! Dim strips / selection border + size label / bottom hint bar.
//! Stateless, all functional; assembled in [`crate::overlay::Overlay::render`].

use gpui_kit::*;

use crate::theme::{ACCENT, CHIP_BG, DIM, HINT_TEXT};

/// Dim layer: one full-screen block when nothing is selected; when a
/// selection exists, four strips around it (the selection "sees through")
pub(crate) fn dim_strips(sel: Option<Bounds<Pixels>>, ws: Size<Pixels>) -> Vec<AnyElement> {
    let mut els = Vec::new();
    let mut strip = |x: Pixels, y: Pixels, w: Pixels, h: Pixels| {
        if w > px(0.) && h > px(0.) {
            els.push(
                div()
                    .absolute()
                    .left(x)
                    .top(y)
                    .w(w)
                    .h(h)
                    .bg(rgba(DIM))
                    .into_any_element(),
            );
        }
    };

    match sel {
        None => strip(px(0.), px(0.), ws.width, ws.height),
        Some(b) => {
            strip(px(0.), px(0.), ws.width, b.top()); // top
            strip(px(0.), b.bottom(), ws.width, ws.height - b.bottom()); // bottom
            strip(px(0.), b.top(), b.left(), b.size.height); // left
            strip(b.right(), b.top(), ws.width - b.right(), b.size.height); // right
        }
    }
    els
}

/// Selection border + size label (label above the selection; below when no room)
pub(crate) fn selection_chrome(b: Bounds<Pixels>) -> impl IntoElement {
    let label_y = if b.top() >= px(34.) {
        b.top() - px(30.)
    } else {
        b.bottom() + px(6.)
    };

    div()
        .absolute()
        .left(b.left())
        .top(b.top())
        .w(b.size.width)
        .h(b.size.height)
        .border_1()
        .border_color(rgba(ACCENT))
        .child(
            div()
                .absolute()
                .left(px(0.))
                .top(label_y - b.top())
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .bg(rgba(ACCENT))
                .text_size(px(12.))
                .text_color(rgba(0xFFFFFFFF))
                .child(format!(
                    "{} × {}",
                    f32::from(b.size.width).round() as i32,
                    f32::from(b.size.height).round() as i32
                )),
        )
}

/// Bottom hint bar
pub(crate) fn hint_bar() -> impl IntoElement {
    div()
        .absolute()
        .bottom(px(24.))
        .left_0()
        .w_full()
        .flex()
        .justify_center()
        .child(
            div()
                .px_4()
                .py_1()
                .rounded_lg()
                .bg(rgba(CHIP_BG))
                .text_size(px(13.))
                .text_color(rgba(HINT_TEXT))
                .child(hint_text()),
        )
}

/// Hint text is trimmed per feature set: builds without `ocr` must not
/// advertise a shortcut that does not exist
#[cfg(feature = "ocr")]
fn hint_text() -> &'static str {
    "Drag to select · Enter copy · Ctrl+S save · Ctrl+O OCR · Esc exit"
}
#[cfg(not(feature = "ocr"))]
fn hint_text() -> &'static str {
    "Drag to select · Enter copy · Ctrl+S save · Esc exit"
}