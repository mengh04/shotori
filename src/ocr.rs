//! # OCR：选区 → PP-OCRv6（rapidocr-core + ONNX Runtime）→ 文本
//!
//! 引擎懒加载（OnceLock<Mutex>）：首次 Ctrl+O 时 ~1-2s 初始化（含模型下载），
//! 后续调用 <200ms。模型缓存在 `~/.local/share/shotori/ocr-models/`。
//!
//! 模块仅在 `--features ocr` 时编译；默认 cargo build 零增量。

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::Context as _;
use rapidocr_core::model::PPOCRV6_SMALL;

/// 模型缓存目录（XDG 兼容）
fn model_dir() -> anyhow::Result<PathBuf> {
    let base = if let Ok(d) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(d)
    } else {
        let home = std::env::var("HOME").context("没有 HOME 环境变量")?;
        PathBuf::from(home).join(".local/share")
    };
    Ok(base.join("shotori/ocr-models"))
}

/// 一次性初始化：下载模型（首次）→ 建 OCR 引擎。失败 panic（终端报错），
/// OnceLock 未 set 成功不留下脏状态，下次 Ctrl+O 重新尝试。
fn init_engine() -> Mutex<rapidocr_core::RapidOcr> {
    let dir = model_dir().expect("HOME 环境变量必需");

    // 确保模型存在（首次自动从 ModelScope 下载）
    std::thread::Builder::new()
        .name("shotori-ocr-model-dl".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || rapidocr_core::model::ensure_ppocrv6_small_models(&dir))
        .expect("启动模型下载线程失败")
        .join()
        .expect("模型下载线程 panic")
        .expect("模型下载失败");

    let cfg = PPOCRV6_SMALL.config(model_dir().expect("HOME"));
    Mutex::new(rapidocr_core::RapidOcr::new(cfg).expect("OCR 引擎初始化失败"))
}

/// 拿引擎（锁守护）。OCR 天然串行：推理在 gpui 后台线程池里执行，
/// 锁竞争只发生在连按 Ctrl+O 的场景（排队即可）。
fn engine() -> anyhow::Result<std::sync::MutexGuard<'static, rapidocr_core::RapidOcr>> {
    static ENG: OnceLock<Mutex<rapidocr_core::RapidOcr>> = OnceLock::new();
    let e = ENG.get_or_init(init_engine);
    e.lock().map_err(|_| anyhow::anyhow!("OCR 引擎锁中毒"))
}

/// RGBA 像素 → OCR 文本。
/// `w`、`h` 是物理像素尺寸（来自 crop 裁剪后的输出）。
pub fn run_ocr(rgba: &[u8], w: u32, h: u32) -> anyhow::Result<String> {
    // RGBA → RGB（丢 alpha 道）
    let rgb: Vec<u8> = rgba
        .chunks(4)
        .flat_map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect();
    let img =
        image::RgbImage::from_raw(w, h, rgb).context("RGBA→RGB 转换失败")?;

    let out = engine()?.run_image(&img).context("OCR 推理失败")?;
    let text: String = out
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(text.trim().to_string())
}