//! # Screenshot overlay: assembly layer for frozen-screen + selection interaction
//!
//! Flow: frozen frame as the base (img) → drag a selection (the selection
//! "sees through", everything around it dims) → Enter copies to clipboard /
//! Ctrl+S saves PNG / Ctrl+O OCR / Esc exits.
//!
//! Division of labor: pure logic lives in [`crate::selection`] (state
//! machine) and [`crate::export`] (crop/encode), visuals in [`crate::hud`]
//! and [`crate::toolbar`] — this file only does gpui assembly:
//! window, events → state-machine calls, state → rendering.

use std::sync::Arc;

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;

use crate::capture::Capture;
use crate::hud::{dim_strips, hint_bar, selection_chrome};
use crate::image_util;
use crate::selection::Selection;
use crate::toolbar::selection_toolbar;

gpui_kit::actions!([QuitOverlay, CopySelection, SaveSelection, OcrSelection]);

pub struct Overlay {
    focus_handle: FocusHandle,
    /// The frozen screen image (displayed by the img element)
    frozen: Arc<RenderImage>,
    /// Raw pixels (for cropping)
    capture: Capture,
    selection: Selection,
    /// First-run OCR setup (confirm → download progress), open while active
    #[cfg(feature = "ocr")]
    ocr_setup: Option<Box<crate::ocr_setup::OcrSetup>>,
    /// OCR inference in flight → show the busy badge (spinner)
    #[cfg(feature = "ocr")]
    ocr_busy: bool,
}

impl Overlay {
    pub fn new(
        capture: Capture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let frozen = image_util::rgba_to_render_image(
            capture.rgba.clone(),
            capture.width,
            capture.height,
        );

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        // Pre-warm the OCR engine while the user is still drawing their
        // selection: the init cost hides behind interaction time.
        // warmup() skips first-ever runs (no surprise 31MB download).
        #[cfg(feature = "ocr")]
        std::thread::Builder::new()
            .name("shotori-ocr-warmup".into())
            .spawn(crate::ocr::warmup)
            .ok();

        let debug_targeted = debug_targeted(&capture.output_name);
        if debug_targeted {
            spawn_debug_action(window, cx);
        }

        Self {
            focus_handle,
            frozen,
            capture,
            selection: debug_selection(debug_targeted),
            #[cfg(feature = "ocr")]
            ocr_setup: None,
            #[cfg(feature = "ocr")]
            ocr_busy: false,
        }
    }

