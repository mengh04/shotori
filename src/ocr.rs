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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
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

/// Whether any model files are missing (cheap existence check; used to
/// decide between "run OCR" and "show the setup dialog")
pub fn models_missing() -> bool {
    model_dir().map(|d| assets_missing(&d) > 0).unwrap_or(true)
}

/// Human-readable model cache path (shown in the setup dialog)
pub fn model_dir_display() -> String {
    model_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".into())
}

/// Filename of the 1-based asset `idx` (progress readout)
pub fn asset_name(idx: u8) -> &'static str {
    PPOCRV6_SMALL
        .assets()
        .get(idx as usize - 1)
        .map(|a| a.filename)
        .unwrap_or("?")
}

// ── First-run download with progress + cancellation ───────────────────

/// Shared state between the download thread and the setup UI.
/// All fields are lock-free; the UI polls at ~80ms.
pub struct DownloadProgress {
    /// Set by the UI to abort; checked between chunks
    pub cancel: AtomicBool,
    /// Bytes written so far (across all files this session)
    pub bytes: AtomicU64,
    /// Total bytes (Σ content-length; grows as each file starts)
    pub total: AtomicU64,
    /// 1-based index of the file currently downloading
    pub file_idx: AtomicU8,
    /// Number of files in the set
    pub file_count: u8,
    status: AtomicU8, // 0 running, 1 ok, 2 failed
    error: Mutex<String>,
}

impl Default for DownloadProgress {
    fn default() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            bytes: AtomicU64::new(0),
            total: AtomicU64::new(0),
            file_idx: AtomicU8::new(0),
            file_count: PPOCRV6_SMALL.assets().len() as u8,
            status: AtomicU8::new(0),
            error: Mutex::new(String::new()),
        }
    }
}

impl DownloadProgress {
    pub fn is_running(&self) -> bool {
        self.status.load(Ordering::Relaxed) == 0
    }
    pub fn finished_ok(&self) -> bool {
        self.status.load(Ordering::Relaxed) == 1
    }
    pub fn error(&self) -> String {
        self.error
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

/// Spawn the model download on its own thread (reqwest::blocking cannot run
/// in async contexts). Downloads to `*.part` temp files and renames on
/// success (atomic — a cancelled or crashed download never leaves a
/// half-written model behind), verifying sha256 after each file.
pub fn spawn_download(progress: std::sync::Arc<DownloadProgress>) {
    // The thread gets its own Arc; the caller's (and the UI's) stays valid
    // for cancellation even if the thread fails to spawn
    let thread_progress = std::sync::Arc::clone(&progress);
    let spawn = std::thread::Builder::new()
        .name("shotori-ocr-download".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let result = download_models(&thread_progress);
            match result {
                Ok(()) => thread_progress.status.store(1, Ordering::Relaxed),
                Err(e) => {
                    if let Ok(mut err) = thread_progress.error.lock() {
                        *err = format!("{e:#}");
                    }
                    thread_progress.status.store(2, Ordering::Relaxed);
                }
            }
        });
    if let Err(e) = spawn {
        // Thread spawn failure must not leave the UI waiting forever
        if let Ok(mut err) = progress.error.lock() {
            *err = format!("failed to spawn download thread: {e:#}");
        }
        progress.status.store(2, Ordering::Relaxed);
    }
}

fn download_models(progress: &DownloadProgress) -> anyhow::Result<()> {
    use std::io::{Read as _, Write as _};

    let dir = model_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    let client = reqwest::blocking::Client::builder()
        .user_agent("shotori")
        .build()
        .context("building HTTP client")?;

    for (i, asset) in PPOCRV6_SMALL.assets().iter().enumerate() {
        if progress.cancel.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        let dest = dir.join(asset.filename);
        if dest.exists() {
            continue; // already have this one (e.g. after a partial retry)
        }
        progress.file_idx.store(i as u8 + 1, Ordering::Relaxed);

        let resp = client
            .get(asset.url)
            .send()
            .with_context(|| format!("fetching {}", asset.url))?
            .error_for_status()
            .with_context(|| format!("bad status for {}", asset.url))?;
        if let Some(len) = resp.content_length() {
            progress.total.fetch_add(len, Ordering::Relaxed);
        }

        let tmp = dir.join(format!("{}.part", asset.filename));
        let mut file = std::fs::File::create(&tmp)
            .with_context(|| format!("creating {}", tmp.display()))?;
        let mut reader = resp;
        let mut buf = [0u8; 64 * 1024];
        loop {
            if progress.cancel.load(Ordering::Relaxed) {
                let _ = std::fs::remove_file(&tmp);
                anyhow::bail!("cancelled");
            }
            let n = reader.read(&mut buf).context("reading download chunk")?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).context("writing download chunk")?;
            progress.bytes.fetch_add(n as u64, Ordering::Relaxed);
        }
        drop(file);

        if let Some(expected) = asset.sha256
            && sha256_file(&tmp)? != expected
        {
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!("sha256 mismatch for {}", asset.filename);
        }
        std::fs::rename(&tmp, &dest)
            .with_context(|| format!("finalizing {}", dest.display()))?;
    }
    Ok(())
}

fn sha256_file(path: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    // Manual hex: sha2's output type lost its LowerHex impl in newer
    // patch releases (hybrid-array migration) — don't depend on it
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
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