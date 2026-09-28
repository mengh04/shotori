//! Selecting placed shapes: hit-testing and the selection state.
//!
//! The hit region of every kind is its visible stroke or region —
//! "what you see is what you can click" (issue #5). This module owns
//! the geometry probes and the `selected` slot on [`Annotations`];
//! the pointer-level state machine (click vs. draw-through) lives in
//! the session, which drives these APIs.
use super::{Annotations, HistoryEntry, Shape, ShapeKind, line};
use gpui_kit::{Bounds, Pixels, Point, point, px, size};

/// Pointing forgiveness for hairline geometry — the hit region is the
/// visible stroke itself; this only covers the antialiased fringe so
/// an edge-pointing click still lands.
const HIT_TOLERANCE: f32 = 0.5;

impl Annotations {
    /// Hit probe without side effects: the topmost shape index under
    /// the point, if any.
    pub(crate) fn hit_test(&self, p: Point<Pixels>) -> Option<usize> {
        self.shapes.iter().rposition(|s| shape_hit(s, p))
    }

    /// Whether the pointer currently sits on a selectable shape — the
    /// hover probe for the pointer affordance.
    pub(crate) fn hits_shape(&self, p: Point<Pixels>) -> bool {
        self.shapes.iter().rev().any(|s| shape_hit(s, p))
    }

    /// Select a shape by index (the click-select path); no-op when the
    /// index no longer exists.
    pub(crate) fn select_index(&mut self, ix: usize) -> bool {
        if self.shapes.get(ix).is_some() {
            self.selected = Some(ix);
            true
        } else {
            false
        }
    }

    pub(crate) fn deselect(&mut self) {
        self.selected = None;
    }

    /// Remove the selected shape (the Delete/Backspace path). Records
    /// a Remove entry so undo re-inserts at the same index; drops the
    /// selection because indices shift after a mid-sequence removal.
    pub(crate) fn delete_selected(&mut self) -> bool {
        let Some(ix) = self.selected_index() else {
            return false;
        };
        if ix >= self.shapes.len() {
            return false;
        }
        let shape = self.shapes.remove(ix);
        self.history.push(HistoryEntry::Remove { ix, shape });
        self.redo.clear();
        self.selected = None;
        true
    }

    pub(crate) fn selected(&self) -> Option<&Shape> {
        self.selected.and_then(|ix| self.shapes.get(ix))
    }

    /// The selected shape's index (valid or None).
    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.selected.filter(|ix| self.shapes.get(*ix).is_some())
    }

    /// Re-place shape `ix` as the press-time snapshot translated by
    /// `delta`. Snapshot re-derivation means move events cannot
    /// accumulate float error, and a zero delta restores the snapshot
    /// (the Escape path).
    pub(crate) fn place_shape(&mut self, ix: usize, before: &Shape, delta: Point<Pixels>) {
        if let Some(shape) = self.shapes.get_mut(ix) {
            shape.bounds = Bounds::new(before.bounds.origin + delta, before.bounds.size);
            shape.points = before.points.iter().map(|p| *p + delta).collect();
        }
    }

    /// Re-derive shape `ix` from the press snapshot with handle
    /// `anchor` placed at `p` — snapshot semantics like [`place_shape`].
    pub(crate) fn place_handle(
        &mut self,
        ix: usize,
        anchor: usize,
        before: &Shape,
        p: Point<Pixels>,
    ) {
        if let Some(shape) = self.shapes.get_mut(ix) {
            *shape = before.clone();
            shape.set_handle(anchor, p);
        }
    }

    /// Finish an in-place edit drag (move or handle): record the Edit
    /// (before → current) so undo restores the pre-drag state. No
    /// entry when nothing changed.
    pub(crate) fn commit_move(&mut self, ix: usize, before: Shape) {
        let Some(after) = self.shapes.get(ix) else {
            return;
        };
        if after != &before {
            self.history.push(HistoryEntry::Edit {
                ix,
                before,
                after: after.clone(),
            });
            self.redo.clear();
        }
    }

    /// Whether a press right now should park a click-select pending
    /// resolution (release = select, drag past the slop = draw
    /// through). Polyline never parks: its clicks PLACE VERTICES, and
    /// an intercepting hit would break polygons mid-drawing — its
    /// shapes stay selectable from any other tool.
    pub(crate) fn parks_click_select(&self) -> bool {
        self.enabled() && self.tool != Some(ShapeKind::Polyline)
    }

    /// Step the selected shape's size through its OWN preset ladder —
    /// the same rungs the toolbar exposes per tool. Number badges grow
    /// their diameter (bounds re-centered); every other kind steps its
    /// width field. One reversible Edit entry per notch.
    pub(crate) fn step_selected_size(&mut self, ix: usize, up: bool) -> bool {
        let kind = self.shapes[ix].kind;
        let spec = super::size_spec(kind);
        let current = if kind == ShapeKind::Number {
            f32::from(self.shapes[ix].bounds.size.width)
        } else {
            self.shapes[ix].width
        };
        let next = if up { current + 1. } else { current - 1. };
        if next == current || next < spec.min || next > spec.max {
            return false;
        }
        let before = self.shapes[ix].clone();
        if kind == ShapeKind::Number {
            // grow the badge around its center
            let b = self.shapes[ix].bounds;
            let c = point(b.left() + b.size.width / 2., b.top() + b.size.height / 2.);
            self.shapes[ix].bounds = Bounds::new(
                point(c.x - px(next / 2.), c.y - px(next / 2.)),
                size(px(next), px(next)),
            );
        } else {
            self.shapes[ix].width = next;
        }
        let after = self.shapes[ix].clone();
        self.history.push(HistoryEntry::Edit { ix, before, after });
        self.redo.clear();
        true
    }
}

