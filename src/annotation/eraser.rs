//! Restore capture pixels in gesture order, rather than painting a background color.
use super::{Shape, ShapeKind, line};
use gpui_kit::{Pixels, Point};

pub(super) fn rasterize(
    shape: &Shape,
    rgba: &mut [u8],
    original: &[u8],
    w: u32,
    h: u32,
    origin: Point<Pixels>,
    scale: f32,
) {
    if shape.kind == ShapeKind::Eraser {
        line::coverage(shape, w, h, origin, scale, |offset, coverage| {
            for channel in 0..4 {
                let i = offset + channel;
                rgba[i] = (rgba[i] as f32 * (1. - coverage) + original[i] as f32 * coverage).round()
                    as u8;
            }
        });
    } else {
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
        for row in y(shape.bounds.top())..y(shape.bounds.bottom()) {
            let start = (row * w as usize + x(shape.bounds.left())) * 4;
            let end = (row * w as usize + x(shape.bounds.right())) * 4;
            rgba[start..end].copy_from_slice(&original[start..end]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::Annotations;
    use gpui_kit::{Bounds, point, px, size};

    #[test]
    fn erasure_restores_capture_and_respects_history_and_later_marks() {
        for scale in [1., 1.25, 1.73, 2.] {
            for kind in [ShapeKind::Eraser, ShapeKind::EraserRect] {
                let w = (100. * scale) as u32;
                let origin = point(px(-20.), px(30.));
                let selection = Bounds::new(origin, size(px(100.), px(100.)));
                let at = |x, y| origin + point(px(x), px(y));
                let mut original: Vec<u8> = (0..w * w)
                    .flat_map(|i| [(i % 251) as u8, (i % 199) as u8, 90, 255])
                    .collect();
                original[..4].fill(0); // A transparent gap between screens must stay transparent.
                let render = |a: &Annotations| {
                    let mut pixels = original.clone();
                    a.rasterize(&mut pixels, w, w, origin, scale);
                    pixels
                };
                let mut a = Annotations::default();
                a.toggle(ShapeKind::Mosaic);
                a.begin(at(0., 0.), selection);
                a.drag_to(at(100., 100.), selection, false);
                a.end();
                let marked = render(&a);
                assert_ne!(marked, original);
                a.toggle(kind);
                a.set_width(2);
                a.begin(at(40., 40.), selection);
                a.drag_to(at(60., 60.), selection, false);
                let preview = render(&a);
                a.end();
                assert_eq!(render(&a), preview);
                let center = (((50. * scale) as usize * w as usize) + (50. * scale) as usize) * 4;
                assert_eq!(&preview[center..center + 4], &original[center..center + 4]);
                assert_eq!(&preview[..4], &[0; 4]);
                let outside = (w as usize * 5 + 5) * 4;
                assert_eq!(
                    &preview[outside..outside + 4],
                    &marked[outside..outside + 4]
                );
                a.undo();
                assert_eq!(render(&a), marked);
                a.redo();
                assert_eq!(render(&a), preview);
                a.toggle(ShapeKind::Pencil);
                a.begin(at(50., 50.), selection);
                a.end();
                assert_ne!(
                    &render(&a)[center..center + 4],
                    &original[center..center + 4]
                );
                a.undo();
                assert_eq!(render(&a), preview);
                a.toggle(kind);
                a.begin(at(5., 5.), selection);
                a.drag_to(at(95., 95.), selection, false);
                a.cancel();
                assert_eq!(render(&a), preview);
            }
        }
    }
}
