//! # OCR: selection → PP-OCRv6 (rapidocr-core + ONNX Runtime) → text
//!
//! Engine loading: the overlay pre-warms on open ([`warmup`]) so the ~1s
//! init hides behind the user drawing their selection; the first-ever use
//! downloads the models (~31MB, ModelScope) inline. Models are cached in
//! `~/.local/share/shotori/ocr-models/`; when all files exist we skip the
//! sha256 re-verification entirely (corruption is caught by engine init
//! failing, which also cleans the cache).
//!
//! Failure policy: **initialization failures neither panic nor get cached** —
//! a clean Err is returned (the overlay prints the error and stays usable),
//! the next Ctrl+O retries automatically; a half-finished model cache is
//! wiped first (rapidocr-core writes straight to the target file, and a
//! truncated file never passes the sha256 check — without cleanup that
//! would brick OCR forever).
//!
//! Compiled under the `ocr` feature (on by default; `--no-default-features`
//! trims it).

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use anyhow::Context as _;
use rapidocr_core::model::PPOCRV6_SMALL;

/// Model cache directory (XDG-aware)
fn model_dir() -> anyhow::Result<PathBuf> {
    let base = if let Ok(d) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(d)
    } else {
        let home = std::env::var("HOME").context("HOME environment variable is not set")?;
        PathBuf::from(home).join(".local/share")
    };
    Ok(base.join("shotori/ocr-models"))
}

/// Download missing models + build the engine. Err = a recoverable failure
/// (retry next time); never panics.
///
/// Perf note: when all model files exist we skip `ensure_*` entirely — it
/// re-hashes all 31MB on every call, which is pure waste in the common
/// path (corruption is instead caught by engine init failing, which also
/// cleans the cache).
fn init_engine() -> anyhow::Result<Mutex<rapidocr_core::RapidOcr>> {
    let dir = model_dir()?;
    if assets_missing(&dir) > 0 {
        println!("[shotori] first OCR use: downloading PP-OCRv6 small models (~31MB from ModelScope)…");

        // The download runs on its own thread: reqwest::blocking cannot run
        // inside an async context (gpui's background executor is one, tokio
        // or not), so full isolation is the safest bet; the 8MB stack leaves
        // head room for ort's model loading
        let dl_dir = dir.clone();
        let dl = std::thread::Builder::new()
            .name("shotori-ocr-model-dl".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || rapidocr_core::model::ensure_ppocrv6_small_models(&dl_dir))
            .context("failed to spawn model download thread")?;
        if let Err(e) = dl
            .join()
            .map_err(|_| anyhow::anyhow!("model download thread panicked"))?
        {
            // Wipe half-finished downloads: a truncated/corrupt model file
            // fails the sha256 check every single time — without this
            // cleanup OCR is dead permanently (rapidocr-core's downloader
            // has no temp+rename)
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e.context("model download failed (cache cleaned, safe to retry)"));
        }
    }

    let cfg = PPOCRV6_SMALL.config(&dir);
    match rapidocr_core::RapidOcr::new(cfg) {
        Ok(eng) => Ok(Mutex::new(eng)),
        Err(e) => {
            // Session creation failing usually means a corrupt model file
            // (the sha256 path is skipped in the common path above) — clean
            // the cache so the next attempt re-downloads instead of bricking
            let _ = std::fs::remove_dir_all(&dir);
            Err(e.context("OCR engine init failed (model cache cleaned, safe to retry)"))
        }
    }
}

/// How many of the model-set files are absent from `dir`
fn assets_missing(dir: &std::path::Path) -> usize {
    PPOCRV6_SMALL
        .assets()
        .iter()
        .filter(|a| !dir.join(a.filename).exists())
        .count()
}

static ENG: OnceLock<Mutex<rapidocr_core::RapidOcr>> = OnceLock::new();
/// Initialization mutex: concurrent Ctrl+O triggers only one download
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the engine (lock-guarded). OCR is inherently serial: inference
/// runs on gpui's background thread pool, so lock contention only happens
/// when Ctrl+O is pressed in rapid succession (requests simply queue).
/// Initialization failures are not cached — OnceLock stays unset, so the
/// next call retries init.
fn engine() -> anyhow::Result<MutexGuard<'static, rapidocr_core::RapidOcr>> {
    // Fast path: already initialized
    if let Some(m) = ENG.get() {
        return m.lock().map_err(|_| anyhow::anyhow!("OCR engine lock poisoned"));
    }
    // Slow path: hold the init lock → double-check → initialize
    let _g = INIT_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("OCR init lock poisoned"))?;
    if let Some(m) = ENG.get() {
        return m.lock().map_err(|_| anyhow::anyhow!("OCR engine lock poisoned"));
    }
    let eng = init_engine()?;
    // Race backstop: a concurrent initializer may have set ENG first — ours
    // gets dropped, theirs is used (the two engines are equivalent)
    let _ = ENG.set(eng);
    ENG.get()
        .context("OCR engine vanished after init")?
        .lock()
        .map_err(|_| anyhow::anyhow!("OCR engine lock poisoned"))
}

/// Pre-initialize the engine in the background (no inference). The overlay
/// calls this on open so the ~1s init hides behind the user drawing their
/// selection. Only runs when models are already cached — a first-ever run
/// must not surprise the user with a 31MB download during a plain screenshot.
pub fn warmup() {
    if let Ok(dir) = model_dir()
        && assets_missing(&dir) == 0
    {
        // We only want the init side effect; the guard is released
        // immediately (that's the point of a warmup)
        drop(engine());
    }
}

/// RGBA pixels → OCR text.
/// `w`, `h` are physical pixel dimensions (output of the crop).
pub fn run_ocr(rgba: &[u8], w: u32, h: u32) -> anyhow::Result<String> {
    // RGBA → RGB (drop the alpha channel)
    let rgb: Vec<u8> = rgba
        .chunks(4)
        .flat_map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect();
    let img = image::RgbImage::from_raw(w, h, rgb).context("RGBA→RGB conversion failed")?;

    let out = engine()?.run_image(&img).context("OCR inference failed")?;
    let text: String = out
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(text.trim().to_string())
}