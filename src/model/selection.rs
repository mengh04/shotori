//! # Selection state machine: the pure-logic core of overlay interaction
//!
//! Three-state lifecycle: `Idle` (no selection) → `Dragging` (held down)
//! → `Selected` (released and finalized). No rendering/UI dependencies →
//! unit-testable without a compositor; the render layer only reads
//! [`Selection::bounds`].
//!
//! Interaction semantics (since v0.2):
//! - An in-place **click** (drag < 2px) clears the selection instead of
//!   producing a weird 0×0 selection
//! - [`Selection::cancel_drag`] while dragging (Esc) only abandons this drag

use gpui_kit::*;

/// Threshold for "click, not drag" (logical pixels): a selection with either
/// side smaller than this is considered meaningless
const MIN_SIZE: f32 = 2.0;

#[derive(Clone, Copy)]
pub enum Selection {
    Idle,
    Dragging {
        start: Point<Pixels>,
        current: Point<Pixels>,
    },
    Selected {
        bounds: Bounds<Pixels>,
    },
}

impl Selection {
    /// Press: start a new selection (also used to re-select while Selected)
    pub fn begin(&mut self, p: Point<Pixels>) {
        *self = Self::Dragging {
            start: p,
            current: p,
        };
    }

    /// Drag to p; returns whether anything actually changed (callers decide
    /// on cx.notify() accordingly)
    pub fn drag_to(&mut self, p: Point<Pixels>) -> bool {
        if let Self::Dragging { current, .. } = self
            && *current != p
        {
            *current = p;
            return true;
        }
        false
    }

    /// Release: a real drag → `Selected` (corners normalized, dragging
    /// up-left works too); an in-place click (< [`MIN_SIZE`]) → `Idle`
    /// (clears the selection)
    pub fn end(&mut self, p: Point<Pixels>) {
        if let Self::Dragging { start, .. } = *self {
            let bounds = Self::normalized(start, p);
            *self = if f32::from(bounds.size.width) < MIN_SIZE
                || f32::from(bounds.size.height) < MIN_SIZE
            {
                Self::Idle
            } else {
                Self::Selected { bounds }
            };
        }
    }

    /// Cancel while dragging (first-stage Esc semantics); no-op otherwise
    pub fn cancel_drag(&mut self) {
        if matches!(self, Self::Dragging { .. }) {
            *self = Self::Idle;
        }
    }

    /// Current selection (counts while dragging too — used by the live size label)
    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        match *self {
            Self::Idle => None,
            Self::Dragging { start, current } => Some(Self::normalized(start, current)),
            Self::Selected { bounds } => Some(bounds),
        }
    }

    /// Two corners → normalized Bounds (top-left = origin, positive size).
    /// `Bounds::from_corners` must not be used: it does not sort, so dragging
    /// toward the top-left produces negative width/height (a latent v0.1 bug:
    /// an "invisible selection" that made Enter report an empty selection)
    fn normalized(a: Point<Pixels>, b: Point<Pixels>) -> Bounds<Pixels> {
        let (left, right) = if a.x <= b.x { (a.x, b.x) } else { (b.x, a.x) };
        let (top, bottom) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
        Bounds {
            origin: point(left, top),
            size: size(right - left, bottom - top),
        }
    }

    pub fn is_dragging(&self) -> bool {
        matches!(self, Self::Dragging { .. })
    }

    /// Finalized? (the toolbar only appears in this state)
    pub fn is_selected(&self) -> bool {
        matches!(self, Self::Selected { .. })
    }
}

#[cfg(test)]
mod tests {
    // Deliberately not `use super::*`: the parent module's `use gpui_kit::*`
    // pulls in gpui's own `test` attribute macro which shadows the built-in
    // #[test] (macro expansion hits the recursion limit)
    use super::Selection;
    use gpui_kit::{Pixels, Point, point, px};

    fn pt(x: f32, y: f32) -> Point<Pixels> {
        point(px(x), px(y))
    }

    fn size_of(s: &Selection) -> (f32, f32) {
        let b = s.bounds().expect("selection should exist");
        (f32::from(b.size.width), f32::from(b.size.height))
    }

    #[test]
    fn drag_up_left_normalizes_bounds() {
        let mut s = Selection::Idle;
        s.begin(pt(100., 100.));
        s.drag_to(pt(40., 60.));
        s.end(pt(40., 60.));
        let b = s.bounds().unwrap();
        assert_eq!(f32::from(b.left()), 40.);
        assert_eq!(f32::from(b.top()), 60.);
        assert_eq!(size_of(&s), (60., 40.));
        assert!(s.is_selected());
    }

    #[test]
    fn in_place_click_clears_selection() {
        let mut s = Selection::Idle;
        s.begin(pt(50., 50.));
        s.end(pt(50., 50.));
        assert!(matches!(s, Selection::Idle));
    }

    #[test]
    fn tiny_drag_treated_as_click() {
        let mut s = Selection::Idle;
        s.begin(pt(10., 10.));
        s.end(pt(11.5, 10.)); // 1.5px < 2px
        assert!(matches!(s, Selection::Idle));
    }

    #[test]
    fn click_with_existing_selection_restarts() {
        let mut s = Selection::Idle;
        s.begin(pt(0., 0.));
        s.end(pt(100., 100.));
        assert!(s.is_selected());
        s.begin(pt(200., 200.)); // press while Selected = new selection
        assert!(s.is_dragging());
    }

    #[test]
    fn cancel_drag_returns_to_idle_without_touching_finalized() {
        let mut s = Selection::Idle;
        s.begin(pt(0., 0.));
        s.drag_to(pt(30., 30.));
        s.cancel_drag();
        assert!(matches!(s, Selection::Idle));

        let mut s = Selection::Idle;
        s.begin(pt(0., 0.));
        s.end(pt(30., 30.));
        s.cancel_drag(); // already finalized: untouched (Esc means exit here)
        assert!(s.is_selected());
    }

    #[test]
    fn drag_to_returns_false_when_unchanged() {
        let mut s = Selection::Idle;
        s.begin(pt(5., 5.));
        assert!(s.drag_to(pt(9., 5.)));
        assert!(!s.drag_to(pt(9., 5.))); // same position: no duplicate notify
        // drag while Idle is a no-op
        let mut s = Selection::Idle;
        assert!(!s.drag_to(pt(9., 5.)));
    }
}