/// Whether a point lands on a selectable shape. Every kind's region is
/// its visible stroke or body; see [`line::geometry`] for the shared
/// visual outline.
fn shape_hit(shape: &Shape, p: Point<Pixels>) -> bool {
    let (x, y) = (f32::from(p.x), f32::from(p.y));
    match shape.kind {
        // stroke band: any of the four edge rectangles, inflated by the
        // antialiased fringe
        ShapeKind::Rectangle => shape
            .strokes()
            .iter()
            .any(|s| inflate(s, px(HIT_TOLERANCE)).contains(&p)),
        ShapeKind::Ellipse => {
            let rx = f32::from(shape.bounds.size.width) / 2.;
            let ry = f32::from(shape.bounds.size.height) / 2.;
            if rx <= 0. || ry <= 0. {
                return false;
            }
            let cx = f32::from(shape.bounds.origin.x) + rx;
            let cy = f32::from(shape.bounds.origin.y) + ry;
            let outer = ((x - cx) / (rx + HIT_TOLERANCE)).powi(2)
                + ((y - cy) / (ry + HIT_TOLERANCE)).powi(2);
            let inner_rx = (rx - shape.width - HIT_TOLERANCE).max(0.);
            let inner_ry = (ry - shape.width - HIT_TOLERANCE).max(0.);
            // a band thinner than the tolerance means even the center
            // is within reach — the whole disc hits
            let inner_clear = inner_rx <= 0.
                || inner_ry <= 0.
                || ((x - cx) / inner_rx).powi(2) + ((y - cy) / inner_ry).powi(2) >= 1.;
            outer <= 1. && inner_clear
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            // the exact visual geometry (capsule / arrowhead polygon):
            // what you see is what you can click
            line::geometry(&shape.points, shape.width, shape.kind == ShapeKind::Arrow)
                .iter()
                .any(|poly| point_in_polygon(p, poly))
        }
        // freehand families share the same visual-polygon outline
        ShapeKind::Polyline | ShapeKind::Pencil | ShapeKind::Highlighter => {
            line::geometry(&shape.points, shape.width, false)
                .iter()
                .any(|poly| point_in_polygon(p, poly))
        }
        // the badge is a circle inscribed in its bounds
        ShapeKind::Number => {
            let r = f32::from(shape.bounds.size.width) / 2.;
            if r <= 0. {
                return false;
            }
            let c = shape.bounds.origin + point(px(r), px(r));
            (f32::from(p.x - c.x)).hypot(f32::from(p.y - c.y)) <= r + HIT_TOLERANCE
        }
        // solid regions: anywhere inside the bounds
        ShapeKind::Text | ShapeKind::Mosaic | ShapeKind::Blur => {
            inflate(&shape.bounds, px(HIT_TOLERANCE)).contains(&p)
        }
        _ => false,
    }
}

/// Even-odd ray casting: is the point inside the polygon?
fn point_in_polygon(p: Point<Pixels>, poly: &[Point<Pixels>]) -> bool {
    let (x, y) = (f32::from(p.x), f32::from(p.y));
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = (f32::from(poly[i].x), f32::from(poly[i].y));
        let (xj, yj) = (f32::from(poly[j].x), f32::from(poly[j].y));
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn inflate(b: &Bounds<Pixels>, by: Pixels) -> Bounds<Pixels> {
    Bounds::new(
        point(b.origin.x - by, b.origin.y - by),
        size(b.size.width + by * 2., b.size.height + by * 2.),
    )
}
