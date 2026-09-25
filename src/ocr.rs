//! # OCR：选区 → PP-OCRv6（rapidocr-core + ONNX Runtime）→ 文本
//!
//! 引擎懒加载：首次 Ctrl+O 下载模型（~31MB，ModelScope）+ 初始化 ~1-2s，
//! 之后常驻复用（<200ms/次）。模型缓存在 `~/.local/share/shotori/ocr-models/`。
//!
//! 失败策略：**初始化失败不 panic、不缓存失败**——干净返回 Err（覆盖层
//! 打印错误后保持可用），下次 Ctrl+O 自动重试；模型缓存若留有半成品
//! 会先清掉（rapidocr-core 直接写目标文件，截断文件过不了 sha256 校验，
//! 不清就会永久堵死）。

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

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

/// 下载缺失模型 + 建引擎。Err = 可恢复失败（下次重试），不 panic。
fn init_engine() -> anyhow::Result<Mutex<rapidocr_core::RapidOcr>> {
    let dir = model_dir()?;
    let missing = PPOCRV6_SMALL
        .assets()
        .iter()
        .filter(|a| !dir.join(a.filename).exists())
        .count();
    if missing > 0 {
        println!("[shotori] OCR 首次使用：下载 PP-OCRv6 small 模型（~31MB，来自 ModelScope）…");
    }

    // 下载在独立线程：reqwest::blocking 不能在异步上下文里跑
    // （gpui 后台执行器就是异步上下文，虽然它不是 tokio——彻底隔离最稳）；
    // 8MB 栈给 ort 的模型加载留余量
    let dl_dir = dir.clone();
    let dl = std::thread::Builder::new()
        .name("shotori-ocr-model-dl".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || rapidocr_core::model::ensure_ppocrv6_small_models(&dl_dir))
        .context("启动模型下载线程失败")?;
    if let Err(e) = dl
        .join()
        .map_err(|_| anyhow::anyhow!("模型下载线程 panic"))?
    {
        // 清掉半成品：截断/损坏的模型文件每次都过不了 sha256 校验，
        // 不清的话 OCR 从此永久失败（rapidocr-core 的下载无 temp+rename）
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.context("模型下载失败（缓存已清理，可重试）"));
    }

    let cfg = PPOCRV6_SMALL.config(&dir);
    let eng = rapidocr_core::RapidOcr::new(cfg).context("OCR 引擎初始化失败")?;
    Ok(Mutex::new(eng))
}

static ENG: OnceLock<Mutex<rapidocr_core::RapidOcr>> = OnceLock::new();
/// 初始化互斥：并发 Ctrl+O 只触发一次下载
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// 拿引擎（锁守护）。OCR 天然串行：推理在 gpui 后台线程池执行，
/// 锁竞争只发生在连按 Ctrl+O 的场景（排队即可）。
/// 初始化失败不缓存——OnceLock 不 set，下次调用重新走 init。
fn engine() -> anyhow::Result<MutexGuard<'static, rapidocr_core::RapidOcr>> {
    // 快路径：已就绪
    if let Some(m) = ENG.get() {
        return m.lock().map_err(|_| anyhow::anyhow!("OCR 引擎锁中毒"));
    }
    // 慢路径：持初始化锁 → 双检 → 初始化
    let _g = INIT_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("OCR 初始化锁中毒"))?;
    if let Some(m) = ENG.get() {
        return m.lock().map_err(|_| anyhow::anyhow!("OCR 引擎锁中毒"));
    }
    let eng = init_engine()?;
    // 竞态兜底：并发初始化时 set 可能失败（别人已放好）——那份 ours drop，
    // 用先到者的（两份引擎等价，丢一份无妨）
    let _ = ENG.set(eng);
    ENG.get()
        .context("OCR 引擎初始化后丢失")?
        .lock()
        .map_err(|_| anyhow::anyhow!("OCR 引擎锁中毒"))
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