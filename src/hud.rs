//! # Overlay HUD: pure visual elements tied to the selection
//!
//! Dim strips / selection border + size label / bottom hint bar.
//! Stateless, all functional; assembled in [`crate::overlay::Overlay::render`].

use gpui_kit::*;

use crate::theme::{ACCENT, CHIP_BG, DIM, HINT_TEXT};

/// Paint the dim layer and border together. Separate positioned divs snap
/// their origins and sizes independently during layout; at fractional DPI
/// their edges can differ by a device pixel. Painting shared edges bypasses
/// that layout rounding and lets GPUI snap each absolute edge consistently.
pub(crate) fn selection_backdrop(sel: Option<Bounds<Pixels>>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |viewport, (), window, _| {
            let Some(mut b) = sel else {
                window.paint_quad(fill(viewport, rgba(DIM)));
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
                    window.paint_quad(fill(strip, rgba(DIM)));
                }
            }
            window.paint_quad(outline(border, rgba(ACCENT), BorderStyle::default()));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
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
        .bg(rgba(ACCENT))
        .text_size(px(12.))
        .text_color(rgba(0xFFFFFFFF))
        .child(format!(
            "{} × {}",
            f32::from(selected_size.width).round() as i32,
            f32::from(selected_size.height).round() as i32
        ))
        .into_any_element()
}

/// Label geometry, pure for tests. Rough width covers the widest
/// "3072 × 1920" + padding; the height matches the rendered chip.
const LABEL_W: f32 = 110.;
pub(crate) const LABEL_H: f32 = 24.;

/// Inset kept between the box border and elements drawn INSIDE it —
/// flush against the border line looks glued-on (user-reported).
const INSET: f32 = 12.;

/// Label placement: ABOVE the selection, or — when the selection hugs the
/// top of the screen — INSIDE the box at its top-left corner. Never below:
/// the label owns the top zone and the toolbar owns the bottom zone
/// (hud::label_anchor / toolbar::toolbar_anchor), so the two cannot
/// collide by construction, and the label never depends on the toolbar's
/// existence (no "reserving room" jumps while dragging).
fn label_anchor(b: &Bounds<Pixels>, ws: Size<Pixels>) -> (f32, f32) {
    let inside = f32::from(b.top()) < LABEL_H + 10.;
    let x = (f32::from(b.left()) + if inside { INSET } else { 0. })
        .clamp(4., (f32::from(ws.width) - LABEL_W - 4.).max(4.));
    let y = if inside {
        // inside, top-left corner
        f32::from(b.top()) + 8.
    } else {
        f32::from(b.top()) - LABEL_H - 6.
    };
    (x, y)
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
    use super::{LABEL_H, label_anchor};
    use crate::toolbar::{TB_H, toolbar_anchor};
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

    fn screen() -> gpui_kit::Size<Pixels> {
        ws(1920., 1080.)
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

    #[test]
    fn label_sits_above_by_default() {
        let (x, y) = label_anchor(&bounds(50., 100., 300., 200.), screen());
        assert_eq!((x, y), (50., 70.));
    }

    #[test]
    fn label_goes_inside_top_left_when_hugging_the_top() {
        let (x, y) = label_anchor(&bounds(50., 0., 300., 200.), screen());
        assert_eq!((x, y), (50. + 12., 8.));
    }

    #[test]
    fn label_stays_put_while_dragging_near_the_bottom() {
        // The label must not depend on the toolbar (which only exists after
        // release): a drag reaching the screen bottom keeps the label above
        let (_, y) = label_anchor(&bounds(50., 300., 300., 779.), screen());
        assert_eq!(y, 270.);
    }

    #[test]
    fn label_clamps_near_the_right_edge() {
        let (x, _) = label_anchor(&bounds(1900., 100., 20., 200.), screen());
        assert_eq!(x, 1920. - 110. - 4.);
    }

    #[test]
    fn toolbar_sits_below_by_default() {
        let (x, y) = toolbar_anchor(&bounds(50., 100., 300., 200.), screen(), TB_H);
        assert_eq!((x, y), (50., 308.));
    }

    #[test]
    fn toolbar_goes_inside_bottom_left_when_reaching_the_bottom() {
        let (x, y) = toolbar_anchor(&bounds(50., 300., 300., 780.), screen(), TB_H);
        assert_eq!((x, y), (50. + 12., 1080. - TB_H - 8.));
    }

    #[test]
    fn zones_stay_disjoint_across_a_grid_of_selections() {
        // The scheme's core invariant: the label zone (top) and the toolbar
        // zone (bottom) never overlap and never leave the screen — swept
        // over a representative grid of selection geometries.
        for top in [0., 4., 34., 50., 78., 200., 800.] {
            for bottom in [top + 40., 1000., 1040., 1072., 1080.] {
                if bottom <= top || bottom > 1080. {
                    continue;
                }
                let b = bounds(50., top, 300., bottom - top);
                let (_, ly) = label_anchor(&b, screen());
                let (_, ty) = toolbar_anchor(&b, screen(), TB_H);
                assert!(ly >= 0., "label off-screen for {b:?}");
                assert!(
                    ty >= 0. && ty + TB_H <= 1080.,
                    "toolbar off-screen for {b:?}"
                );
                assert!(
                    ly + LABEL_H <= ty,
                    "zones overlap for {b:?}: label {}..{}, toolbar {ty}..{}",
                    ly,
                    ly + LABEL_H,
                    ty + TB_H
                );
            }
        }
    }
}
