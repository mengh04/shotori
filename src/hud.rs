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

/// Selection border + size label (label above the selection; below when no
/// room). Returns TWO window-anchored elements: the label must NOT live
/// inside the border box — a narrow selection would clamp the label's width
/// to the selection's, wrapping "W × H" into a one-character-per-line
/// tower. As a sibling anchored to the overlay root it is content-sized.
pub(crate) fn selection_chrome(b: Bounds<Pixels>) -> Vec<AnyElement> {
    let label_y = if b.top() >= px(34.) {
        b.top() - px(30.)
    } else {
        b.bottom() + px(6.)
    };

    let border = div()
        .absolute()
        .left(b.left())
        .top(b.top())
        .w(b.size.width)
        .h(b.size.height)
        .border_1()
        .border_color(rgba(ACCENT))
        .into_any_element();

    let label = div()
        .absolute()
        .left(b.left())
        .top(label_y)
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
        ))
        .into_any_element();

    vec![border, label]
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

// ── OCR busy badge (spinner) ──────────────────────────────────────────

/// The busy badge: a spinner + label, centered on the selection (or the
/// window when nothing is selected), clamped on-screen.
#[cfg(feature = "ocr")]
pub(crate) fn ocr_busy_badge(
    sel: Option<Bounds<Pixels>>,
    ws: Size<Pixels>,
) -> AnyElement {
    const BADGE_W: f32 = 118.;
    const BADGE_H: f32 = 40.;
    let (cx, cy) = match sel {
        Some(b) => (
            f32::from(b.left()) + f32::from(b.size.width) / 2.,
            f32::from(b.top()) + f32::from(b.size.height) / 2.,
        ),
        None => (f32::from(ws.width) / 2., f32::from(ws.height) / 2.),
    };
    let x = (cx - BADGE_W / 2.).clamp(8., f32::from(ws.width) - BADGE_W - 8.);
    let y = (cy - BADGE_H / 2.).clamp(8., f32::from(ws.height) - BADGE_H - 8.);

    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(rgba(CHIP_BG))
        .border_1()
        .border_color(rgba(ACCENT))
        .child(spinner())
        .child(
            div()
                .text_size(px(13.))
                .text_color(rgba(HINT_TEXT))
                .child("OCR…"),
        )
        .into_any_element()
}

/// Spinner: a faint ring with one accent dot orbiting inside. Pure element
/// properties animated via `with_animation` (respects reduce_motion;
/// max_fps caps the redraw rate).
#[cfg(feature = "ocr")]
fn spinner() -> impl IntoElement {
    const R: f32 = 7.; // orbit radius
    const BOX: f32 = 2. * R + 5.; // container edge
    const CENTER: f32 = BOX / 2.;
    const DOT: f32 = 4.;

    div()
        .id("shotori-spinner")
        .relative()
        .size(px(BOX))
        // faint ring for context
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(CENTER))
                .border_1()
                .border_color(rgba(crate::theme::PIN_BORDER)),
        )
        // the orbiting dot
        .child(
            div()
                .absolute()
                .size(px(DOT))
                .rounded(px(DOT / 2.))
                .bg(rgba(crate::theme::ACCENT))
                .with_animation(
                    "shotori-spin",
                    Animation::new(std::time::Duration::from_millis(900))
                        .repeat()
                        .with_max_fps(15.),
                    move |dot, delta| {
                        let a = delta * std::f32::consts::TAU
                            - std::f32::consts::FRAC_PI_2;
                        let (dx, dy) = (a.cos() * R, a.sin() * R);
                        dot.left(px(CENTER + dx - DOT / 2.))
                            .top(px(CENTER + dy - DOT / 2.))
                    },
                ),
        )
}