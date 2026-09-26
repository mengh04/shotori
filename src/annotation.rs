//! Rectangle annotations in desktop logical coordinates, shared by all outputs.
use gpui_kit::{Bounds, Pixels, Point, point, px, size};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rectangle {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) color: u32,
    pub(crate) width: f32,
}

impl Rectangle {
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
    rectangle: Rectangle,
}

pub(crate) struct Annotations {
    enabled: bool,
    color_ix: usize,
    width_ix: usize,
    rectangles: Vec<Rectangle>,
    undone: Vec<Rectangle>,
    draft: Option<Draft>,
}

impl Default for Annotations {
    fn default() -> Self {
        Self {
            enabled: false,
            color_ix: 0,
            width_ix: 1,
            rectangles: Vec::new(),
            undone: Vec::new(),
            draft: None,
        }
    }
}

impl Annotations {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(crate) fn color(&self) -> (u32, &'static str) {
        crate::theme::ANNOTATION_COLORS[self.color_ix]
    }
    pub(crate) fn width(&self) -> f32 {
        [1., 3., 5.][self.width_ix]
    }
    pub(crate) fn toggle(&mut self) {
        self.draft = None;
        self.enabled = !self.enabled;
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
        self.rectangles.clear();
        self.undone.clear();
        self.draft = None;
        self.enabled = false;
    }

    pub(crate) fn begin(&mut self, p: Point<Pixels>, selection: Bounds<Pixels>) {
        if !self.enabled || !selection.contains(&p) {
            return;
        }
        self.draft = Some(Draft {
            start: p,
            rectangle: Rectangle {
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
        let changed = draft.rectangle.bounds != bounds;
        draft.rectangle.bounds = bounds;
        changed
    }

    pub(crate) fn end(&mut self) {
        if let Some(draft) = self.draft.take()
            && draft.rectangle.bounds.size.width >= px(2.)
            && draft.rectangle.bounds.size.height >= px(2.)
        {
            self.rectangles.push(draft.rectangle);
            self.undone.clear();
        }
    }

    /// Escape cancels a stroke first, then leaves the tool while keeping marks.
    pub(crate) fn cancel(&mut self) -> bool {
        if self.draft.take().is_some() {
            return true;
        }
        if self.enabled {
            self.enabled = false;
            return true;
        }
        false
    }
    pub(crate) fn undo(&mut self) {
        if self.draft.take().is_some() {
            return;
        }
        if let Some(rectangle) = self.rectangles.pop() {
            self.undone.push(rectangle);
        }
    }
    pub(crate) fn redo(&mut self) {
        if self.draft.is_none()
            && let Some(rectangle) = self.undone.pop()
        {
            self.rectangles.push(rectangle);
        }
    }
    pub(crate) fn visible(&self) -> impl Iterator<Item = Rectangle> + '_ {
        self.rectangles
            .iter()
            .copied()
            .chain(self.draft.map(|draft| draft.rectangle))
    }

    pub(crate) fn rasterize(
        &self,
        rgba: &mut [u8],
        w: u32,
        h: u32,
        origin: Point<Pixels>,
        scale: f32,
    ) {
        for rectangle in self.visible() {
            let color = rectangle.color.to_be_bytes();
            for stroke in rectangle.strokes() {
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
        a.toggle();
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
        a.toggle();
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
        a.toggle();
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
        a.toggle();
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
        a.toggle();
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
}
