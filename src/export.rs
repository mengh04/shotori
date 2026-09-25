//! # 导出管线：逻辑选区 → 物理像素裁剪 → PNG 编码 → 落盘
//!
//! 纯函数集合（不碰 Wayland/gpui 窗口）→ 用合成像素即可单元测试。
//! 调用方（overlay）只负责传窗口 scale_factor 和错误提示。

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui_kit::*;

/// 逻辑选区 → 物理像素裁剪。
/// 坐标乘 scale 后取整、夹紧到捕获范围；空选区返回 `None`。
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

/// 生成不冲突的保存路径：`~/Pictures/Shotori/Shotori_年-月-日_时-分-秒.png`
/// （同一秒内多次保存自动加 `_2`、`_3` 后缀，不覆盖）
pub fn next_path() -> anyhow::Result<PathBuf> {
    let dir = save_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("创建目录 {}", dir.display()))?;
    let stamp = chrono::Local::now().format("Shotori_%Y-%m-%d_%H-%M-%S").to_string();
    Ok(next_path_in(&dir, &stamp))
}

fn save_dir() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("没有 HOME 环境变量")?;
    Ok(PathBuf::from(home).join("Pictures/Shotori"))
}

/// 冲突规避的纯逻辑（可测试）：`stem.png` 占用时依次尝试 `stem_2.png`、`stem_3.png`…
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

/// RGBA8 像素编码为 PNG（内存版，剪贴板/落盘共用）
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().context("PNG header")?;
    writer.write_image_data(rgba).context("PNG 数据")?;
    writer.finish().context("PNG IEND 尾块")?;
    Ok(out)
}

/// RGBA8 像素编码为 PNG 写入 path。错误带上下文返回（调用方决定去留）。
pub fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let bytes = encode_png(w, h, rgba)?;
    std::fs::write(path, &bytes)
        .with_context(|| format!("写入 {}（{} KB）", path.display(), bytes.len() / 1024))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // 显式导入（同 selection.rs 的理由：避开 gpui 的 test 宏遮蔽内建 #[test]）
    use super::{crop, next_path_in, save_png};
    use gpui_kit::{point, px, size, Bounds, Pixels};

    /// 4×3 合成图：像素值 = (x, y, 0, 255)，方便断言坐标映射
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
    fn 裁剪_scale1_精确取像素() {
        let (w, h, rgba) = gradient_4x3();
        let (cw, ch, out) = crop(&rgba, w, h, bounds(1., 1., 2., 1.), 1.0).unwrap();
        assert_eq!((cw, ch), (2, 1));
        // 两个像素：(1,1) 和 (2,1)
        assert_eq!(&out[..4], &[1, 1, 0, 255]);
        assert_eq!(&out[4..8], &[2, 1, 0, 255]);
    }

    #[test]
    fn 裁剪_scale2_逻辑转物理() {
        let (w, h, rgba) = gradient_4x3();
        // 逻辑 (0.5, 0.5) 尺寸 1×1 → 物理 x∈[1,3) y∈[1,3) → 2×2 像素
        let (cw, ch, out) = crop(&rgba, w, h, bounds(0.5, 0.5, 1., 1.), 2.0).unwrap();
        assert_eq!((cw, ch), (2, 2));
        assert_eq!(&out[..4], &[1, 1, 0, 255]); // 第一行左起 (1,1)
        assert_eq!(&out[4..8], &[2, 1, 0, 255]); // (2,1)
    }

    #[test]
    fn 裁剪_越界部分被夹紧() {
        let (w, h, rgba) = gradient_4x3();
        // 右下超出捕获范围：clamp 到 (4,3)，可得 2×2
        let (cw, ch, _) = crop(&rgba, w, h, bounds(2., 1., 99., 99.), 1.0).unwrap();
        assert_eq!((cw, ch), (2, 2));
    }

    #[test]
    fn 裁剪_空选区返回_none() {
        let (w, h, rgba) = gradient_4x3();
        assert!(crop(&rgba, w, h, bounds(4., 0., 4., 3.), 1.0).is_none()); // 零宽
        assert!(crop(&rgba, w, h, bounds(0., 3., 4., 0.), 1.0).is_none()); // 零高
    }

    #[test]
    fn 文件名_冲突时加后缀() {
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
    fn png_编码后能读回() {
        let (w, h, rgba) = gradient_4x3();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtrip.png");
        save_png(&path, w, h, &rgba).unwrap();

        let img = image::open(&path).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (w, h));
        assert_eq!(img.get_pixel(2, 1).0, [2, 1, 0, 255]);
    }
}
