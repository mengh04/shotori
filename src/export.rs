//! # Export pipeline: logical selection → physical-pixel crop → PNG → disk
//!
//! A collection of pure functions (no Wayland/gpui window access) → unit
//! testable with synthetic pixels. Callers (the overlay) only pass the window
//! scale factor and handle error reporting.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui_kit::*;

/// Logical selection → physical-pixel crop.
/// Coordinates are scaled, rounded and clamped into the capture; an empty
/// selection returns `None`.
pub fn crop(
    rgba: &[u8],
    cap_w: u32,
    cap_h: u32,
    bounds: Bounds<Pixels>,
    scale: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    let clamp = |v: f32, max: u32| v.round().clamp(0., max as f32) as u32;
    let x0 = clamp(f32::from(bounds.left()) * scale, cap_w);
    let y0 = clamp(f32::from(bounds.top()) * scale, cap_h);
    let x1 = clamp(f32::from(bounds.right()) * scale, cap_w);
    let y1 = clamp(f32::from(bounds.bottom()) * scale, cap_h);
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w == 0 || h == 0 {
        return None;
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    let cap_stride = cap_w as usize * 4;
    for row in 0..h as usize {
        let src = (y0 as usize + row) * cap_stride + x0 as usize * 4;
        let dst = row * (w as usize * 4);
        out[dst..dst + w as usize * 4].copy_from_slice(&rgba[src..src + w as usize * 4]);
    }
    Some((w, h, out))
}

/// Generate a collision-free save path:
/// `~/Pictures/Shotori/Shotori_<date>_<time>.png`
/// (repeated saves within the same second get `_2`, `_3` suffixes; no overwrites)
pub fn next_path() -> anyhow::Result<PathBuf> {
    let dir = save_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let stamp = chrono::Local::now()
        .format("Shotori_%Y-%m-%d_%H-%M-%S")
        .to_string();
    Ok(next_path_in(&dir, &stamp))
}

fn save_dir() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home).join("Pictures/Shotori"))
}

/// Collision-avoidance pure logic (testable): tries `stem.png`, then
/// `stem_2.png`, `stem_3.png`…
pub fn next_path_in(dir: &Path, stem: &str) -> PathBuf {
    let plain = dir.join(format!("{stem}.png"));
    if !plain.exists() {
        return plain;
    }
    for n in 2.. {
        let p = dir.join(format!("{stem}_{n}.png"));
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

/// Encode RGBA8 pixels as PNG (in memory; shared by clipboard and disk)
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().context("PNG header")?;
    writer.write_image_data(rgba).context("PNG data")?;
    writer.finish().context("PNG IEND chunk")?;
    Ok(out)
}

/// Encode RGBA8 pixels as PNG and write to path. Errors are returned with
/// context (the caller decides whether to stay open).
pub fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let bytes = encode_png(w, h, rgba)?;
    std::fs::write(path, &bytes)
        .with_context(|| format!("writing {} ({} KB)", path.display(), bytes.len() / 1024))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // Explicit imports (same reason as selection.rs: avoid gpui's test macro
    // shadowing the built-in #[test])
    use super::{crop, next_path_in, save_png};
    use gpui_kit::{Bounds, Pixels, point, px, size};

    /// 4×3 synthetic image: pixel value = (x, y, 0, 255) for easy
    /// coordinate-mapping assertions
    fn gradient_4x3() -> (u32, u32, Vec<u8>) {
        let (w, h) = (4u32, 3u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                rgba[i] = x as u8;
                rgba[i + 1] = y as u8;
                rgba[i + 2] = 0;
                rgba[i + 3] = 255;
            }
        }
        (w, h, rgba)
    }

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    #[test]
    fn crop_scale1_extracts_exact_pixels() {
        let (w, h, rgba) = gradient_4x3();
        let (cw, ch, out) = crop(&rgba, w, h, bounds(1., 1., 2., 1.), 1.0).unwrap();
        assert_eq!((cw, ch), (2, 1));
        // two pixels: (1,1) and (2,1)
        assert_eq!(&out[..4], &[1, 1, 0, 255]);
        assert_eq!(&out[4..8], &[2, 1, 0, 255]);
    }

    #[test]
    fn crop_scale2_logical_to_physical() {
        let (w, h, rgba) = gradient_4x3();
        // logical (0.5, 0.5) size 1×1 → physical x∈[1,3) y∈[1,3) → 2×2 pixels
        let (cw, ch, out) = crop(&rgba, w, h, bounds(0.5, 0.5, 1., 1.), 2.0).unwrap();
        assert_eq!((cw, ch), (2, 2));
        assert_eq!(&out[..4], &[1, 1, 0, 255]); // first row, leftmost (1,1)
        assert_eq!(&out[4..8], &[2, 1, 0, 255]); // (2,1)
    }

    #[test]
    fn crop_clamps_out_of_bounds() {
        let (w, h, rgba) = gradient_4x3();
        // bottom-right beyond the capture: clamped to (4,3), yields 2×2
        let (cw, ch, _) = crop(&rgba, w, h, bounds(2., 1., 99., 99.), 1.0).unwrap();
        assert_eq!((cw, ch), (2, 2));
    }

    #[test]
    fn crop_empty_selection_returns_none() {
        let (w, h, rgba) = gradient_4x3();
        assert!(crop(&rgba, w, h, bounds(4., 0., 4., 3.), 1.0).is_none()); // zero width
        assert!(crop(&rgba, w, h, bounds(0., 3., 4., 0.), 1.0).is_none()); // zero height
    }

    #[test]
    fn filename_collision_gets_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = next_path_in(dir.path(), "Shotori_t");
        assert_eq!(p1, dir.path().join("Shotori_t.png"));
        std::fs::write(&p1, b"x").unwrap();

        let p2 = next_path_in(dir.path(), "Shotori_t");
        assert_eq!(p2, dir.path().join("Shotori_t_2.png"));
        std::fs::write(&p2, b"x").unwrap();

        let p3 = next_path_in(dir.path(), "Shotori_t");
        assert_eq!(p3, dir.path().join("Shotori_t_3.png"));
    }

    #[test]
    fn png_encodes_and_reads_back() {
        let (w, h, rgba) = gradient_4x3();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtrip.png");
        save_png(&path, w, h, &rgba).unwrap();

        let img = image::open(&path).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (w, h));
        assert_eq!(img.get_pixel(2, 1).0, [2, 1, 0, 255]);
    }
}