    /// WindowOptions for the overlay window (anchored on all four edges +
    /// Exclusive keyboard). display_id: pin to the output the capture came
    /// from (without it the compositor picks — multi-monitor = lottery)
    pub fn window_options(display_id: Option<DisplayId>) -> WindowOptions {
        WindowOptions {
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: true,
            display_id,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: "shotori-overlay".into(),
                layer: Layer::Overlay,
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                exclusive_zone: Some(px(-1.)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// This window's "captured physical pixels ÷ logical pixels" ratio.
    /// Deliberately not window.scale_factor(): under mixed-DPI multi-monitor
    /// setups gpui reports another output's scale (measured: window pinned
    /// to HDMI renders at 1.0 while scale_factor() reports DP-2's 1.5).
    /// Computing it ourselves is naturally consistent with rendering and
    /// immune to the misreport.
    fn render_scale(&self, window: &Window) -> f32 {
        let ws = window.bounds().size;
        if f32::from(ws.width) > 0. {
            self.capture.width as f32 / f32::from(ws.width)
        } else {
            window.scale_factor()
        }
    }

    /// Logical selection → physical-pixel crop; None for an empty selection
    fn crop(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> Option<(u32, u32, Vec<u8>)> {
        crate::export::crop(
            &self.capture.rgba,
            self.capture.width,
            self.capture.height,
            bounds,
            self.render_scale(window),
        )
    }

    /// The selection to act on; no selection = the whole window (= the whole
    /// output), the default behavior of every screenshot tool
    fn selection_or_full(&self, window: &mut Window) -> Bounds<Pixels> {
        self.selection.bounds().unwrap_or_else(|| Bounds {
            origin: Point::default(),
            size: window.bounds().size,
        })
    }

    /// Enter / Ctrl+C / toolbar [Copy]: crop → PNG → clipboard (resident
    /// daemon) → exit. The primary exit of daily use.
    fn copy_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (w, h, rgba) = match self.crop(self.selection_or_full(window), window) {
            Some(x) => x,
            None => {
                println!("[shotori] empty selection, ignoring");
                return;
            }
        };
        let png = match crate::export::encode_png(w, h, &rgba) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[shotori] PNG encoding failed: {e:#}");
                return;
            }
        };
        if let Err(e) = crate::clipboard::copy_image(png) {
            // Stay in the overlay on failure: the user can still Ctrl+S
            eprintln!("[shotori] copy failed: {e:#}");
            return;
        }
        println!(
            "[shotori] copied {w}x{h} (from {}) to clipboard",
            self.capture.output_name
        );
        crate::notify::send_with_preview(
            "Shotori",
            &format!("Copied {w}×{h} → clipboard"),
            w,
            h,
            &rgba,
        );
        cx.quit();
    }

    /// Ctrl+S / toolbar [Save]: crop → PNG → disk. Stays in the overlay on
    /// failure (retry / Esc to exit)
    fn save_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (w, h, out) = match self.crop(self.selection_or_full(window), window) {
            Some(x) => x,
            None => {
                println!("[shotori] empty selection, ignoring");
                return;
            }
        };

        let path = match crate::export::next_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[shotori] save path failed: {e:#}");
                return;
            }
        };
        if let Err(e) = crate::export::save_png(&path, w, h, &out) {
            eprintln!("[shotori] save failed: {e:#}");
            return;
        }

        println!(
            "[shotori] saved {w}x{h} (from {}) → {}",
            self.capture.output_name,
            path.display()
        );
        // The path is the thing users actually need — stdout is lost when
        // launched from a keybinding, so the notification is the feedback
        crate::notify::send_with_preview(
            "Shotori",
            &format!("Saved {w}×{h} → {}", path.display()),
            w,
            h,
            &out,
        );
        cx.quit();
    }

    /// Ctrl+O / toolbar [OCR]. With cached models this runs immediately; on
    /// the very first use it opens the setup dialog (confirm → progress →
    /// cancel) instead — [`crate::ocr_setup`].
    #[cfg(feature = "ocr")]
    fn ocr_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ocr_setup.is_some() || self.ocr_busy {
            return; // dialog open or an OCR already running
        }
        let Some((w, h, rgba)) = self.crop(self.selection_or_full(window), window) else {
            println!("[shotori] empty selection, ignoring");
            return;
        };
        let snapshot = crate::ocr_setup::Snapshot { w, h, rgba };

