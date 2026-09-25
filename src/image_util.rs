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
    let (chunks, _) = bgra.as_chunks_mut::<4>();
    for px in chunks {
        px.swap(0, 2);
    }
    let buf = ImageBuffer::from_raw(w, h, bgra).expect("pixel buffer size mismatch");
    Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buf), 1)))
}