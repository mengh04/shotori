//! # Desktop notifications: fire-and-forget via a detached child process
//!
//! Why a child process: shotori calls `cx.quit()` right after saving / OCR.
//! A plain background thread would be killed mid-send when the process
//! exits; a detached child (`shotori --notify <summary> <body> [image] [saved path]`,
//! same pattern as the Linux clipboard daemon) outlives the parent and always
//! delivers. Failures are silent by design — a missing notification daemon
//! must never break a screenshot tool.
//!
//! Backends: freedesktop `org.freedesktop.Notifications` over D-Bus
//! (Linux) or WinRT toast (Windows; the PowerShell AppUserModelID is the
//! standard no-install trick — the toast then reports "Windows
//! PowerShell" as its source).
//!
//! Image previews: the freedesktop `image-path` hint / the toast image
//! with a `file://`-style local path (verified against noctalia).
//! Thumbnails are written to the cache dir and must outlive the
//! notification — cleaned up lazily (24h).

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The argv marker for the notify child (main.rs dispatches on this)
pub const NOTIFY_ARG: &str = "--notify";

/// Max thumbnail edge (px) — enough for any daemon's rendering, tiny file
const PREVIEW_MAX: u32 = 256;
/// Notification images older than this are removed on the next copy/save
const PREVIEW_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Queue a plain notification and return immediately.
pub fn send(summary: &str, body: &str) {
    spawn_child(summary, body, None, None);
}

/// Retain the full-resolution clipboard PNG so the notification can open it.
/// Cache failures must not turn a successful clipboard copy into an error.
pub fn copied(png: &[u8], w: u32, h: u32, rgba: &[u8]) {
    let preview = write_preview(w, h, rgba);
    let path = cache_dir().and_then(|dir| match write_clipboard_image(&dir, png) {
        Ok(path) => Some(path),
        Err(error) => {
            eprintln!("[shotori] could not cache copied screenshot: {error}");
            None
        }
    });
    spawn_child(
        "Screenshot copied",
        "The image is ready to paste.",
        preview.as_deref(),
        path.as_deref(),
    );
}

fn write_clipboard_image(dir: &std::path::Path, png: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write;
    cleanup_old_previews(dir);
    // Exclusive creation also handles simultaneous screenshot sessions.
    let mut file = tempfile::Builder::new()
        .prefix("clipboard-")
        .suffix(".png")
        .tempfile_in(dir)?;
    file.write_all(png)?;
    let (_, path) = file.keep().map_err(|error| error.error)?;
    Ok(path)
}

/// Saving has a separate action target: never open the temporary thumbnail.
pub fn saved(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let preview = write_preview(w, h, rgba);
    spawn_child(
        "Screenshot saved",
        &path.to_string_lossy(),
        preview.as_deref(),
        Some(&path),
    );
}

fn spawn_child(
    summary: &str,
    body: &str,
    image: Option<&std::path::Path>,
    open_path: Option<&std::path::Path>,
) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut cmd = Command::new(exe);
    cmd.arg(NOTIFY_ARG).arg(summary).arg(body);
    if image.is_some() || open_path.is_some() {
        cmd.arg(image.unwrap_or_else(|| std::path::Path::new("")));
    }
    if let Some(path) = open_path {
        cmd.arg(path);
    }
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // stderr inherited: real failures stay visible in the terminal
        .spawn();
    // Detached: dropping the handle leaves the child running; the parent
    // usually exits a moment later and the OS reaps it.
}

/// Child entry point: `shotori --notify <summary> <body> [image path] [saved path]`
/// → show → exit.
pub fn notify_main() -> i32 {
    let mut args = std::env::args_os().skip(2);
    let (Some(summary), Some(body)) = (args.next(), args.next()) else {
        eprintln!("[shotori] notify child: usage: --notify <summary> <body> [image] [saved path]");
        return 1;
    };
    let image = args.next().filter(|s| !s.is_empty());
    let open_path = args.next();
    let image_path = image.as_deref().map(std::path::Path::new);
    match show(
        &summary.to_string_lossy(),
        &body.to_string_lossy(),
        image_path,
        open_path.as_deref().map(std::path::Path::new),
    ) {
        Ok(()) => 0,
        Err(e) => {
            // No daemon on the bus, toast disabled, … — not fatal for
            // the caller (the action already succeeded), just report it.
            eprintln!("[shotori] notification not delivered: {e}");
            1
        }
    }
}

// ── Linux: org.freedesktop.Notifications over D-Bus ──────────────────