        // SHOTORI_DEBUG_ACTION=ocr keeps the old headless inline path for
        // e2e (no clicking available); real users get the dialog
        let headless = std::env::var("SHOTORI_DEBUG_ACTION").as_deref() == Ok("ocr");
        if crate::ocr::models_missing() && !headless {
            self.ocr_setup = Some(Box::new(crate::ocr_setup::OcrSetup::new(snapshot)));
            cx.notify();
        } else {
            self.spawn_ocr(snapshot, window, cx);
        }
    }

    /// Run OCR on a snapshot (the Ctrl+O fast path, and the tail of the
    /// first-run setup once models are in): background inference → text to
    /// clipboard → quit. Free of `self` so both the action handler and the
    /// download poll loop can call it.
    #[cfg(feature = "ocr")]
    fn spawn_ocr(
        &mut self,
        snapshot: crate::ocr_setup::Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let crate::ocr_setup::Snapshot { w, h, rgba } = snapshot;
        let window_handle = window.window_handle();
        let entity = cx.entity();
        cx.spawn(async move |_, cx| {
            ocr_to_clipboard(w, h, rgba, window_handle, entity, cx).await;
        })
        .detach();
    }

    /// Setup dialog [Download]/[Retry]: switch to Downloading, spawn the
    /// download thread and start the poll loop that drives the progress bar
    /// and the completion transition. The loop holds an Arc clone of the
    /// progress and the entity handle — no shared state mutation races with
    /// the UI thread.
    #[cfg(feature = "ocr")]
    fn ocr_setup_confirm(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // Retry after a failure needs a fresh progress arc
        let fresh = std::sync::Arc::new(crate::ocr::DownloadProgress::default());
        if let Some(setup) = self.ocr_setup.as_mut() {
            if !matches!(setup.stage, crate::ocr_setup::Stage::Confirm)
                && !matches!(setup.stage, crate::ocr_setup::Stage::Failed(_))
            {
                return; // already downloading
            }
            setup.progress = fresh;
            setup.shown = (0, 0, 0);
            setup.stage = crate::ocr_setup::Stage::Downloading;
        } else {
            return; // nothing to confirm (no dialog open)
        }
        let Some(setup) = self.ocr_setup.as_ref() else {
            return;
        };
        let progress = setup.progress.clone();
        crate::ocr::spawn_download(progress.clone());

        let entity = cx.entity();
        let window_handle = _window.window_handle();

        cx.spawn(async move |_, cx| {
            let mut last = (0u64, 0u64, 0u8);
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(80))
                    .await;
                if !progress.is_running() {
                    break;
                }
                let readout = (
                    progress.bytes.load(std::sync::atomic::Ordering::Relaxed),
                    progress.total.load(std::sync::atomic::Ordering::Relaxed),
                    progress.file_idx.load(std::sync::atomic::Ordering::Relaxed),
                );
                if readout != last {
                    last = readout;
                    // re-render: the card reads the atomics at draw time
                    entity.update(cx, |_, cx| cx.notify());
                }
            }
            if progress.finished_ok() {
                println!("[shotori] model download complete");
                // hand the frozen snapshot over to OCR
                let snap = entity.update(cx, |this, _| {
                    this.ocr_setup.take().map(|s| s.snapshot)
                });
                if let Some(crate::ocr_setup::Snapshot { w, h, rgba }) = snap {
                    ocr_to_clipboard(w, h, rgba, window_handle, entity, cx).await;
                }
            } else {
                let err = progress.error();
                entity.update(cx, |this, cx| {
                    if let Some(setup) = this.ocr_setup.as_mut() {
                        setup.stage = crate::ocr_setup::Stage::Failed(err);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Setup dialog [Cancel]/[Close] and Esc: abort the download, clean up,
    /// back to plain selection mode.
    #[cfg(feature = "ocr")]
    fn ocr_setup_cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(setup) = self.ocr_setup.as_ref() {
            setup
                .progress
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.ocr_setup = None;
        cx.notify();
    }
}

/// Background OCR → text to clipboard → quit. Shared by the Ctrl+O action
/// path and the post-download handover. Flips `ocr_busy` on the view for the
/// duration (spinner badge).
#[cfg(feature = "ocr")]
async fn ocr_to_clipboard(
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    window_handle: gpui_kit::AnyWindowHandle,
    entity: gpui_kit::Entity<Overlay>,
    cx: &mut gpui_kit::AsyncApp,
) {
    entity.update(cx, |this, cx| {
        this.ocr_busy = true;
        cx.notify();
    });
    let result: anyhow::Result<String> = cx
        .background_executor()
        .spawn(async move { crate::ocr::run_ocr(&rgba, w, h) })
        .await;
    let text = result.and_then(|t| {
        if t.is_empty() {
            Err(anyhow::anyhow!("OCR found no text"))
        } else {
            Ok(t)
        }
    });
    let _ = window_handle.update(cx, |_, _, cx| match &text {
        Ok(t) => {
            if let Err(e) = crate::clipboard::copy_text(t.clone()) {
                eprintln!("[shotori] OCR copy failed: {e:#}");
                return;
            }
            let lines = t.lines().count();
            let preview: String = t
                .replace('\n', " ")
                .chars()
                .filter(|c| !c.is_control())
                .take(60)
                .collect();
            println!(
                "[shotori] OCR done {w}x{h} → {lines} line(s) → clipboard (preview: {preview})"
            );
            crate::notify::send(
                "Shotori OCR",
                &format!("{lines} lines → clipboard\n{preview}"),
            );
            cx.quit();
        }
        Err(e) => {
            eprintln!("[shotori] OCR failed: {e:#}");
            // The overlay stays open with no in-UI error display yet —
            // without this notification a keybinding user sees nothing
            let msg: String = e.to_string().chars().take(200).collect();
            crate::notify::send("Shotori OCR failed", &msg);
        }
    });
    // Clear the busy flag on both paths (failure stays on-screen; a stuck
    // spinner would spin forever otherwise)
    entity.update(cx, |this, cx| {
        this.ocr_busy = false;
        cx.notify();
    });
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Round the selection to whole pixels ONCE and share the result
        // between the dim strips and the selection chrome. With fractional
        // bounds (remote mice produce them), the border div and the dim
        // divs round independently inside gpui; for certain fractional
        // phases they diverge and leave a 1px row covered by NEITHER →
        // raw content bleeds through as a spurious bright line (reported
        // as a white line under the selection on light backgrounds).
        let sel = self.selection.bounds().map(round_px);
        let ws = window.bounds().size; // window logical size (= output logical size)

        // Bind the base chain, then attach feature-gated handlers via
        // shadowing — cfg attributes are illegal in the middle of a method
        // chain (see ROADMAP v0.6.2 pitfall notes)
        let base = div()
            .id("shotori-overlay")
            .key_context("ShotoriOverlay")
            .size_full()
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &CopySelection, window, cx| {
                #[cfg(feature = "ocr")]
                if this.ocr_setup.is_some() {
                    return; // setup dialog is modal
                }
                this.copy_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveSelection, window, cx| {
                #[cfg(feature = "ocr")]
                if this.ocr_setup.is_some() {
                    return; // setup dialog is modal
                }
                this.save_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OcrSelection, window, cx| {
                #[cfg(feature = "ocr")]
                this.ocr_selection(window, cx);
                #[cfg(not(feature = "ocr"))]
                {
                    let _ = (this, window, cx);
                }
            }));
        #[cfg(feature = "ocr")]
        let base = base
            .on_action(
                cx.listener(|this, _: &crate::ocr_setup::OcrSetupConfirm, window, cx| {
                    this.ocr_setup_confirm(window, cx);
                }),
            )
            .on_action(cx.listener(
                |this, _: &crate::ocr_setup::OcrSetupCancel, _, cx| {
                    this.ocr_setup_cancel(cx);
                },
            ));

        // ⑥ First-run OCR setup dialog (confirm / progress), topmost.
        // Computed before the chain — cfg attrs are illegal mid-chain
        #[cfg(feature = "ocr")]
        let setup_el: Option<AnyElement> = self
            .ocr_setup
            .as_ref()
            .map(|s| crate::ocr_setup::setup_card(s).into_any_element());
        #[cfg(not(feature = "ocr"))]
        let setup_el: Option<AnyElement> = None;

        // OCR-in-flight spinner badge, centered on the selection
        #[cfg(feature = "ocr")]
        let busy_el: Option<AnyElement> = self
            .ocr_busy
            .then(|| crate::hud::ocr_busy_badge(sel, ws).into_any_element());
        #[cfg(not(feature = "ocr"))]
        let busy_el: Option<AnyElement> = None;

        base
            // Two-stage Esc (handled in place, no reliance on bubbling):
            // measured: dispatch_action inside a gpui window stops at the
            // focus path and never reaches App::on_action — exiting must
            // happen here. With the setup dialog open, Esc cancels the
            // dialog instead (aborting any download).
            .on_action(cx.listener(|this, _: &QuitOverlay, _, cx| {
                #[cfg(feature = "ocr")]
                if this.ocr_setup.is_some() {
                    this.ocr_setup_cancel(cx);
                    cx.stop_propagation();
                    return;
                }
                if this.selection.is_dragging() {
                    this.selection.cancel_drag(); // stage one: abandon this drag
                } else {
                    cx.quit(); // stage two: exit
                }
                cx.stop_propagation();
                cx.notify();
            }))
            // ── Selection interaction (events → state machine) ─────────
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    #[cfg(feature = "ocr")]
                    if this.ocr_setup.is_some() {
                        return; // modal dialog: no new selections
                    }
                    this.selection.begin(ev.position);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.selection.drag_to(ev.position) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    this.selection.end(ev.position);
                    cx.notify();
                }),
            )
            // ── Layer stack (bottom to top) ─────────────────────────────
            // ① The frozen screen image (opaque, filling the window)
            .child(img(self.frozen.clone()).size_full())
            // ② Dim layer: no selection = whole screen; with a selection =
            // four strips around it (the selection "sees through")
            .children(dim_strips(sel, ws))
            // ③ Selection border + size label (live while dragging);
            // Vec: the label is a separate window-anchored element (see
            // selection_chrome)
            .children(sel.map(selection_chrome).unwrap_or_default())
            // ④ Toolbar: appears only after release (no flicker while dragging)
            .children(
                if let Selection::Selected { bounds } = self.selection {
                    Some(selection_toolbar(round_px(bounds), ws))
                } else {
                    None
                },
            )
            // ⑤ Bottom hint bar
            .child(hint_bar())
            // ⑥ OCR busy badge (spinner on the selection)
            .children(busy_el)
            // ⑦ First-run OCR setup dialog (confirm / progress), topmost
            .children(setup_el)
    }
}

