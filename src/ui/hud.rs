//! # Overlay HUD: pure visual elements tied to the selection
//!
//! Dim strips / selection border + size label / bottom hint bar.
//! Stateless, all functional; assembled in [`crate::ui::overlay::Overlay::render`].

use gpui_kit::*;

use crate::model::placement::label_anchor;
use crate::ui::theme;

/// Paint the dim layer and border together. Separate positioned divs snap
/// their origins and sizes independently during layout; at fractional DPI
/// their edges can differ by a device pixel. Painting shared edges bypasses
/// that layout rounding and lets GPUI snap each absolute edge consistently.
pub(crate) fn selection_backdrop(sel: Option<Bounds<Pixels>>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |viewport, (), window, _| {
            let Some(mut b) = sel else {
                window.paint_quad(fill(viewport, rgba(theme::c().dim())));
                return;
            };
            b.origin += viewport.origin;
            let border = b;
            b = b.intersect(&viewport);
            let strips = [
                Bounds::from_corners(viewport.origin, point(viewport.right(), b.top())),
                Bounds::from_corners(point(viewport.left(), b.bottom()), viewport.bottom_right()),
                Bounds::from_corners(point(viewport.left(), b.top()), point(b.left(), b.bottom())),
                Bounds::from_corners(
                    point(b.right(), b.top()),
                    point(viewport.right(), b.bottom()),
                ),
            ];
            for strip in strips {
                if strip.size.width > px(0.) && strip.size.height > px(0.) {
                    window.paint_quad(fill(strip, rgba(theme::c().dim())));
                }
            }
            window.paint_quad(outline(
                border,
                rgba(theme::c().accent),
                BorderStyle::default(),
            ));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Hover highlight for window snapping: a bare 2px accent outline on
/// the window under the cursor. Outline only — it must read as "a click
/// will select this", not as a selection (no dim change, no fill, no
/// label; those belong to a real selection).
pub(crate) fn hover_outline(b: Bounds<Pixels>) -> impl IntoElement {
    div()
        .absolute()
        .left(b.origin.x)
        .top(b.origin.y)
        .w(b.size.width)
        .h(b.size.height)
        .border_2()
        .border_color(rgba(theme::c().accent))
}

/// Selection size label. The label tries above the selection,
/// then below, and when neither fits (a selection spanning the screen
/// height) it is drawn INSIDE the selection box — overlaid beats
/// off-screen. It must NOT live inside the border box when avoidable: a
/// narrow selection would clamp the label's width to the selection's,
/// wrapping "W × H" into a one-character-per-line tower. As a sibling
/// anchored to the overlay root it stays content-sized.
pub(crate) fn selection_label(
    b: Bounds<Pixels>,
    ws: Size<Pixels>,
    selected_size: Size<Pixels>,
) -> AnyElement {
    let (label_x, label_y) = label_anchor(&b, ws);

    div()
        .absolute()
        .left(px(label_x))
        .top(px(label_y))
        .px_2()
        .py(px(2.))
        .rounded(px(4.))
        .bg(rgba(theme::c().accent))
        .text_size(px(12.))
        .text_color(rgba(0xFFFFFFFF))
        .child(format!(
            "{} × {}",
            f32::from(selected_size.width).round() as i32,
            f32::from(selected_size.height).round() as i32
        ))
        .into_any_element()
}

/// Bottom hint bar
pub(crate) fn hint_bar(text: Option<&'static str>) -> impl IntoElement {
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
                .bg(rgba(theme::c().chip_bg))
                .text_size(px(13.))
                .text_color(rgba(theme::c().hint_text))
                .child(text.unwrap_or_else(hint_text)),
        )
}

/// Hint text is trimmed per feature set: builds without `ocr` must not
/// advertise a shortcut that does not exist
fn hint_text() -> &'static str {
    "Drag to select · Enter copy · Ctrl+S save · Ctrl+O OCR · Esc exit"
}

// ── OCR busy badge (spinner) ──────────────────────────────────────────

