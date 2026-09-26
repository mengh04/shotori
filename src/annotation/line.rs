//! Shared rounded stroke geometry for line preview and PNG rasterization.
use gpui_kit::{Path, PathBuilder, Pixels, Point, point, px};

/// A stroke is a union of capsules: round endpoints and round joins, including
/// reversals and self-intersections. Both rendering paths use these polygons.
fn polygons(points: &[Point<Pixels>], width: f32) -> Vec<Vec<Point<Pixels>>> {
    points
        .windows(2)
        .filter_map(|pair| {
            let delta = pair[1] - pair[0];
            let dx = f32::from(delta.x);
            let dy = f32::from(delta.y);
            if dx.hypot(dy) < 0.001 {
                return None;
            }
            let angle = dy.atan2(dx);
            let mut polygon = Vec::with_capacity(34);
            for (center, start) in [
                (pair[1], angle - std::f32::consts::FRAC_PI_2),
                (pair[0], angle + std::f32::consts::FRAC_PI_2),
            ] {
                for i in 0..=16 {
                    let (sin, cos) = (start + i as f32 * std::f32::consts::PI / 16.).sin_cos();
                    polygon.push(center + point(px(cos * width / 2.), px(sin * width / 2.)));
                }
            }
            Some(polygon)
        })
        .collect()
}

pub(super) fn paths(
    points: &[Point<Pixels>],
    width: f32,
    offset: Point<Pixels>,
) -> Vec<Path<Pixels>> {
    polygons(points, width)
        .into_iter()
        .filter_map(|polygon| {
            let mut builder = PathBuilder::fill();
            builder.move_to(polygon[0] + offset);
            for p in &polygon[1..] {
                builder.line_to(*p + offset);
            }
            builder.close();
            builder.build().ok()
        })
        .collect()
}

pub(super) fn rasterize(
    shape: &super::Shape,
    rgba: &mut [u8],
    w: u32,
    h: u32,
    origin: Point<Pixels>,
    scale: f32,
) {
    let points: Vec<_> = shape.points.iter().map(|p| (*p - origin) * scale).collect();
    let polygons = polygons(&points, shape.width * scale);
    if polygons.is_empty() {
        return;
    }
    let vertices = || polygons.iter().flatten();
    let top = vertices()
        .map(|p| f32::from(p.y))
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0., h as f32) as usize;
    let bottom = vertices()
        .map(|p| f32::from(p.y))
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .clamp(0., h as f32) as usize;
    let left = vertices()
        .map(|p| f32::from(p.x))
        .fold(f32::INFINITY, f32::min)
        .floor()
        .clamp(0., w as f32) as usize;
    let right = vertices()
        .map(|p| f32::from(p.x))
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .clamp(0., w as f32) as usize;
    let mut coverage = vec![0_f32; right - left];
    let mut intervals = Vec::with_capacity(polygons.len());
    let color = shape.color.to_be_bytes();
    for row in top..bottom {
        coverage.fill(0.);
        for sample in 0..8 {
            let y = row as f32 + (sample as f32 + 0.5) / 8.;
            intervals.clear();
            for polygon in &polygons {
                let mut lo = f32::INFINITY;
                let mut hi = f32::NEG_INFINITY;
                for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
                    let ay = f32::from(a.y);
                    let by = f32::from(b.y);
                    if (ay <= y && y < by) || (by <= y && y < ay) {
                        let x = f32::from(a.x) + (y - ay) / (by - ay) * f32::from(b.x - a.x);
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                }
                if lo < hi {
                    intervals.push((lo, hi));
                }
            }
            intervals.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
            // Union before blending so joints/crossings never accumulate opacity.
            let mut union: Option<(f32, f32)> = None;
            let mut paint = |start: f32, end: f32| {
                let first = start.floor().clamp(left as f32, right as f32) as usize;
                let last = end.ceil().clamp(left as f32, right as f32) as usize;
                for x in first..last {
                    coverage[x - left] +=
                        (end.min(x as f32 + 1.) - start.max(x as f32)).max(0.) / 8.;
                }
            };
            for &(start, end) in &intervals {
                match union {
                    Some((lo, hi)) if start <= hi => union = Some((lo, hi.max(end))),
                    Some((lo, hi)) => {
                        paint(lo, hi);
                        union = Some((start, end));
                    }
                    None => union = Some((start, end)),
                }
            }
            if let Some((start, end)) = union {
                paint(start, end);
            }
        }
        for (ix, coverage) in coverage.iter().copied().enumerate() {
            let offset = (row * w as usize + left + ix) * 4;
            if coverage == 0. || rgba[offset + 3] == 0 {
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
