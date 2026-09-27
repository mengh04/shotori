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
/// Previews older than this are removed on the next send
const PREVIEW_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Queue a plain notification and return immediately.
pub fn send(summary: &str, body: &str) {
    spawn_child(summary, body, None, None);
}

/// Queue a notification with a thumbnail rendered from the screenshot's
/// raw pixels. Falls back to a plain notification if the thumbnail cannot
/// be written (never let preview plumbing break the feedback).
pub fn send_with_preview(summary: &str, body: &str, w: u32, h: u32, rgba: &[u8]) {
    match write_preview(w, h, rgba) {
        Some(path) => spawn_child(summary, body, Some(&path), None),
        None => spawn_child(summary, body, None, None),
    }
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
        // Some notification centers render the default action as another button.
        // Register only the explicit action so there is exactly one opening control.
        n.action("open", "Open image")
            .timeout(notify_rust::Timeout::Milliseconds(10000));
    }
    let handle = n.show()?;
    if let Some(path) = open_path {
        handle.wait_for_action(|action| {
            if action == "open"
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
        let is_preview = entry.file_name().to_string_lossy().starts_with("preview-");
        if stale && is_preview {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn filenames_and_recognized_text_are_literal_notification_content() {
        assert_eq!(
            super::escape_markup("截图 <b>A&B</b>.png"),
            "截图 &lt;b&gt;A&amp;B&lt;/b&gt;.png"
        );
        assert_eq!(super::escape_markup("x > y\nA & B"), "x &gt; y\nA &amp; B");
    }
}
