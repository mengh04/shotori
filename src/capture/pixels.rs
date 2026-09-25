//! # 纯像素处理：格式转换 + transform 旋转
//!
//! 无 wayland/gpui 依赖，全部可单元测试（旋转语义已被 grim 对拍校准，
//! 这里用小矩阵把语义锁死）。

use super::wayland::OutputTransform;
use wayland_client::protocol::wl_shm;

/// wl_shm 格式名描述 32 位字的位序（MSB→LSB），小端内存字节序正好相反：
///   Xrgb8888（XR24）→ 内存 B,G,R,X   Argb8888 → 内存 B,G,R,A
///   Xbgr8888（XB24）→ 内存 R,G,B,X   Abgr8888 → 内存 R,G,B,A
/// 注意：wl_shm 核心协议的 format 是序号（xrgb8888=1），不是 DRM fourcc！
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

/// transform 后的尺寸（90/270 交换宽高；180 及翻转系不变）
pub(super) fn rotated_size(w: u32, h: u32, t: OutputTransform) -> (u32, u32) {
    use OutputTransform::*;
    match t {
        Normal | _180 | Flipped | Flipped180 => (w, h),
        _ => (h, w),
    }
}

/// 按 wl_output transform 旋转像素，使方向与屏幕所见一致。
/// 物理 buffer 是未变换方向。注意：niri 的 "90° counter-clockwise"（_90）
/// 实测是把 buffer **顺时针**转 90° 填进面板（与协议字面相反，grim 对拍定位）。
#[allow(clippy::just_underscores_and_digits)] // _90/_180/_270 是协议生成的枚举名
pub(super) fn rotate_rgba(rgba: Vec<u8>, w: u32, h: u32, t: OutputTransform) -> Vec<u8> {
    use OutputTransform::*;
    let (rw, _rh) = rotated_size(w, h, t);
    let mut out = vec![0u8; rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let src = ((y * w + x) * 4) as usize;
            let (dx, dy) = match t {
                Normal | Flipped => (x, y),
                _90 => (h - 1 - y, x),
                _180 | Flipped180 => (w - 1 - x, h - 1 - y),
                _270 => (y, w - 1 - x),
                // Flipped90/Flipped270 等罕见组合：先按 _90 处理（回头遇到再补）
                _ => (h - 1 - y, x),
            };
            let dst = ((dy * rw + dx) * 4) as usize;
            out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
        }
    }
    out
}

// tests 模块不 `use super::*`：父模块的 glob 会把 gpui 的 test 宏带进来
// 遮蔽内建 #[test]（详见 selection.rs 的注释）
#[cfg(test)]
mod tests {
    use super::*;
    use wayland_client::protocol::wl_output;

    /// 3×2 像素图，值为坐标编码 (x*10+y)：
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
    fn 旋转_90_顺时针() {
        // niri 的 _90（字面 90CCW）实测 = 顺时针转 buffer：
        // 00 10 20        01 00
        // 01 11 21   →    11 10
        //                 21 20
        let out = rotate_rgba(grid(3, 2), 3, 2, wl_output::Transform::_90);
        assert_eq!(out.len(), grid(3, 2).len());
        assert_eq!(px_at(&out, 2, 0, 0), 1); // 01
        assert_eq!(px_at(&out, 2, 1, 0), 0); // 00
        assert_eq!(px_at(&out, 2, 0, 1), 11); // 11
        assert_eq!(px_at(&out, 2, 1, 2), 20); // 右下角 = 原右上角
    }

    #[test]
    fn 旋转_270_逆时针() {
        // 00 10 20        20 21
        // 01 11 21   →    10 11
        //                 00 01
        let out = rotate_rgba(grid(3, 2), 3, 2, wl_output::Transform::_270);
        assert_eq!(px_at(&out, 2, 0, 0), 20);
        assert_eq!(px_at(&out, 2, 1, 0), 21);
        assert_eq!(px_at(&out, 2, 0, 2), 0); // 00（左下 = 原左上）
        assert_eq!(px_at(&out, 2, 1, 2), 1); // 01
    }

    #[test]
    fn 旋转_180() {
        let out = rotate_rgba(grid(3, 2), 3, 2, wl_output::Transform::_180);
        // 四角全对（此前 rotated_size 的 _180 分支写错，靠未写内存假通过过一次）
        assert_eq!(px_at(&out, 3, 0, 0), 21); // 原右下
        assert_eq!(px_at(&out, 3, 2, 1), 0); // 原左上
        assert_eq!(px_at(&out, 3, 2, 0), 1); // 原左下
        assert_eq!(px_at(&out, 3, 0, 1), 20); // 原右上
    }

    #[test]
    fn 旋转_尺寸互换() {
        use wl_output::Transform::*;
        assert_eq!(rotated_size(3, 2, Normal), (3, 2));
        assert_eq!(rotated_size(3, 2, _90), (2, 3));
        assert_eq!(rotated_size(3, 2, _270), (2, 3));
        assert_eq!(rotated_size(3, 2, _180), (3, 2));
    }

    #[test]
    fn 转换_xrgb_小端字节序换位() {
        // 1×2 像素、stride=12：每行 = [B,G,R,X] + 8 字节其他内容（测行距跳读）
        let bytes = [
            10, 20, 30, 255, 0, 0, 0, 0, 0, 0, 0, 0, // 行0
            11, 21, 31, 255, 0, 0, 0, 0, 0, 0, 0, 0, // 行1
        ];
        let out = convert_to_rgba(&bytes, wl_shm::Format::Xrgb8888, 1, 2, 12, false);
        assert_eq!(&out[..8], &[30, 20, 10, 255, 31, 21, 11, 255]);
    }

    #[test]
    fn 转换_y翻转() {
        // 1×2、stride=8：两行各 [B,G,R,X]
        let bytes = [10, 20, 30, 255, 0, 0, 0, 0, 11, 21, 31, 255, 0, 0, 0, 0];
        let out = convert_to_rgba(&bytes, wl_shm::Format::Xrgb8888, 1, 2, 8, true);
        // 两行交换：第二行变第一行
        assert_eq!(&out[..4], &[31, 21, 11, 255]);
        assert_eq!(&out[4..8], &[30, 20, 10, 255]);
    }
}
