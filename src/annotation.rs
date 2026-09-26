//! Geometry annotations in desktop logical coordinates, shared by all outputs.
use gpui_kit::{Bounds, Path, PathBuilder, Pixels, Point, point, px, size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShapeKind {
    Rectangle,
    Ellipse,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Shape {
    pub(crate) kind: ShapeKind,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) color: u32,
    pub(crate) width: f32,
}

impl Shape {
    /// Match export's inward ellipse ring; reverse the inner contour to cut a hole.
    pub(crate) fn ellipse_path(self, offset: Point<Pixels>) -> Option<Path<Pixels>> {
        let rx = f32::from(self.bounds.size.width) / 2.;
        let ry = f32::from(self.bounds.size.height) / 2.;
        if rx <= 0. || ry <= 0. {
            return None;
        }
        let center = self.bounds.origin + offset + point(px(rx), px(ry));
        let mut path = PathBuilder::fill();
        let mut contour = |rx: f32, ry: f32, direction: f32| {
            let step = direction * std::f32::consts::TAU / 8.;
            let k = 4. / 3. * (step / 4.).tan();
            let at = |x, y| center + point(px(rx * x), px(ry * y));
            path.move_to(at(1., 0.));
            for i in 0..8 {
                let (s0, c0) = (i as f32 * step).sin_cos();
                let (s1, c1) = ((i + 1) as f32 * step).sin_cos();
                path.cubic_bezier_to(
                    at(c1, s1),
                    at(c0 - k * s0, s0 + k * c0),
                    at(c1 + k * s1, s1 - k * c1),
                );
            }
            path.close();
        };
        contour(rx, ry, 1.);
        if rx > self.width && ry > self.width {
            contour(rx - self.width, ry - self.width, -1.);
        }
        path.build().ok()
    }

    fn rasterize_ellipse(self, rgba: &mut [u8], w: u32, h: u32, origin: Point<Pixels>, scale: f32) {
        let rx = f32::from(self.bounds.size.width) * scale / 2.;
        let ry = f32::from(self.bounds.size.height) * scale / 2.;
        if rx <= 0. || ry <= 0. {
            return;
        }
        let cx = f32::from(self.bounds.left() - origin.x) * scale + rx;
        let cy = f32::from(self.bounds.top() - origin.y) * scale + ry;
        let inner_rx = rx - self.width * scale;
        let inner_ry = ry - self.width * scale;
        let left = (cx - rx).floor().clamp(0., w as f32) as usize;
        let right = (cx + rx).ceil().clamp(0., w as f32) as usize;
        let top = (cy - ry).floor().clamp(0., h as f32) as usize;
        let bottom = (cy + ry).ceil().clamp(0., h as f32) as usize;
        let color = self.color.to_be_bytes();
        let mut coverage = vec![0_f32; right - left];
        for row in top..bottom {
            coverage.fill(0.);
            // Analytic horizontal coverage and eight vertical samples smooth the edge
            // without testing every subpixel of the entire bounding rectangle.
            for sample in 0..8 {
                let dy = row as f32 + (sample as f32 + 0.5) / 8. - cy;
                if dy.abs() >= ry {
                    continue;
                }
                let outer = rx * (1. - (dy / ry).powi(2)).sqrt();
                let inner = if inner_rx > 0. && inner_ry > 0. && dy.abs() < inner_ry {
                    inner_rx * (1. - (dy / inner_ry).powi(2)).sqrt()
                } else {
                    0.
                };
                for (start, end) in [(cx - outer, cx - inner), (cx + inner, cx + outer)] {
                    let start_col = (start.floor().max(left as f32) as usize).min(right);
                    let end_col = (end.ceil().max(left as f32) as usize).min(right);
                    for col in start_col..end_col {
                        coverage[col - left] +=
                            (end.min(col as f32 + 1.) - start.max(col as f32)).max(0.) / 8.;
                    }
                }
            }
            for (index, coverage) in coverage.iter().copied().enumerate() {
                let offset = (row * w as usize + left + index) * 4;
                if rgba[offset + 3] == 0 || coverage == 0. {
                    continue;
                }
                let coverage = coverage.min(1.);
                for channel in 0..3 {
                    rgba[offset + channel] = (rgba[offset + channel] as f32 * (1. - coverage)
                        + color[channel] as f32 * coverage)
                        .round() as u8;
                }
            }
        }
    }

    /// Inward strokes keep both preview and export within the rectangle.
    pub(crate) fn strokes(self) -> [Bounds<Pixels>; 4] {
        let b = self.bounds;
        let width = px(self.width)
            .min(b.size.width / 2.)
            .min(b.size.height / 2.);
        [
            Bounds::new(b.origin, size(b.size.width, width)),
            Bounds::new(
                point(b.left(), b.bottom() - width),
                size(b.size.width, width),
            ),
            Bounds::new(
                point(b.left(), b.top() + width),
                size(width, b.size.height - width * 2.),
            ),
            Bounds::new(
                point(b.right() - width, b.top() + width),
                size(width, b.size.height - width * 2.),
            ),
        ]
    }
}

#[derive(Clone, Copy)]
struct Draft {
    start: Point<Pixels>,
    shape: Shape,
}

pub(crate) struct Annotations {
    tool: Option<ShapeKind>,
    color_ix: usize,
    width_ix: usize,
    shapes: Vec<Shape>,
    undone: Vec<Shape>,
    draft: Option<Draft>,
}

impl Default for Annotations {
    fn default() -> Self {
        Self {
            tool: None,
            color_ix: 0,
            width_ix: 1,
            shapes: Vec::new(),
            undone: Vec::new(),
            draft: None,
        }
    }
}

impl Annotations {
    pub(crate) fn enabled(&self) -> bool {
        self.tool.is_some()
    }
    pub(crate) fn color(&self) -> (u32, &'static str) {
        crate::theme::ANNOTATION_COLORS[self.color_ix]
    }
    pub(crate) fn width(&self) -> f32 {
        [1., 3., 5.][self.width_ix]
    }
    pub(crate) fn tool(&self) -> Option<ShapeKind> {
        self.tool
    }
    pub(crate) fn toggle(&mut self, kind: ShapeKind) {
        self.draft = None;
        self.tool = if self.tool == Some(kind) {
            None
        } else {
            Some(kind)
        };
    }
    pub(crate) fn set_color(&mut self, ix: usize) {
        if ix < crate::theme::ANNOTATION_COLORS.len() {
            self.color_ix = ix;
        }
    }
    pub(crate) fn set_width(&mut self, ix: usize) {
        if ix < 3 {
            self.width_ix = ix;
        }
    }
    pub(crate) fn reset(&mut self) {
        self.shapes.clear();
        self.undone.clear();
        self.draft = None;
        self.tool = None;
    }

    pub(crate) fn begin(&mut self, p: Point<Pixels>, selection: Bounds<Pixels>) {
        if !self.enabled() || !selection.contains(&p) {
            return;
        }
        self.draft = Some(Draft {
            start: p,
            shape: Shape {
                kind: self.tool.expect("active annotation tool"),
                bounds: Bounds::new(p, size(px(0.), px(0.))),
                color: self.color().0,
                width: self.width(),
            },
        });
    }

    pub(crate) fn drag_to(
        &mut self,
        p: Point<Pixels>,
        selection: Bounds<Pixels>,
        square: bool,
    ) -> bool {
        let Some(draft) = self.draft.as_mut() else {
            return false;
        };
        let start = draft.start;
        let mut end = point(
            p.x.clamp(selection.left(), selection.right()),
            p.y.clamp(selection.top(), selection.bottom()),
        );
        if square {
            let dx = f32::from(end.x - start.x);
            let dy = f32::from(end.y - start.y);
            let available_x = if dx < 0. {
                start.x - selection.left()
            } else {
                selection.right() - start.x
            };
            let available_y = if dy < 0. {
                start.y - selection.top()
            } else {
                selection.bottom() - start.y
            };
            let side = dx
                .abs()
                .max(dy.abs())
                .min(f32::from(available_x))
                .min(f32::from(available_y));
            end = point(
                start.x + px(if dx < 0. { -side } else { side }),
                start.y + px(if dy < 0. { -side } else { side }),
            );
        }
        let bounds = Bounds::from_corners(
            point(start.x.min(end.x), start.y.min(end.y)),
            point(start.x.max(end.x), start.y.max(end.y)),
        );
        let changed = draft.shape.bounds != bounds;
        draft.shape.bounds = bounds;
        changed
    }

    pub(crate) fn end(&mut self) {
        if let Some(draft) = self.draft.take()
            && draft.shape.bounds.size.width >= px(2.)
            && draft.shape.bounds.size.height >= px(2.)
        {
            self.shapes.push(draft.shape);
            self.undone.clear();
        }
    }

    /// Escape cancels a stroke first, then leaves the tool while keeping marks.
    pub(crate) fn cancel(&mut self) -> bool {
        if self.draft.take().is_some() {
            return true;
        }
        if self.enabled() {
            self.tool = None;
            return true;
        }
        false
    }
    pub(crate) fn undo(&mut self) {
        if self.draft.take().is_some() {
            return;
        }
        if let Some(shape) = self.shapes.pop() {
            self.undone.push(shape);
        }
    }
    pub(crate) fn redo(&mut self) {
        if self.draft.is_none()
            && let Some(shape) = self.undone.pop()
        {
            self.shapes.push(shape);
        }
    }
    pub(crate) fn visible(&self) -> impl Iterator<Item = Shape> + '_ {
        self.shapes
            .iter()
            .copied()
            .chain(self.draft.map(|draft| draft.shape))
    }

    pub(crate) fn rasterize(
        &self,
        rgba: &mut [u8],
        w: u32,
        h: u32,
        origin: Point<Pixels>,
        scale: f32,
    ) {
        for shape in self.visible() {
            if shape.kind == ShapeKind::Ellipse {
                shape.rasterize_ellipse(rgba, w, h, origin, scale);
                continue;
            }
            let color = shape.color.to_be_bytes();
            for stroke in shape.strokes() {
                let x = |v: Pixels| {
                    (f32::from(v - origin.x) * scale)
                        .round()
                        .clamp(0., w as f32) as usize
                };
                let y = |v: Pixels| {
                    (f32::from(v - origin.y) * scale)
                        .round()
                        .clamp(0., h as f32) as usize
                };
                for row in y(stroke.top())..y(stroke.bottom()) {
                    for col in x(stroke.left())..x(stroke.right()) {
                        let offset = (row * w as usize + col) * 4;
                        // Preserve transparent gaps between desktop outputs.
                        if rgba[offset + 3] != 0 {
                            rgba[offset..offset + 4].copy_from_slice(&color);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Annotations;
    use gpui_kit::{Bounds, point, px, size};

    fn selection() -> Bounds<gpui_kit::Pixels> {
        Bounds::new(point(px(-20.), px(0.)), size(px(100.), px(100.)))
    }
    fn rectangle(a: &mut Annotations) {
        a.begin(point(px(10.), px(10.)), selection());
        a.drag_to(point(px(30.), px(40.)), selection(), false);
        a.end();
    }

    #[test]
    fn rectangle_history_preserves_style_and_new_strokes_clear_redo() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Rectangle);
        rectangle(&mut a);
        let first_color = a.visible().next().unwrap().color;
        a.set_color(1);
        a.set_width(2);
        rectangle(&mut a);
        assert_eq!(a.visible().count(), 2);
        a.undo();
        assert!(!a.undone.is_empty());
        assert_eq!(a.visible().next().unwrap().color, first_color);
        a.redo();
        assert_eq!(a.visible().count(), 2);
        a.undo();
        rectangle(&mut a);
        assert!(a.undone.is_empty());
        a.reset();
        assert_eq!(a.visible().count(), 0);
        assert!(!a.enabled());
    }

    #[test]
    fn square_stays_square_at_boundary_and_reverse_drag_normalizes() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Rectangle);
        a.begin(point(px(20.), px(20.)), selection());
        a.drag_to(point(px(-100.), px(-200.)), selection(), true);
        let b = a.visible().next().unwrap().bounds;
        assert_eq!(b.origin, point(px(0.), px(0.)));
        assert_eq!(b.size, size(px(20.), px(20.)));
        a.end();
        assert_eq!(a.visible().count(), 1);
    }

    #[test]
    fn outside_click_and_tiny_stroke_do_not_destroy_redo() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Rectangle);
        rectangle(&mut a);
        a.undo();
        a.begin(point(px(200.), px(20.)), selection());
        a.end();
        a.begin(point(px(10.), px(10.)), selection());
        a.drag_to(point(px(11.), px(11.)), selection(), false);
        a.end();
        assert!(!a.undone.is_empty());
        assert_eq!(a.visible().count(), 0);
    }

    #[test]
    fn escape_cancels_draft_then_exits_tool_without_losing_marks() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Rectangle);
        rectangle(&mut a);
        a.begin(point(px(10.), px(10.)), selection());
        assert!(a.cancel());
        assert!(a.enabled());
        assert_eq!(a.visible().count(), 1);
        assert!(a.cancel());
        assert!(!a.enabled());
        assert_eq!(a.visible().count(), 1);
        assert!(!a.cancel());
    }

    #[test]
    fn raster_strokes_preserve_interior_and_transparent_gaps() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Rectangle);
        rectangle(&mut a);
        let mut pixels = [10, 20, 30, 255].repeat(100 * 100);
        let gap = (10 * 100 + 15) * 4;
        pixels[gap..gap + 4].fill(0);
        a.rasterize(&mut pixels, 100, 100, point(px(0.), px(0.)), 1.);
        assert_eq!(
            &pixels[(10 * 100 + 10) * 4..(10 * 100 + 10) * 4 + 4],
            &a.color().0.to_be_bytes()
        );
        assert_eq!(
            &pixels[(20 * 100 + 20) * 4..(20 * 100 + 20) * 4 + 4],
            &[10, 20, 30, 255]
        );
        assert_eq!(&pixels[gap..gap + 4], &[0; 4]);
    }
    #[test]
    fn mixed_shapes_share_history_and_switching_cancels_only_the_draft() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Rectangle);
        rectangle(&mut a);
        a.begin(point(px(10.), px(10.)), selection());
        a.toggle(super::ShapeKind::Ellipse);
        assert_eq!(a.visible().count(), 1);
        a.set_color(3);
        rectangle(&mut a);
        a.undo();
        assert_eq!(
            a.visible().next().unwrap().kind,
            super::ShapeKind::Rectangle
        );
        a.redo();
        let ellipse = a.visible().last().unwrap();
        assert_eq!(ellipse.kind, super::ShapeKind::Ellipse);
        assert_eq!(ellipse.color, a.color().0);
        a.toggle(super::ShapeKind::Ellipse);
        assert!(!a.enabled());
        assert_eq!(a.visible().count(), 2);
    }

    #[test]
    fn shift_ellipse_is_a_circle_even_at_selection_boundary() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Ellipse);
        a.begin(point(px(20.), px(20.)), selection());
        a.drag_to(point(px(-100.), px(-200.)), selection(), true);
        a.end();
        let ellipse = a.visible().next().unwrap();
        assert_eq!(ellipse.kind, super::ShapeKind::Ellipse);
        assert_eq!(ellipse.bounds.origin, point(px(0.), px(0.)));
        assert_eq!(ellipse.bounds.size, size(px(20.), px(20.)));
    }

    #[test]
    fn ellipse_raster_has_smooth_edges_and_preserves_hole_corners_and_gaps() {
        for scale in [1., 1.25, 1.5, 1.73, 2.] {
            let ellipse = super::Shape {
                kind: super::ShapeKind::Ellipse,
                bounds: Bounds::new(point(px(-10.), px(15.)), size(px(60.), px(40.))),
                color: 0xff0000ff,
                width: 3.,
            };
            let w = (100. * scale) as u32;
            let mut pixels = [0, 0, 0, 255].repeat((w * w) as usize);
            let gap_x = (40. * scale) as usize;
            let gap_y = (16. * scale) as usize;
            let gap = (gap_y * w as usize + gap_x) * 4;
            pixels[gap..gap + 4].fill(0);
            ellipse.rasterize_ellipse(&mut pixels, w, w, point(px(-20.), px(0.)), scale);
            let pixel = |x: f32, y: f32| {
                let offset = (((y * scale) as usize) * w as usize + (x * scale) as usize) * 4;
                &pixels[offset..offset + 4]
            };
            assert_eq!(pixel(40., 35.), &[0, 0, 0, 255], "center at {scale}");
            assert_eq!(pixel(11., 16.), &[0, 0, 0, 255], "corner at {scale}");
            assert_eq!(pixel(11., 35.), &[255, 0, 0, 255], "left edge at {scale}");
            assert_eq!(pixel(68., 35.), &[255, 0, 0, 255], "right edge at {scale}");
            assert_eq!(&pixels[gap..gap + 4], &[0; 4]);
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] > 0 && p[0] < 255)
            );
            assert!(ellipse.ellipse_path(point(px(0.), px(0.))).is_some());
        }
    }

    #[test]
    fn tiny_and_clipped_ellipses_render_without_invalid_geometry() {
        for (width, height) in [(0., 0.), (2., 2.), (2., 60.), (60., 2.), (60., 40.)] {
            let ellipse = super::Shape {
                kind: super::ShapeKind::Ellipse,
                bounds: Bounds::new(point(px(-10.), px(-10.)), size(px(width), px(height))),
                color: 0xff0000ff,
                width: 5.,
            };
            let mut pixels = [0, 0, 0, 255].repeat(400);
            ellipse.rasterize_ellipse(&mut pixels, 20, 20, point(px(0.), px(0.)), 1.25);
            assert_eq!(
                ellipse.ellipse_path(point(px(0.), px(0.))).is_some(),
                width > 0.
            );
        }
    }
}