// ── Debug backdoors (entry points for automated e2e; normal launches are
// unaffected) ─────────────────────────────────────────────────────────

/// Snap a bounds to whole pixels for DISPLAY (dim strips, chrome,
/// toolbar). Edges are rounded independently (round(origin)+round(size)
/// can drift by 1px from round(origin+size)). Cropping keeps its own
/// physical-pixel rounding — this is purely a rendering concern.
fn round_px(b: Bounds<Pixels>) -> Bounds<Pixels> {
    let l = f32::from(b.left()).round();
    let t = f32::from(b.top()).round();
    let r = f32::from(b.right()).round();
    let btm = f32::from(b.bottom()).round();
    Bounds {
        origin: point(px(l), px(t)),
        size: size(px(r - l), px(btm - t)),
    }
}

/// Does the backdoor target this overlay? SHOTORI_DEBUG_TARGET=<output name>
/// (with multiple overlays all running this code, enabling all of them makes
/// them fight each other); unset = enabled everywhere
fn debug_targeted(output_name: &str) -> bool {
    std::env::var("SHOTORI_DEBUG_TARGET")
        .map(|t| t == output_name)
        .unwrap_or(true)
}

/// SHOTORI_DEBUG_SELECTION=x,y,w,h: inject a ready-made selection
fn debug_selection(targeted: bool) -> Selection {
    if !targeted {
        return Selection::Idle;
    }
    std::env::var("SHOTORI_DEBUG_SELECTION")
        .ok()
        .and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|n| n.trim().parse().ok()).collect();
            (v.len() == 4).then(|| Selection::Selected {
                bounds: Bounds {
                    origin: point(px(v[0]), px(v[1])),
                    size: size(px(v[2]), px(v[3])),
                },
            })
        })
        .unwrap_or(Selection::Idle)
}

