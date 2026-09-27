//! # In-memory pixels → gpui display: the one place that owns the contract
//!
//! **RenderImage expects BGRA bytes** (gpui's img.rs decode path performs the
//! same RGBA→BGRA swap; we skip decoding and feed memory directly, so we must
//! swap R/B ourselves). Hard-learned lesson: "capture looks right, display
//! looks wrong" — two independent swaps scattered around is a trap, so the
//! conversion is centralized here.

use std::sync::Arc;

use gpui_kit::*;
use image::{Frame, ImageBuffer};
use smallvec::SmallVec;

/// RGBA8 in-memory pixels → a RenderImage ready for `img()` (swaps R/B inside)
pub fn rgba_to_render_image(rgba: Vec<u8>, w: u32, h: u32) -> Arc<RenderImage> {
    let mut bgra = rgba;
    // Whole-pixel u32 swap (little-endian word R|G<<8|B<<16|A<<24 →
    // B|G<<8|R<<16|A<<24): two shifts + two ANDs/ORS per pixel, fully
    // register-resident. The API does not promise 4-byte alignment —
    // fall back to byte swaps for the rare unaligned buffer.
    let n = (w * h) as usize;
    let ptr = bgra.as_mut_ptr() as usize;
    if n > 0 && ptr.is_multiple_of(4) {
        let words = unsafe { std::slice::from_raw_parts_mut(bgra.as_mut_ptr().cast::<u32>(), n) };
        for word in words {
            let w0 = *word;
            *word = (w0 & 0xFF00_FF00) | ((w0 & 0xFF) << 16) | ((w0 >> 16) & 0xFF);
        }
    } else {
        let (chunks, _) = bgra.as_chunks_mut::<4>();
        for px in chunks {
            px.swap(0, 2);
        }
    }
    let buf = ImageBuffer::from_raw(w, h, bgra).expect("pixel buffer size mismatch");
    Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buf), 1)))
}

/// Images painted directly on a canvas bypass GPUI's managed image element.
/// Retain only this window's current frame and explicitly evict retired atlas tiles.
#[derive(Default)]
pub(crate) struct CanvasImages {
    current: Vec<Arc<RenderImage>>,
}

impl CanvasImages {
    #[cfg(test)]
    pub(crate) fn current(&self) -> &[Arc<RenderImage>] {
        &self.current
    }

    pub(crate) fn replace(&mut self, next: Vec<Arc<RenderImage>>) -> Vec<Arc<RenderImage>> {
        let retired = self
            .current
            .drain(..)
            .filter(|old| !next.iter().any(|new| old.id == new.id))
            .collect();
        self.current = next;
        retired
    }
}

#[cfg(test)]
mod tests {
    use super::{CanvasImages, rgba_to_render_image};

    #[test]
    fn changing_canvas_images_retire_old_frames_but_keep_shared_images() {
        let fixed = rgba_to_render_image(vec![255; 16], 2, 2);
        let mut images = CanvasImages::default();
        let mut previous = None;
        for _ in 0..2000 {
            let fresh = rgba_to_render_image(vec![255; 16], 2, 2);
            let retired = images.replace(vec![fixed.clone(), fresh.clone()]);
            assert_eq!(retired.len(), usize::from(previous.is_some()));
            if let Some(previous) = previous {
                assert_eq!(retired[0].id, previous);
            }
            previous = Some(fresh.id);
            assert_eq!(images.current.len(), 2);
        }
        assert_eq!(images.replace(Vec::new()).len(), 2);
        assert!(images.current.is_empty());
    }
}