/// The busy badge: a spinner + label, centered on the selection (or the
/// window when nothing is selected), clamped on-screen.
pub(crate) fn ocr_busy_badge(sel: Option<Bounds<Pixels>>, ws: Size<Pixels>) -> AnyElement {
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
        .bg(rgba(theme::c().chip_bg))
        .border_1()
        .border_color(rgba(theme::c().accent))
        .child(spinner())
        .child(
            div()
                .text_size(px(13.))
                .text_color(rgba(theme::c().hint_text))
                .child("OCR…"),
        )
        .into_any_element()
}

/// Spinner: a faint ring with one accent dot orbiting inside. Pure element
/// properties animated via `with_animation` (respects reduce_motion;
/// max_fps caps the redraw rate).
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
                .border_color(rgba(crate::ui::theme::c().pin_border)),
        )
        // the orbiting dot
        .child(
            div()
                .absolute()
                .size(px(DOT))
                .rounded(px(DOT / 2.))
                .bg(rgba(crate::ui::theme::c().accent))
                .with_animation(
                    "shotori-spin",
                    Animation::new(std::time::Duration::from_millis(900))
                        .repeat()
                        .with_max_fps(15.),
                    move |dot, delta| {
                        let a = delta * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                        let (dx, dy) = (a.cos() * R, a.sin() * R);
                        dot.left(px(CENTER + dx - DOT / 2.))
                            .top(px(CENTER + dy - DOT / 2.))
                    },
                ),
        )
}

#[cfg(test)]
mod tests {
    // Explicit imports (same reason as selection.rs: avoid gpui's test macro
    // shadowing the built-in #[test])
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

    struct BackdropHarness {
        selection: Option<Bounds<Pixels>>,
    }

    impl gpui_kit::Render for BackdropHarness {
        fn render(
            &mut self,
            _: &mut gpui_kit::Window,
            _: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            use gpui_kit::{ParentElement, Styled};
            gpui_kit::div()
                .relative()
                .size_full()
                .child(super::selection_backdrop(self.selection))
        }
    }

    // Inspect GPUI's actual device-pixel quads after layout and painting.
    // Each pixel outside the border must have exactly one dim layer:
    // zero produces a bright seam, two produce a dark seam.
    #[gpui_kit::test]
    fn backdrop_has_no_gaps_or_overlaps_at_fractional_dpi(cx: &mut gpui_kit::TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| BackdropHarness { selection: None });
        cx.update(|window, _| window.resize(ws(400., 400.)));
        for scale in [1., 1.25, 1.5, 1.75, 2.] {
            cx.simulate_scale_factor_change(scale);
            let selections = [
                None,
                Some(bounds(50., 101., 200., 230.)),
                Some(bounds(51., 102., 201., 229.)),
                Some(bounds(50., 103., 200., 230.)),
                Some(bounds(50., 104., 200., 230.)),
                Some(bounds(0., 0., 200., 230.)),
                Some(bounds(200., 170., 200., 230.)),
                Some(bounds(0., 0., 400., 400.)),
                // A global selection can continue beyond this output. Its
                // border must stay at the real edges, not the monitor seam.
                Some(bounds(-100., 10., 600., 380.)),
                Some(bounds(10., -100., 380., 600.)),
            ];
            for selection in selections {
                view.update(cx, |view, cx| {
                    view.selection = selection;
                    cx.notify();
                });
                let quads = cx.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    window.painted_quads()
                });
                let border = quads.iter().find(|q| q.border_widths.top.0 > 0.);
                assert_eq!(border.is_some(), selection.is_some());
                let contains = |b: &gpui_kit::Bounds<gpui_kit::ScaledPixels>, x: f32, y: f32| {
                    x >= b.left().0 && x < b.right().0 && y >= b.top().0 && y < b.bottom().0
                };
                for y in 0..(400. * scale) as usize {
                    for x in 0..(400. * scale) as usize {
                        let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
                        let inside = border.is_some_and(|q| contains(&q.bounds, x, y));
                        let layers = quads
                            .iter()
                            .filter(|q| q.border_widths.top.0 == 0. && contains(&q.bounds, x, y))
                            .count();
                        assert_eq!(
                            layers,
                            usize::from(!inside),
                            "scale={scale}, selection={selection:?}, pixel=({x}, {y})"
                        );
                    }
                }
            }
        }
    }
}
