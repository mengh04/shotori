//! Geometry annotations in desktop logical coordinates, shared by all outputs.
mod highlighter;
mod line;
pub(crate) use highlighter::HighlighterCache;
mod number;
pub(crate) use number::NumberCache;

use gpui_kit::{Bounds, Path, PathBuilder, Pixels, Point, point, px, size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShapeKind {
    Number,
    Pencil,
    Highlighter,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Polyline,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shape {
    pub(crate) kind: ShapeKind,
    pub(crate) number: Option<u32>,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) color: u32,
    pub(crate) width: f32,
    pub(crate) points: Vec<Point<Pixels>>,
}

impl Shape {
    pub(crate) fn line_paths(&self, offset: Point<Pixels>) -> Vec<Path<Pixels>> {
        line::paths(self, offset)
    }

    /// Match export's inward ellipse ring; reverse the inner contour to cut a hole.
    pub(crate) fn ellipse_path(&self, offset: Point<Pixels>) -> Option<Path<Pixels>> {
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

    fn rasterize_ellipse(
        &self,
        rgba: &mut [u8],
        w: u32,
        h: u32,
        origin: Point<Pixels>,
        scale: f32,
    ) {
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
    pub(crate) fn strokes(&self) -> [Bounds<Pixels>; 4] {
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

#[derive(Clone)]
struct Draft {
    start: Point<Pixels>,
    shape: Shape,
}

pub(crate) struct Annotations {
    tool: Option<ShapeKind>,
    color_ix: usize,
    width_ix: usize,
    number_size_ix: usize,
    highlighter_width_ix: usize,
    highlighter_color_ix: usize,
    shapes: Vec<Shape>,
    undone: Vec<Shape>,
    draft: Option<Draft>,
    pressed: bool,
}

impl Default for Annotations {
    fn default() -> Self {
        Self {
            tool: None,
            color_ix: 0,
            width_ix: 1,
            number_size_ix: 1,
            highlighter_width_ix: 1,
            highlighter_color_ix: 2,
            shapes: Vec::new(),
            undone: Vec::new(),
            draft: None,
            pressed: false,
        }
    }
}

impl Annotations {
    pub(crate) fn enabled(&self) -> bool {
        self.tool.is_some()
    }
    pub(crate) fn color(&self) -> (u32, &'static str) {
        crate::theme::ANNOTATION_COLORS[if self.tool == Some(ShapeKind::Highlighter) {
            self.highlighter_color_ix
        } else {
            self.color_ix
        }]
    }
    pub(crate) fn width(&self) -> f32 {
        if self.tool == Some(ShapeKind::Highlighter) {
            [12., 20., 32.][self.highlighter_width_ix]
        } else {
            [1., 3., 5.][self.width_ix]
        }
    }
    pub(crate) fn number_size(&self) -> f32 {
        [24., 32., 40.][self.number_size_ix]
    }
    pub(crate) fn set_number_size(&mut self, ix: usize) {
        if ix < 3 {
            self.number_size_ix = ix;
        }
    }
    pub(crate) fn next_number(&self) -> u32 {
        self.shapes
            .iter()
            .filter_map(|s| s.number)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }
    pub(crate) fn tool(&self) -> Option<ShapeKind> {
        self.tool
    }
    pub(crate) fn toggle(&mut self, kind: ShapeKind) {
        self.draft = None;
        self.pressed = false;
        self.tool = if self.tool == Some(kind) {
            None
        } else {
            Some(kind)
        };
    }
    pub(crate) fn set_color(&mut self, ix: usize) {
        if ix < crate::theme::ANNOTATION_COLORS.len() {
            if self.tool == Some(ShapeKind::Highlighter) {
                self.highlighter_color_ix = ix;
            } else {
                self.color_ix = ix;
            }
        }
    }
    pub(crate) fn set_width(&mut self, ix: usize) {
        if ix < 3 {
            if self.tool == Some(ShapeKind::Highlighter) {
                self.highlighter_width_ix = ix;
            } else {
                self.width_ix = ix;
            }
        }
    }
    pub(crate) fn reset(&mut self) {
        self.shapes.clear();
        self.undone.clear();
        self.draft = None;
        self.pressed = false;
        self.tool = None;
    }

    pub(crate) fn begin(&mut self, p: Point<Pixels>, selection: Bounds<Pixels>) {
        if !self.enabled() || !selection.contains(&p) {
            return;
        }
        if self.tool == Some(ShapeKind::Number)
            && (selection.size.width < px(16.) || selection.size.height < px(16.))
        {
            return;
        }
        self.pressed = true;
        if self.tool == Some(ShapeKind::Polyline) && self.draft.is_some() {
            return;
        }
        self.draft = Some(Draft {
            start: p,
            shape: Shape {
                kind: self.tool.expect("active annotation tool"),
                number: (self.tool == Some(ShapeKind::Number)).then(|| self.next_number()),
                bounds: if self.tool == Some(ShapeKind::Number) {
                    number_bounds(p, selection, self.number_size())
                } else {
                    Bounds::new(p, size(px(0.), px(0.)))
                },
                color: if self.tool == Some(ShapeKind::Highlighter) {
                    (self.color().0 & 0xffffff00) | 96
                } else {
                    self.color().0
                },
                width: self.width(),
                points: if matches!(
                    self.tool,
                    Some(ShapeKind::Line | ShapeKind::Arrow | ShapeKind::Polyline)
                ) {
                    vec![p, p]
                } else if matches!(self.tool, Some(ShapeKind::Pencil | ShapeKind::Highlighter)) {
                    vec![p]
                } else {
                    Vec::new()
                },
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
        if matches!(draft.shape.kind, ShapeKind::Pencil | ShapeKind::Highlighter) {
            let end = line_endpoint(draft.start, p, selection, false);
            if draft.shape.points.last() == Some(&end) {
                return false;
            }
            draft.shape.points.push(end);
            return true;
        }
        if draft.shape.kind == ShapeKind::Number {
            let bounds = number_bounds(p, selection, f32::from(draft.shape.bounds.size.width));
            let changed = bounds != draft.shape.bounds;
            draft.shape.bounds = bounds;
            return changed;
        }
        if matches!(
            draft.shape.kind,
            ShapeKind::Line | ShapeKind::Arrow | ShapeKind::Polyline
        ) {
            let last = draft.shape.points.len() - 1;
            let start = draft.shape.points[last - 1];
            let end = line_endpoint(start, p, selection, square);
            let changed = draft.shape.points[last] != end;
            draft.shape.points[last] = end;
            return changed;
        }
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
        if !std::mem::take(&mut self.pressed) {
            return;
        }
        if let Some(draft) = self.draft.as_mut()
            && draft.shape.kind == ShapeKind::Polyline
        {
            let last = draft.shape.points.len() - 1;
            let end = draft.shape.points[last];
            if distance(draft.shape.points[last - 1], end) >= 2. {
                draft.shape.points.push(end);
            } else {
                draft.shape.points[last] = draft.shape.points[last - 1];
            }
            return;
        }
        if let Some(draft) = self.draft.take() {
            let valid = if matches!(draft.shape.kind, ShapeKind::Line | ShapeKind::Arrow) {
                distance(draft.shape.points[0], draft.shape.points[1]) >= 2.
            } else if matches!(draft.shape.kind, ShapeKind::Pencil | ShapeKind::Highlighter) {
                true
            } else {
                draft.shape.bounds.size.width >= px(2.) && draft.shape.bounds.size.height >= px(2.)
            };
            if valid {
                self.shapes.push(draft.shape);
                self.undone.clear();
            }
        }
    }

    pub(crate) fn is_pressed(&self) -> bool {
        self.pressed
    }

    pub(crate) fn is_drawing_polyline(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|draft| draft.shape.kind == ShapeKind::Polyline)
    }

    /// Commit confirmed vertices, never the floating cursor preview.
    pub(crate) fn finish_polyline(&mut self) {
        if !self.is_drawing_polyline() {
            return;
        }
        self.pressed = false;
        let mut shape = self.draft.take().unwrap().shape;
        shape.points.pop();
        if shape.points.len() >= 2 {
            self.shapes.push(shape);
            self.undone.clear();
        }
    }

    /// Escape cancels a stroke first, then leaves the tool while keeping marks.
    pub(crate) fn cancel(&mut self) -> bool {
        self.pressed = false;
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
        self.pressed = false;
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
    pub(crate) fn visible(&self) -> impl Iterator<Item = &Shape> + '_ {
        self.shapes
            .iter()
            .chain(self.draft.as_ref().map(|draft| &draft.shape))
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
            if shape.kind == ShapeKind::Number {
                number::rasterize(shape, rgba, w, h, origin, scale);
                continue;
            }
            if matches!(
                shape.kind,
                ShapeKind::Line
                    | ShapeKind::Arrow
                    | ShapeKind::Polyline
                    | ShapeKind::Pencil
                    | ShapeKind::Highlighter
            ) {
                line::rasterize(shape, rgba, w, h, origin, scale);
                continue;
            }
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

fn number_bounds(p: Point<Pixels>, selection: Bounds<Pixels>, diameter: f32) -> Bounds<Pixels> {
    let diameter = px(diameter)
        .min(selection.size.width)
        .min(selection.size.height);
    let radius = diameter / 2.;
    let center = point(
        p.x.clamp(selection.left() + radius, selection.right() - radius),
        p.y.clamp(selection.top() + radius, selection.bottom() - radius),
    );
    Bounds::new(center - point(radius, radius), size(diameter, diameter))
}

fn distance(a: Point<Pixels>, b: Point<Pixels>) -> f32 {
    f32::from(b.x - a.x).hypot(f32::from(b.y - a.y))
}

fn line_endpoint(
    start: Point<Pixels>,
    p: Point<Pixels>,
    bounds: Bounds<Pixels>,
    constrain: bool,
) -> Point<Pixels> {
    let end = point(
        p.x.clamp(bounds.left(), bounds.right()),
        p.y.clamp(bounds.top(), bounds.bottom()),
    );
    if !constrain {
        return end;
    }
    let dx = f32::from(end.x - start.x);
    let dy = f32::from(end.y - start.y);
    let direction =
        ((dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8) as usize;
    let (x, y): (f32, f32) = [
        (1., 0.),
        (1., 1.),
        (0., 1.),
        (-1., 1.),
        (-1., 0.),
        (-1., -1.),
        (0., -1.),
        (1., -1.),
    ][direction];
    let mut length = dx.hypot(dy) / x.hypot(y);
    for (direction, available) in [
        (
            x,
            if x < 0. {
                start.x - bounds.left()
            } else {
                bounds.right() - start.x
            },
        ),
        (
            y,
            if y < 0. {
                start.y - bounds.top()
            } else {
                bounds.bottom() - start.y
            },
        ),
    ] {
        if direction != 0. {
            length = length.min(f32::from(available));
        }
    }
    point(
        (start.x + px(x * length)).clamp(bounds.left(), bounds.right()),
        (start.y + px(y * length)).clamp(bounds.top(), bounds.bottom()),
    )
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
    fn highlighter_has_independent_style_and_whole_stroke_history() {
        let mut a = Annotations::default();
        let original_color = a.color();
        a.toggle(super::ShapeKind::Highlighter);
        assert_eq!(a.color().1, "Yellow");
        assert_eq!(a.width(), 20.);
        a.set_color(4);
        a.set_width(2);
        a.begin(point(px(10.), px(30.)), selection());
        a.drag_to(point(px(60.), px(30.)), selection(), false);
        a.end();
        let mark = a.visible().next().unwrap().clone();
        assert_eq!(mark.color & 255, 96);
        assert_eq!(mark.width, 32.);
        a.toggle(super::ShapeKind::Pencil);
        assert_eq!(a.color(), original_color);
        assert_eq!(a.width(), 3.);
        a.undo();
        assert_eq!(a.visible().count(), 0);
        a.redo();
        assert_eq!(a.visible().next().unwrap(), &mark);
        a.toggle(super::ShapeKind::Highlighter);
        assert_eq!(a.width(), 32.);
        assert_eq!(a.color().1, "Blue");
    }

    #[test]
    fn pencil_records_curve_and_release_as_one_history_entry() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Pencil);
        let points =
            [(0., 10.), (15., 25.), (30., 10.), (40., 35.)].map(|(x, y)| point(px(x), px(y)));
        a.begin(points[0], selection());
        for p in &points[1..] {
            assert!(a.drag_to(*p, selection(), false));
            assert!(!a.drag_to(*p, selection(), false));
        }
        a.end();
        assert!(!a.drag_to(point(px(70.), px(70.)), selection(), false));
        assert_eq!(a.visible().next().unwrap().points, points);
        a.undo();
        assert_eq!(a.visible().count(), 0);
        a.redo();
        assert_eq!(a.visible().next().unwrap().points, points);
        a.begin(points[0], selection());
        a.drag_to(point(px(200.), px(-40.)), selection(), false);
        assert_eq!(
            *a.visible().last().unwrap().points.last().unwrap(),
            point(px(80.), px(0.))
        );
        a.cancel();
        assert_eq!(a.visible().count(), 1);
    }

    #[test]
    fn pencil_click_exports_round_dot_at_fractional_scales() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Pencil);
        a.set_width(2);
        a.begin(point(px(20.), px(20.)), selection());
        a.end();
        assert_eq!(a.visible().count(), 1);
        assert_eq!(
            a.visible()
                .next()
                .unwrap()
                .line_paths(point(px(0.), px(0.)))
                .len(),
            1
        );
        for scale in [1., 1.25, 1.7, 2.] {
            let mut pixels = vec![255; 100 * 100 * 4];
            let center = (20. * scale) as usize;
            // Transparent gaps between screens must remain transparent.
            pixels[(center * 100 + center + 1) * 4 + 3] = 0;
            a.rasterize(&mut pixels, 100, 100, point(px(0.), px(0.)), scale);
            let at = |x, y| &pixels[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
            assert_eq!(at(center, center), a.color().0.to_be_bytes());
            assert_eq!(at(center + 1, center)[3], 0);
            assert_eq!(at(center + 5, center + 5), [255; 4]);
        }
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
                number: None,
                kind: super::ShapeKind::Ellipse,
                bounds: Bounds::new(point(px(-10.), px(15.)), size(px(60.), px(40.))),
                color: 0xff0000ff,
                points: Vec::new(),
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
                number: None,
                kind: super::ShapeKind::Ellipse,
                bounds: Bounds::new(point(px(-10.), px(-10.)), size(px(width), px(height))),
                color: 0xff0000ff,
                points: Vec::new(),
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
    #[test]
    fn horizontal_vertical_and_reverse_lines_are_valid_but_tiny_clicks_preserve_redo() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Line);
        for (x, y) in [(50., 20.), (20., 50.), (-10., 20.)] {
            a.begin(point(px(20.), px(20.)), selection());
            a.drag_to(point(px(x), px(y)), selection(), false);
            a.end();
        }
        assert_eq!(a.visible().count(), 3);
        a.undo();
        a.begin(point(px(20.), px(20.)), selection());
        a.end();
        a.redo();
        assert_eq!(a.visible().count(), 3);
    }

    #[test]
    fn line_snapping_preserves_45_degree_angles_at_every_boundary() {
        let start = point(px(30.), px(30.));
        for (x, y) in [
            (-200., -80.),
            (-80., 10.),
            (40., -300.),
            (200., 200.),
            (200., 50.),
            (45., 200.),
        ] {
            let end = super::line_endpoint(start, point(px(x), px(y)), selection(), true);
            // Stroke endpoints may lie exactly on the crop's exclusive right/bottom edge.
            assert!(end.x >= selection().left() && end.x <= selection().right());
            assert!(end.y >= selection().top() && end.y <= selection().bottom());
            let dx = f32::from(end.x - start.x).abs();
            let dy = f32::from(end.y - start.y).abs();
            assert!(dx < 0.001 || dy < 0.001 || (dx - dy).abs() < 0.001);
        }
    }

    #[test]
    fn polyline_commits_clicked_nodes_only_and_undoes_as_one_annotation() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Polyline);
        for (x, y) in [(10., 10.), (40., 40.), (10., 70.)] {
            let p = point(px(x), px(y));
            a.begin(p, selection());
            a.drag_to(p, selection(), false);
            a.end();
        }
        a.drag_to(point(px(60.), px(80.)), selection(), false);
        // A toolbar mouse-up has no corresponding canvas mouse-down.
        a.end();
        assert!(a.is_drawing_polyline());
        a.finish_polyline();
        let shape = a.visible().next().unwrap();
        assert_eq!(shape.points.len(), 3);
        assert_eq!(shape.points[2], point(px(10.), px(70.)));
        a.undo();
        assert_eq!(a.visible().count(), 0);
        a.redo();
        assert_eq!(a.visible().next().unwrap().points.len(), 3);
        a.begin(point(px(10.), px(10.)), selection());
        assert!(a.cancel());
        assert_eq!(a.visible().count(), 1);
        a.begin(point(px(10.), px(10.)), selection());
        a.end();
        a.finish_polyline();
        assert_eq!(a.visible().count(), 1);
    }

    #[test]
    fn line_raster_preserves_gaps_and_has_round_caps_at_fractional_scales() {
        for scale in [1., 1.25, 1.73, 2.] {
            let shape = super::Shape {
                number: None,
                kind: super::ShapeKind::Polyline,
                bounds: selection(),
                points: vec![
                    point(px(10.), px(30.)),
                    point(px(60.), px(30.)),
                    point(px(60.), px(60.)),
                ],
                width: 5.,
                color: 0xff0000ff,
            };
            let w = (100. * scale) as u32;
            let mut rgba = [0, 0, 0, 255].repeat((w * w) as usize);
            let at =
                |x: f32, y: f32| (((y * scale) as usize) * w as usize + (x * scale) as usize) * 4;
            let gap = at(40., 30.);
            rgba[gap..gap + 4].fill(0);
            super::line::rasterize(&shape, &mut rgba, w, w, point(px(0.), px(0.)), scale);
            for (x, y) in [(30., 30.), (60., 30.), (60., 50.)] {
                assert_eq!(&rgba[at(x, y)..at(x, y) + 4], &[255, 0, 0, 255]);
            }
            assert!(rgba[at(8., 30.)] > 200, "round cap at scale {scale}");
            assert_eq!(&rgba[gap..gap + 4], &[0; 4]);
            assert_eq!(&rgba[at(30., 40.)..at(30., 40.) + 4], &[0, 0, 0, 255]);
            assert!(
                rgba.as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] > 0 && p[0] < 255)
            );
            assert_eq!(shape.line_paths(point(px(0.), px(0.))).len(), 2);
        }
    }
    #[test]
    fn sequence_history_reuses_undone_numbers_and_reset_starts_at_one() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Number);
        for expected in 1..=12 {
            a.begin(point(px(30.), px(30.)), selection());
            a.end();
            assert_eq!(a.visible().last().unwrap().number, Some(expected));
        }
        a.undo();
        assert_eq!(a.next_number(), 12);
        a.redo();
        assert_eq!(a.next_number(), 13);
        a.undo();
        a.begin(point(px(30.), px(30.)), selection());
        a.end();
        a.redo();
        assert_eq!(a.visible().count(), 12);
        a.begin(point(px(30.), px(30.)), selection());
        a.cancel();
        assert_eq!(a.next_number(), 13);
        a.toggle(super::ShapeKind::Rectangle);
        rectangle(&mut a);
        assert_eq!(a.next_number(), 13);
        a.reset();
        assert_eq!(a.next_number(), 1);
    }
    #[test]
    fn number_drag_and_size_stay_inside_selection_and_tiny_regions_do_not_count() {
        let mut a = Annotations::default();
        a.toggle(super::ShapeKind::Number);
        a.set_number_size(2);
        a.begin(point(px(-19.), px(1.)), selection());
        a.drag_to(point(px(300.), px(300.)), selection(), false);
        a.end();
        let mark = a.visible().next().unwrap();
        assert_eq!(mark.bounds.size, size(px(40.), px(40.)));
        assert_eq!(mark.bounds.right(), selection().right());
        assert_eq!(mark.bounds.bottom(), selection().bottom());
        assert_eq!(a.width(), 3.);
        let tiny = Bounds::new(point(px(0.), px(0.)), size(px(10.), px(10.)));
        a.begin(point(px(5.), px(5.)), tiny);
        a.end();
        assert_eq!(a.next_number(), 2);
    }
}
