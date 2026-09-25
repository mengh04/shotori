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

fn save_dir() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home).join("Pictures/Shotori"))
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

/// Save a PNG named with local date and time, including milliseconds.
/// Returns the actual path; concurrent saves never overwrite an existing file.
pub fn save_png(w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<PathBuf> {
    let stamp = chrono::Local::now()
        .format("Shotori_%Y-%m-%d_%H-%M-%S_%3f")
        .to_string();
    save_png_in(&save_dir()?, &stamp, w, h, rgba)
}

fn save_png_in(dir: &Path, stem: &str, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<PathBuf> {
    use std::io::{ErrorKind, Write as _};

    let bytes = encode_png(w, h, rgba)?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for n in 1u64.. {
        let name = if n == 1 {
            format!("{stem}.png")
        } else {
            format!("{stem}_{n}.png")
        };
        let path = dir.join(name);
        // Reserve and open in one operation. An existence check followed by
        // std::fs::write would race with another Shotori process.
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
        };
        if let Err(e) = file.write_all(&bytes) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(e).with_context(|| format!("writing {}", path.display()));
        }
        return Ok(path);
    }
    anyhow::bail!("no available screenshot filename for {stem}")
}

#[cfg(test)]
mod tests {
    // Explicit imports (same reason as selection.rs: avoid gpui's test macro
    // shadowing the built-in #[test])
    use super::{crop, save_png_in};
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
    fn existing_screenshot_is_preserved_on_timestamp_collision() {
        let dir = tempfile::tempdir().unwrap();
        let stem = "Shotori_2026-09-26_12-34-56_789";
        let existing = dir.path().join(format!("{stem}.png"));
        std::fs::write(&existing, b"existing screenshot").unwrap();
        let (w, h, rgba) = gradient_4x3();
        let saved = save_png_in(dir.path(), stem, w, h, &rgba).unwrap();
        assert_eq!(saved, dir.path().join(format!("{stem}_2.png")));
        assert_eq!(std::fs::read(existing).unwrap(), b"existing screenshot");
        assert_eq!(image::open(saved).unwrap().to_rgba8().into_raw(), rgba);
    }

    #[test]
    fn concurrent_saves_with_the_same_timestamp_keep_every_image() {
        use std::sync::{Arc, Barrier};

        let dir = tempfile::tempdir().unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8u8)
            .map(|i| {
                let dir = dir.path().to_path_buf();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let pixel = [i, 0, 0, 255];
                    barrier.wait();
                    let path =
                        save_png_in(&dir, "Shotori_2026-09-26_12-34-56_789", 1, 1, &pixel).unwrap();
                    (path, pixel)
                })
            })
            .collect();
        let saved: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        let unique: std::collections::HashSet<_> = saved.iter().map(|(p, _)| p).collect();
        assert_eq!(unique.len(), 8);
        for (path, pixel) in saved {
            assert_eq!(
                image::open(path).unwrap().to_rgba8().get_pixel(0, 0).0,
                pixel
            );
        }
    }

    #[test]
    fn png_encodes_and_reads_back() {
        let (w, h, rgba) = gradient_4x3();
        let dir = tempfile::tempdir().unwrap();
        let path = save_png_in(dir.path(), "roundtrip", w, h, &rgba).unwrap();

        let img = image::open(&path).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (w, h));
        assert_eq!(img.get_pixel(2, 1).0, [2, 1, 0, 255]);
    }
}
