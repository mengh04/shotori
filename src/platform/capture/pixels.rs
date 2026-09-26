//! # Pure pixel processing: format conversion + transform rotation
//!
//! No wayland/gpui dependencies; everything is unit-testable (rotation
//! semantics were calibrated against grim; small matrices lock them in here).

use super::Transform;

#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_shm;

/// wl_shm format names describe the byte order of the 32-bit word
/// (MSB→LSB); little-endian memory byte order is exactly reversed:
///   Xrgb8888 (XR24) → memory B,G,R,X   Argb8888 → memory B,G,R,A
///   Xbgr8888 (XB24) → memory R,G,B,X   Abgr8888 → memory R,G,B,A
/// Note: the core wl_shm protocol's format is an ordinal (xrgb8888=1),
/// NOT a DRM fourcc!
#[cfg(target_os = "linux")]
pub(super) fn convert_to_rgba(
    bytes: &[u8],
    format: wl_shm::Format,
    w: i32,
    h: i32,
    stride: i32,
    y_invert: bool,
) -> Vec<u8> {
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let is_xrgb = matches!(format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888);

    for y in 0..h as usize {
        let src_row = &bytes[y * stride as usize..][..(w * 4) as usize];
        let dst_y = if y_invert { h as usize - 1 - y } else { y };
        let dst_row = &mut rgba[dst_y * (w * 4) as usize..][..(w * 4) as usize];
        let (src_chunks, _) = src_row.as_chunks::<4>();
        let (dst_chunks, _) = dst_row.as_chunks_mut::<4>();
        for (px, chunk) in src_chunks.iter().zip(dst_chunks) {
            if is_xrgb {
                // B,G,R,(A) → R,G,B,A
                chunk[0] = px[2];
                chunk[1] = px[1];
                chunk[2] = px[0];
                chunk[3] = 255;
            } else {
                // R,G,B,(A) → R,G,B,A
                chunk[0] = px[0];
                chunk[1] = px[1];
                chunk[2] = px[2];
                chunk[3] = 255;
            }
        }
    }
    rgba
}

/// Size after transform (90/270 swap width and height; 180 and the flipped
/// family keep them)
// only the wayland backend captures un-rotated buffers today; the windows
// backend keeps the pure functions exercised through tests
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn rotated_size(w: u32, h: u32, t: Transform) -> (u32, u32) {
    use Transform::*;
    match t {
        Normal | Rot180 | Flipped | Flipped180 => (w, h),
        _ => (h, w),
    }
}

/// Rotate pixels per the output transform so the orientation matches what
/// the screen shows. The physical buffer is in the untransformed orientation.
/// Note: niri's "90° counter-clockwise" (Rot90) actually fills the panel by
/// rotating the buffer **clockwise** 90° (opposite of the protocol wording;
/// pinned down by comparing against grim).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn rotate_rgba(rgba: Vec<u8>, w: u32, h: u32, t: Transform) -> Vec<u8> {
    use Transform::*;
    let (rw, _rh) = rotated_size(w, h, t);
    let mut out = vec![0u8; rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let src = ((y * w + x) * 4) as usize;
            let (dx, dy) = match t {
                Normal | Flipped => (x, y),
                Rot90 => (h - 1 - y, x),
                Rot180 | Flipped180 => (w - 1 - x, h - 1 - y),
                Rot270 => (y, w - 1 - x),
                // Flipped90/Flipped270 and other rare combos: treat as Rot90
                // for now (handle when actually encountered)
                _ => (h - 1 - y, x),
            };
            let dst = ((dy * rw + dx) * 4) as usize;
            out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
        }
    }
    out
}

// tests avoids `use super::*`: the parent module's glob import pulls gpui's
// test macro in and shadows the built-in #[test] (see the comment in
// selection.rs)
#[cfg(test)]
mod tests {
    use super::*;