#[cfg(target_os = "linux")]
fn show(
    summary: &str,
    body: &str,
    image: Option<&std::path::Path>,
    open_path: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let mut n = notify_rust::Notification::new();
    n.appname("Shotori")
        .summary(summary)
        .body(&escape_markup(body))
        .timeout(notify_rust::Timeout::Milliseconds(3500));
    if let Some(path) = image {
        n.image_path(&format!("file://{}", path.display()));
    }
    if open_path.is_some() {
        // Clicking the notification body invokes the freedesktop
        // "default" action — the standard open gesture and the ONLY
        // control we ship. An explicit button would duplicate exactly
        // what a click already does.
        n.action("default", "View image")
            .timeout(notify_rust::Timeout::Milliseconds(10000));
    }
    let handle = n.show()?;
    if let Some(path) = open_path {
        handle.wait_for_action(|action| {
            if action == "default"
                && let Err(error) = open_image(path)
            {
                eprintln!("[shotori] could not open saved screenshot: {error}");
                send(
                    "Couldn’t open image",
                    "Open the saved file from your file manager.",
                );
            }
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_image(path: &std::path::Path) -> anyhow::Result<()> {
    let status = Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()?;
    anyhow::ensure!(status.success(), "image opener exited with {status}");
    Ok(())
}

// ── Windows: WinRT toast ─────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn show(
    summary: &str,
    body: &str,
    image: Option<&std::path::Path>,
    open_path: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    use tauri_winrt_notification::Toast;
    // Windows retains the saved path in the toast body. Its action backend
    // is separate from freedesktop actions and is not implemented yet.
    let _ = open_path;

    let mut toast = Toast::new(Toast::POWERSHELL_APP_ID)
        .title(summary)
        .text1(body)
        .duration(tauri_winrt_notification::Duration::Short);
    if let Some(path) = image {
        // Local absolute paths are accepted as toast image sources
        if path.is_absolute() {
            toast = toast.image(path, "screenshot preview");
        }
    }
    toast.show()?;
    Ok(())
}

/// Notification daemons may parse body markup, including OCR text and filenames.
#[cfg(target_os = "linux")]
fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ── Preview thumbnails ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn cache_dir() -> Option<PathBuf> {
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(d) => PathBuf::from(d),
        Err(_) => PathBuf::from(std::env::var("HOME").ok()?).join(".cache"),
    };
    let dir = base.join("shotori");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(target_os = "windows")]
fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("shotori").join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Downscale raw RGBA pixels to a cached PNG and return its path.
fn write_preview(w: u32, h: u32, rgba: &[u8]) -> Option<PathBuf> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())?;
    let thumb = image::imageops::thumbnail(&img, PREVIEW_MAX, PREVIEW_MAX);
    let dir = cache_dir()?;
    cleanup_old_previews(&dir);
    let path = dir.join(format!(
        "preview-{}.png",
        chrono::Local::now().format("%Y%m%d%H%M%S%3f")
    ));
    thumb.save(&path).ok()?;
    Some(path)
}

/// Best-effort removal of previews past their TTL. The file must outlive
/// the notification that references it — 24h is orders of magnitude more
/// than any notification timeout.
fn cleanup_old_previews(dir: &std::path::Path) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > PREVIEW_TTL);
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_preview = name.starts_with("preview-") || name.starts_with("clipboard-");
        if stale && is_preview {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn cache_cleanup_removes_only_expired_notification_images() {
        let dir = tempfile::tempdir().unwrap();
        let old =
            std::time::SystemTime::now() - super::PREVIEW_TTL - std::time::Duration::from_secs(60);
        for name in ["clipboard-old.png", "preview-old.png", "saved.png"] {
            let file = std::fs::File::create(dir.path().join(name)).unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(old))
                .unwrap();
        }
        let fresh = super::write_clipboard_image(dir.path(), b"new").unwrap();
        assert!(fresh.exists());
        assert!(dir.path().join("saved.png").exists());
        assert!(!dir.path().join("clipboard-old.png").exists());
        assert!(!dir.path().join("preview-old.png").exists());
    }

    #[test]
    fn clipboard_cache_retains_full_png_and_uses_unique_paths() {
        let dir = tempfile::tempdir().unwrap();
        let rgba = [30, 90, 180, 255].repeat(640 * 360);
        let png = crate::model::export::encode_png(640, 360, &rgba).unwrap();
        let first = super::write_clipboard_image(dir.path(), &png).unwrap();
        let second = super::write_clipboard_image(dir.path(), &png).unwrap();
        assert_ne!(first, second);
        let image = image::open(&first).unwrap().into_rgba8();
        assert_eq!(image.dimensions(), (640, 360));
        assert_eq!(image.into_raw(), rgba);
        assert_eq!(std::fs::read(second).unwrap(), png);
    }

    #[test]
    fn filenames_and_recognized_text_are_literal_notification_content() {
        assert_eq!(
            super::escape_markup("截图 <b>A&B</b>.png"),
            "截图 &lt;b&gt;A&amp;B&lt;/b&gt;.png"
        );
        assert_eq!(super::escape_markup("x > y\nA & B"), "x &gt; y\nA &amp; B");
    }
}