/// SHOTORI_DEBUG_ACTION=copy|quit|ocr|ocrsetup: fire the action(s)
/// automatically after 1.5s — the only entry point for headless e2e (the
/// virtual pointer is dead on niri, see ROADMAP). quit/ocr go through the
/// real dispatch_action pipeline. "ocrsetup" drives the full first-run flow:
/// OcrSelection at 1.5s (opens the dialog since models are missing), then
/// OcrSetupConfirm at 6s (starts the download) — exercise the whole UI path.
fn spawn_debug_action(window: &mut Window, cx: &mut Context<Overlay>) {
    let Some(action) = std::env::var("SHOTORI_DEBUG_ACTION")
        .ok()
        .filter(|a| {
            a == "copy" || a == "quit" || a == "save"
                || (a == "ocr" && cfg!(feature = "ocr"))
                || (a == "ocrsetup" && cfg!(feature = "ocr"))
        })
    else {
        return;
    };
    let win = window.window_handle();
    cx.spawn(async move |_, cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(1500))
            .await;
        if action == "ocrsetup" {
            // Only reachable with the ocr feature (see the filter above)
            #[cfg(feature = "ocr")]
            {
                // phase 1: open the setup dialog (models must be missing)
                let _ = win.update(cx, |_, window, cx| {
                    window.dispatch_action(
                        Box::new(crate::overlay::OcrSelection),
                        cx,
                    );
                });
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(4500))
                    .await;
                let _ = win.update(cx, |_, window, cx| {
                    window.dispatch_action(
                        Box::new(crate::ocr_setup::OcrSetupConfirm),
                        cx,
                    );
                });
            }
            return;
        }
        let _ = win.update(cx, |_, window, cx| {
            let action: Box<dyn gpui_kit::Action> = match action.as_str() {
                "copy" => Box::new(CopySelection),
                "save" => Box::new(SaveSelection),
                #[cfg(feature = "ocr")]
                "ocr" => Box::new(OcrSelection),
                _ => Box::new(QuitOverlay),
            };
            window.dispatch_action(action, cx);
        });
    })
    .detach();
}