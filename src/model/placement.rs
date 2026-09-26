//! # Placement: the two-zone chrome layout contract
//!
//! The user-designed scheme (2026-09-26): the **label owns the top zone**
//! (above the selection, or inside its top-left corner when the selection
//! hugs the screen top) and the **toolbar owns the bottom zone** (below
//! the selection, or inside its bottom-left corner when it reaches the
//! screen bottom). The zones cannot intersect by construction, the label
//! never depends on the toolbar's existence (no "reserving room" jumps
//! while dragging), and nothing renders off-screen.
//!
//! Both anchors are pure functions; the invariant is enforced by a grid
//! sweep test over 35 selection geometries. `ui::hud` and `ui::toolbar`
//! consume these — this module is the single source of truth.

use gpui_kit::*;

/// Rough label width covering the widest "3072 × 1920" + padding; the
/// height matches the rendered chip.
const LABEL_W: f32 = 110.;
pub(crate) const LABEL_H: f32 = 24.;

/// Inset kept between the box border and elements drawn INSIDE it —
/// flush against the border line looks glued-on (user-reported).
const INSET: f32 = 12.;

/// Toolbar: [Copy][Save][OCR][Cancel] on row one; annotation tools,
/// colors and widths on row two (only while a tool is active).
pub(crate) const TB_W: f32 = 460.;
/// Single-row height (tools inactive)
pub(crate) const ROW_H: f32 = 38.;
/// Two-row height (the tall case used for placement decisions)
pub(crate) const TB_H: f32 = ROW_H * 2. + 6.;

/// Breathing room kept between the lowest element and the screen edge —
/// "fits at exactly zero margin" still looks glued on (measured).
const EDGE_B: f32 = 12.;

/// Label placement: ABOVE the selection, or — when the selection hugs
/// the top of the screen — INSIDE the box at its top-left corner. Never
/// below (see the module doc).
pub(crate) fn label_anchor(b: &Bounds<Pixels>, ws: Size<Pixels>) -> (f32, f32) {
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

/// Toolbar placement: BELOW the selection, or — when the selection
/// reaches the bottom of the screen — INSIDE the box at its bottom-left
/// corner. Horizontally clamped; `height` is the toolbar's current
/// height (single row, or two rows while annotating).
pub(crate) fn toolbar_anchor(b: &Bounds<Pixels>, ws: Size<Pixels>, height: f32) -> (f32, f32) {
    let inside = f32::from(b.bottom()) + height + 8. + EDGE_B > f32::from(ws.height);
    let x = (f32::from(b.left()) + if inside { INSET } else { 0. })
        .clamp(8., (f32::from(ws.width) - TB_W - 8.).max(8.));
    let y = if inside {
        // inside, bottom-left corner (inset from the border)
        f32::from(b.bottom()) - height - 8.
    } else {
        f32::from(b.bottom()) + 8.
    };
    (x, y.clamp(8., (f32::from(ws.height) - height - 8.).max(8.)))
}

#[cfg(test)]
mod tests {
    // Explicit imports (same reason as selection.rs: avoid gpui's test
    // macro shadowing the built-in #[test])
    use super::{LABEL_H, ROW_H, TB_H, label_anchor, toolbar_anchor};
    use gpui_kit::{Bounds, Pixels, point, px, size};

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    fn screen() -> gpui_kit::Size<Pixels> {
        size(px(1920.), px(1080.))
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
        let (x, y) = toolbar_anchor(&bounds(50., 100., 300., 200.), screen(), ROW_H);
        assert_eq!((x, y), (50., 308.));
    }

    #[test]
    fn toolbar_goes_inside_bottom_left_when_reaching_the_bottom() {
        // two-row toolbar (annotating): the tall case
        let (x, y) = toolbar_anchor(&bounds(50., 300., 300., 780.), screen(), TB_H);
        assert_eq!((x, y), (50. + 12., 1080. - TB_H - 8.));
    }

    #[test]
    fn toolbar_clamps_horizontally() {
        // a selection hugging the right edge: the toolbar pins into the screen
        let b = bounds(1800., 500., 100., 200.);
        let (x, _) = toolbar_anchor(&b, screen(), TB_H);
        assert_eq!(x, 1920. - 460. - 8.);
    }

    #[test]
    fn zones_stay_disjoint_across_a_grid_of_selections() {
        // The scheme's core invariant: the label zone (top) and the toolbar
        // zone (bottom) never overlap and never leave the screen — swept
        // over a representative grid of selection geometries, with the
        // toolbar at its tallest (two rows while annotating).
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
