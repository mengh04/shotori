//! Selection chrome geometry: the highlight outline and the handle
//! anchors a selected shape paints. Kept next to the shape data (not
//! in the overlay) so both preview paths render identical chrome and
//! future geometry editing (issue #5 phase C) grows on the same
//! anchors. Pure geometry — no state.
use gpui_kit::{Bounds, Path, PathBuilder, Pixels, Point, point, px};

use super::{Shape, ShapeKind};

impl Shape {
    /// Selection highlight: a thin stroke tracing the shape's own
    /// visual outline — capsule rim, ellipse ring, arrowhead, badge
    /// circle; never a filled blob over the content. Corner handles
    /// come from [`Shape::handle_points`] on the overlay side.
    pub(crate) fn hilite_paths(&self, offset: Point<Pixels>) -> Vec<Path<Pixels>> {
        match self.kind {
            ShapeKind::Line
            | ShapeKind::Arrow
            | ShapeKind::Polyline
            | ShapeKind::Pencil
            | ShapeKind::Highlighter => {
                super::line::geometry(&self.points, self.width, self.kind == ShapeKind::Arrow)
                    .iter()
                    .filter_map(|poly| {
                        let pts: Vec<Point<Pixels>> = poly.iter().map(|p| *p + offset).collect();
                        let mut builder = PathBuilder::stroke(px(1.));
                        builder.add_polygon(&pts, true);
                        builder.build().ok()
                    })
                    .collect()
            }
            ShapeKind::Rectangle => self
                .strokes()
                .iter()
                .filter_map(|s| rect_stroke(*s, offset))
                .collect(),
            ShapeKind::Ellipse => ellipse_stroke(self.bounds, offset).into_iter().collect(),
            ShapeKind::Number => ellipse_stroke(self.bounds, offset).into_iter().collect(),
            // solid regions trace their bounds rectangle
            ShapeKind::Text | ShapeKind::Mosaic | ShapeKind::Blur => {
                rect_stroke(self.bounds, offset).into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    /// Handle anchors for the selection chrome — the points where a
    /// grab makes sense for geometry editing: the two endpoints of a
    /// line/arrow, every vertex of a polyline, the four corners of
    /// rectangles/ellipses. Freehand strokes and content shapes
    /// (badges, text, filters) have no per-point editing semantics and
    /// get the outline alone.
    pub(crate) fn handle_points(&self) -> Vec<Point<Pixels>> {
        match self.kind {
            ShapeKind::Line | ShapeKind::Arrow => self.points.iter().take(2).cloned().collect(),
            ShapeKind::Polyline => self.points.clone(),
            ShapeKind::Rectangle | ShapeKind::Ellipse => vec![
                point(self.bounds.left(), self.bounds.top()),
                point(self.bounds.right(), self.bounds.top()),
                point(self.bounds.right(), self.bounds.bottom()),
                point(self.bounds.left(), self.bounds.bottom()),
            ],
            _ => Vec::new(),
        }
    }
}

/// A 1 px stroke tracing a rectangle's perimeter.
fn rect_stroke(b: Bounds<gpui_kit::Pixels>, offset: Point<Pixels>) -> Option<Path<Pixels>> {
    let mut builder = PathBuilder::stroke(px(1.));
    builder.add_polygon(
        &[
            point(b.left(), b.top()) + offset,
            point(b.right(), b.top()) + offset,
            point(b.right(), b.bottom()) + offset,
            point(b.left(), b.bottom()) + offset,
        ],
        true,
    );
    builder.build().ok()
}

/// Append one eight-arc cubic ellipse contour to a builder (either
/// fill or stroke mode). `direction` flips the winding to cut holes.
/// Shared by the export ring ([`Shape::ellipse_path`]) and the chrome
/// stroke so both use the identical ellipse approximation.
pub(super) fn ellipse_contour(
    builder: &mut PathBuilder,
    center: Point<Pixels>,
    rx: f32,
    ry: f32,
    direction: f32,
) {
    let step = direction * std::f32::consts::TAU / 8.;
    let k = 4. / 3. * (step / 4.).tan();
    let at = |x, y| center + point(px(rx * x), px(ry * y));
    builder.move_to(at(1., 0.));
    for i in 0..8 {
        let (s0, c0) = (i as f32 * step).sin_cos();
        let (s1, c1) = ((i + 1) as f32 * step).sin_cos();
        builder.cubic_bezier_to(
            at(c1, s1),
            at(c0 - k * s0, s0 + k * c0),
            at(c1 + k * s1, s1 - k * c1),
        );
    }
    builder.close();
}

/// A 1 px stroke tracing an ellipse's perimeter (the bounds box; a
/// circle is the square-bounds case).
fn ellipse_stroke(b: Bounds<gpui_kit::Pixels>, offset: Point<Pixels>) -> Option<Path<Pixels>> {
    let rx = f32::from(b.size.width) / 2.;
    let ry = f32::from(b.size.height) / 2.;
    if rx <= 0. || ry <= 0. {
        return None;
    }
    let mut builder = PathBuilder::stroke(px(1.));
    ellipse_contour(
        &mut builder,
        b.origin + offset + point(px(rx), px(ry)),
        rx,
        ry,
        1.,
    );
    builder.build().ok()
}
