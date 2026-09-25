//! # 内存像素 → gpui 显示的契约集中地
//!
//! **RenderImage 的契约是 BGRA 字节**（gpui 的 img.rs 解码路径同样做了
//! RGBA→BGRA 转换；我们跳过解码直接喂内存，必须自己交换 R/B）。
//! 曾经的教训："截图颜色对、显示颜色错"——两处各自为政的 swap 就是隐患，
//! 统一收口在这。pin.rs 暂用本地副本（挂起中），恢复时改为调用此处。

use std::sync::Arc;

use gpui_kit::*;
use image::{Frame, ImageBuffer};
use smallvec::SmallVec;

/// RGBA8 内存像素 → 可直接 `img()` 显示的 RenderImage（内部完成 R/B 交换）
pub fn rgba_to_render_image(rgba: Vec<u8>, w: u32, h: u32) -> Arc<RenderImage> {
    let mut bgra = rgba;
    let (chunks, _) = bgra.as_chunks_mut::<4>();
    for px in chunks {
        px.swap(0, 2);
    }
    let buf = ImageBuffer::from_raw(w, h, bgra).expect("像素尺寸不一致");
    Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buf), 1)))
}
