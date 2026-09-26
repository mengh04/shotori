//! Rectangular pixel filters, applied in annotation order to the composed image.
use super::{Shape, ShapeKind};
use gpui_kit::{Pixels, Point};

pub(super) fn rasterize(
    shape: &Shape,
    rgba: &mut [u8],
    w: u32,
    h: u32,
    origin: Point<Pixels>,
    scale: f32,
) {
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
    let (left, right, top, bottom) = (
        x(shape.bounds.left()),
        x(shape.bounds.right()),
        y(shape.bounds.top()),
        y(shape.bounds.bottom()),
    );
    if right <= left || bottom <= top {
        return;
    }
    let (width, height) = (right - left, bottom - top);
    let stride = w as usize;
    let strength = (shape.width * scale).round().max(1.) as usize;
    if shape.kind == ShapeKind::Mosaic {
        for y in (top..bottom).step_by(strength) {
            for x in (left..right).step_by(strength) {
                let end_x = (x + strength).min(right);
                let end_y = (y + strength).min(bottom);
                let mut sum = [0_u64; 4];
                for row in y..end_y {
                    for col in x..end_x {
                        let pixel = &rgba[(row * stride + col) * 4..][..4];
                        for c in 0..3 {
                            sum[c] += pixel[c] as u64 * pixel[3] as u64;
                        }
                        sum[3] += pixel[3] as u64;
                    }
                }
                if sum[3] == 0 {
                    continue;
                }
                for row in y..end_y {
                    for col in x..end_x {
                        let pixel = &mut rgba[(row * stride + col) * 4..][..4];
                        if pixel[3] == 0 {
                            continue;
                        }
                        for c in 0..3 {
                            pixel[c] = ((sum[c] + sum[3] / 2) / sum[3]) as u8;
                        }
                    }
                }
            }
        }
    } else {
        // Separable sliding sums: O(area), even at high strength. Weight by alpha
        // so transparent desktop gaps never introduce a dark halo.
        let radius = (strength / 2).max(1);
        let mut horizontal = vec![[0_u64; 4]; width * height];
        for row in 0..height {
            let mut sum = [0_u64; 4];
            let add = |sum: &mut [u64; 4], col: usize, subtract: bool| {
                let p = &rgba[((top + row) * stride + left + col) * 4..][..4];
                for c in 0..4 {
                    let v = if c == 3 {
                        p[3] as u64
                    } else {
                        p[c] as u64 * p[3] as u64
                    };
                    if subtract {
                        sum[c] -= v;
                    } else {
                        sum[c] += v;
                    }
                }
            };
            for col in 0..=radius.min(width - 1) {
                add(&mut sum, col, false);
            }
            for col in 0..width {
                horizontal[row * width + col] = sum;
                if col >= radius {
                    add(&mut sum, col - radius, true);
                }
                if col + radius + 1 < width {
                    add(&mut sum, col + radius + 1, false);
                }
            }
        }
        for col in 0..width {
            let mut sum = [0_u64; 4];
            for row in 0..=radius.min(height - 1) {
                for c in 0..4 {
                    sum[c] += horizontal[row * width + col][c];
                }
            }
            for row in 0..height {
                let p = &mut rgba[((top + row) * stride + left + col) * 4..][..4];
                if p[3] != 0 && sum[3] != 0 {
                    for c in 0..3 {
                        p[c] = ((sum[c] + sum[3] / 2) / sum[3]) as u8;
                    }
                }
                if row >= radius {
                    for c in 0..4 {
                        sum[c] -= horizontal[(row - radius) * width + col][c];
                    }
                }
                if row + radius + 1 < height {
                    for c in 0..4 {
                        sum[c] += horizontal[(row + radius + 1) * width + col][c];
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Bounds, point, px, size};
    fn shape(kind: ShapeKind) -> Shape {
        Shape {
            kind,
            number: None,
            bounds: Bounds::new(point(px(2.), px(2.)), size(px(8.), px(8.))),
            color: 0,
            width: 4.,
            points: vec![],
        }
    }
    #[test]
    fn mosaic_averages_each_tile_and_preserves_outside_and_gaps() {
        let mut data = [20, 40, 60, 255].repeat(12 * 12);
        for y in 2..6 {
            for x in 2..6 {
                data[(y * 12 + x) * 4] = if x < 4 { 0 } else { 100 };
            }
        }
        data[(3 * 12 + 3) * 4..(3 * 12 + 3) * 4 + 4].fill(0);
        let before = data.clone();
        rasterize(
            &shape(ShapeKind::Mosaic),
            &mut data,
            12,
            12,
            point(px(0.), px(0.)),
            1.,
        );
        assert_eq!(
            &data[(2 * 12 + 2) * 4..(2 * 12 + 2) * 4 + 4],
            &[53, 40, 60, 255]
        );
        assert_eq!(&data[(3 * 12 + 3) * 4..(3 * 12 + 3) * 4 + 4], &[0; 4]);
        for y in 0..12 {
            for x in 0..12 {
                if !(2..10).contains(&x) || !(2..10).contains(&y) {
                    let at = (y * 12 + x) * 4;
                    assert_eq!(&data[at..at + 4], &before[at..at + 4]);
                }
            }
        }
    }
    #[test]
    fn blur_matches_naive_alpha_weighted_reference() {
        let before: Vec<_> = (0..144)
            .flat_map(|i| {
                [
                    (i * 11 % 256) as u8,
                    70,
                    120,
                    if i % 7 == 0 { 0 } else { 255 },
                ]
            })
            .collect();
        let mut actual = before.clone();
        rasterize(
            &shape(ShapeKind::Blur),
            &mut actual,
            12,
            12,
            point(px(0.), px(0.)),
            1.,
        );
        for y in 2_usize..10 {
            for x in 2_usize..10 {
                let at = (y * 12 + x) * 4;
                if before[at + 3] == 0 {
                    assert_eq!(&actual[at..at + 4], &before[at..at + 4]);
                    continue;
                }
                let mut total = [0_u64; 4];
                for row in y.saturating_sub(2).max(2)..=(y + 2).min(9) {
                    for col in x.saturating_sub(2).max(2)..=(x + 2).min(9) {
                        let p = &before[(row * 12 + col) * 4..][..4];
                        for c in 0..3 {
                            total[c] += p[c] as u64 * p[3] as u64;
                        }
                        total[3] += p[3] as u64;
                    }
                }
                for c in 0..3 {
                    assert_eq!(actual[at + c], ((total[c] + total[3] / 2) / total[3]) as u8);
                }
            }
        }
    }
    #[test]
    fn filters_handle_reverse_origin_fractional_scale_and_tiny_clipped_regions() {
        for kind in [ShapeKind::Mosaic, ShapeKind::Blur] {
            for scale in [1., 1.25, 1.73, 2.] {
                let mut mark = shape(kind);
                mark.bounds.origin = point(px(-4.), px(-4.));
                let mut pixels = [20, 40, 60, 255].repeat(12 * 12);
                rasterize(&mark, &mut pixels, 12, 12, point(px(-2.), px(-2.)), scale);
                assert!(
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| *p == [20, 40, 60, 255])
                );
                mark.bounds.size = size(px(0.1), px(0.1));
                rasterize(&mark, &mut pixels, 12, 12, point(px(0.), px(0.)), scale);
            }
        }
    }
}
