//! # First-run OCR setup UI: confirm dialog → progress bar → cancel/retry
//!
//! Shown inside the overlay when Ctrl+O is pressed and no models are cached.
//! The download itself lives in [`crate::ocr::spawn_download`]; this module
//! only owns UI state and rendering. The overlay polls the shared progress
//! at ~80ms and transitions on completion.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use gpui_kit::*;

use crate::ocr::DownloadProgress;
use crate::theme;

gpui_kit::actions!([OcrSetupConfirm, OcrSetupCancel]);

/// Cropped pixels frozen at Ctrl+O time. The selection may change while the
/// dialog is open; the frozen screen cannot — so we keep the exact crop the
/// user OCR'd and run it once the download finishes.
pub(crate) struct Snapshot {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub(crate) enum Stage {
    /// Awaiting the user's decision
    Confirm,
    /// Downloading; the overlay polls `progress`
    Downloading,
    /// Download or post-download OCR failed; message shown
    Failed(String),
}

pub(crate) struct OcrSetup {
    pub stage: Stage,
    pub snapshot: Snapshot,
    pub progress: Arc<DownloadProgress>,
}

impl OcrSetup {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            stage: Stage::Confirm,
            snapshot,
            progress: Arc::new(DownloadProgress::default()),
        }
    }

    /// Only the active download may update this dialog or consume its snapshot.
    pub fn owns_download(&self, progress: &Arc<DownloadProgress>) -> bool {
        Arc::ptr_eq(&self.progress, progress)
            && !progress.cancel.load(Ordering::Relaxed)
            && matches!(self.stage, Stage::Downloading)
    }
}

/// The setup card: dim backdrop + centered card, rendered above everything
/// else in the overlay. Clicks inside are swallowed (no new selections).
pub(crate) fn setup_card(setup: &OcrSetup) -> impl IntoElement {
    div()
        .id("shotori-ocr-setup")
        .absolute()
        .size_full()
        .left_0()
        .top_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(theme::DIM))
        // clicks on the backdrop must not start a new selection underneath
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(card_body(setup))
}

fn card_body(setup: &OcrSetup) -> impl IntoElement {
    const CARD_W: f32 = 460.;

    let (title, body, buttons): (&str, Vec<AnyElement>, Vec<AnyElement>) = match &setup.stage {
        Stage::Confirm => (
            "OCR needs models",
            vec![text_line(&format!(
                "First OCR use: download PP-OCRv6 small models (~31 MB, from \
                 ModelScope)? Stored in {}\nOne-time only.",
                crate::ocr::model_dir_display()
            ))],
            vec![
                action_button("ocr-setup-go", "Download", OcrSetupConfirm),
                action_button("ocr-setup-no", "Cancel", OcrSetupCancel),
            ],
        ),
        Stage::Downloading => {
            let p = &setup.progress;
            let bytes = p.bytes.load(Ordering::Relaxed);
            let total = p.total.load(Ordering::Relaxed);
            let idx = p.file_idx.load(Ordering::Relaxed);
            let file = crate::ocr::asset_name(idx);
            let pct = if total > 0 {
                (bytes as f32 / total as f32).clamp(0., 1.)
            } else {
                0.
            };
            let mb = |v: u64| format!("{:.1} MB", v as f64 / 1e6);
            (
                "Downloading models…",
                vec![
                    text_line(&format!("{file} ({idx}/{})", p.file_count)),
                    progress_bar(pct, CARD_W - 64.),
                    text_line(&if total > 0 {
                        format!("{} / {}", mb(bytes), mb(total))
                    } else {
                        mb(bytes)
                    }),
                ],
                vec![action_button("ocr-setup-abort", "Cancel", OcrSetupCancel)],
            )
        }
        Stage::Failed(msg) => (
            "OCR setup failed",
            vec![text_line(&truncate(msg, 320))],
            vec![
                action_button("ocr-setup-retry", "Retry", OcrSetupConfirm),
                action_button("ocr-setup-close", "Close", OcrSetupCancel),
            ],
        ),
    };

    div()
        .w(px(CARD_W))
        .rounded(px(14.))
        .bg(rgba(theme::CHIP_BG))
        .border_1()
        .border_color(rgba(theme::ACCENT))
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(
            div()
                .text_size(px(15.))
                .text_color(rgba(theme::BTN_TEXT))
                .child(title),
        )
        .children(body)
        .child(div().flex().justify_end().gap_2().pt_2().children(buttons))
}

fn text_line(s: &str) -> AnyElement {
    div()
        .text_size(px(13.))
        .text_color(rgba(theme::HINT_TEXT))
        .line_height(px(19.))
        .child(s.replace('\n', " "))
        .into_any_element()
}

fn progress_bar(pct: f32, w: f32) -> AnyElement {
    div()
        .w(px(w))
        .h(px(8.))
        .rounded(px(4.))
        .bg(rgba(theme::BTN_HOVER_BG))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(px(w * pct))
                .rounded(px(4.))
                .bg(rgba(theme::ACCENT)),
        )
        .into_any_element()
}

fn action_button<A: Action + Clone + 'static>(
    id: &'static str,
    label: &'static str,
    action: A,
) -> AnyElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded(px(6.))
        .text_size(px(13.))
        .text_color(rgba(theme::BTN_TEXT))
        .border_1()
        .border_color(rgba(theme::PIN_BORDER))
        .hover(|s| s.bg(rgba(theme::BTN_HOVER_BG)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_click(move |_, window, cx| {
            window.dispatch_action(Box::new(action.clone()), cx);
        })
        .child(label)
        .into_any_element()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::{OcrSetup, Snapshot, Stage};
    use crate::ocr::DownloadProgress;
    use std::sync::{Arc, atomic::Ordering};

    fn setup() -> OcrSetup {
        OcrSetup::new(Snapshot {
            w: 1,
            h: 1,
            rgba: vec![0; 4],
        })
    }

    #[test]
    fn cancelled_download_cannot_update_reopened_dialog() {
        let mut old_setup = setup();
        old_setup.stage = Stage::Downloading;
        let old = old_setup.progress.clone();
        assert!(old_setup.owns_download(&old));
        old.cancel.store(true, Ordering::Relaxed);
        assert!(!old_setup.owns_download(&old));

        let mut reopened = setup();
        assert!(!reopened.owns_download(&old));
        reopened.stage = Stage::Downloading;
        assert!(!reopened.owns_download(&old));
        assert!(reopened.owns_download(&reopened.progress));
    }

    #[test]
    fn retry_rejects_previous_attempt_even_without_cancellation() {
        let mut setup = setup();
        setup.stage = Stage::Downloading;
        let old = setup.progress.clone();
        setup.stage = Stage::Failed("network error".into());
        assert!(!setup.owns_download(&old));
        setup.progress = Arc::new(DownloadProgress::default());
        setup.stage = Stage::Downloading;
        assert!(!setup.owns_download(&old));
        assert!(setup.owns_download(&setup.progress));
    }
}