    /// 3×2 pixel grid, values encode coordinates (x*10+y):
    /// ```text
    ///  00 10 20
    ///  01 11 21
    /// ```
    fn grid(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[x as u8 * 10 + y as u8, 0, 0, 255]);
            }
        }
        v
    }

    fn px_at(rgba: &[u8], w: u32, x: u32, y: u32) -> u8 {
        rgba[(y * w + x) as usize * 4]
    }

    #[test]
    fn rotate_90_clockwise() {
        // niri's _90 (literally 90 CCW) measured = rotate buffer clockwise:
        // 00 10 20        01 00
        // 01 11 21   →    11 10
        //                 21 20
        let out = rotate_rgba(grid(3, 2), 3, 2, Transform::Rot90);
        assert_eq!(out.len(), grid(3, 2).len());
        assert_eq!(px_at(&out, 2, 0, 0), 1); // 01
        assert_eq!(px_at(&out, 2, 1, 0), 0); // 00
        assert_eq!(px_at(&out, 2, 0, 1), 11); // 11
        assert_eq!(px_at(&out, 2, 1, 2), 20); // bottom-right = original top-right
    }

    #[test]
    fn rotate_270_counter_clockwise() {
        // 00 10 20        20 21
        // 01 11 21   →    10 11
        //                 00 01
        let out = rotate_rgba(grid(3, 2), 3, 2, Transform::Rot270);
        assert_eq!(px_at(&out, 2, 0, 0), 20);
        assert_eq!(px_at(&out, 2, 1, 0), 21);
        assert_eq!(px_at(&out, 2, 0, 2), 0); // 00 (bottom-left = original top-left)
        assert_eq!(px_at(&out, 2, 1, 2), 1); // 01
    }

    #[test]
    fn rotate_180() {
        let out = rotate_rgba(grid(3, 2), 3, 2, Transform::Rot180);
        // all four corners correct (rotated_size's _180 branch was once wrong
        // and fake-passed via unwritten memory)
        assert_eq!(px_at(&out, 3, 0, 0), 21); // original bottom-right
        assert_eq!(px_at(&out, 3, 2, 1), 0); // original top-left
        assert_eq!(px_at(&out, 3, 2, 0), 1); // original bottom-left
        assert_eq!(px_at(&out, 3, 0, 1), 20); // original top-right
    }

    #[test]
    fn rotate_swaps_size() {
        use Transform::*;
        assert_eq!(rotated_size(3, 2, Normal), (3, 2));
        assert_eq!(rotated_size(3, 2, Rot90), (2, 3));
        assert_eq!(rotated_size(3, 2, Rot270), (2, 3));
        assert_eq!(rotated_size(3, 2, Rot180), (3, 2));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn convert_xrgb_little_endian_swizzle() {
        // 1×2 pixels, stride=12: each row = [B,G,R,X] + 8 bytes of other
        // content (tests stride-skipping reads)
        let bytes = [
            10, 20, 30, 255, 0, 0, 0, 0, 0, 0, 0, 0, // row 0
            11, 21, 31, 255, 0, 0, 0, 0, 0, 0, 0, 0, // row 1
        ];
        let out = convert_to_rgba(&bytes, wl_shm::Format::Xrgb8888, 1, 2, 12, false);
        assert_eq!(&out[..8], &[30, 20, 10, 255, 31, 21, 11, 255]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn convert_y_invert() {
        // 1×2, stride=8: two rows, each [B,G,R,X]
        let bytes = [10, 20, 30, 255, 0, 0, 0, 0, 11, 21, 31, 255, 0, 0, 0, 0];
        let out = convert_to_rgba(&bytes, wl_shm::Format::Xrgb8888, 1, 2, 8, true);
        // rows swapped: the second row becomes the first
        assert_eq!(&out[..4], &[31, 21, 11, 255]);
        assert_eq!(&out[4..8], &[30, 20, 10, 255]);
    }
}
