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

        let debug_targeted = debug_targeted(&capture.output_name);
        if debug_targeted {
            spawn_debug_action(window, cx);
        }

        Self {
            focus_handle,
            frozen,
            capture,
            selection: debug_selection(debug_targeted),
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
        cx.quit();
    }

    /// Ctrl+O / toolbar [OCR]: crop the selection → OCR → text to clipboard.
    #[cfg(feature = "ocr")]
    fn ocr_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (w, h, rgba) = match self.crop(self.selection_or_full(window), window) {
            Some(x) => x,
            None => {
                println!("[shotori] empty selection, ignoring");
                return;
            }
        };
        let window_handle = window.window_handle();
        cx.spawn(async move |_, cx| {
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
                    cx.quit();
                }
                Err(e) => {
                    eprintln!("[shotori] OCR failed: {e:#}");
                }
            });
        })
        .detach();
    }
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sel = self.selection.bounds();
        let ws = window.bounds().size; // window logical size (= output logical size)

        div()
            .id("shotori-overlay")
            .key_context("ShotoriOverlay")
            .size_full()
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &CopySelection, window, cx| {
                this.copy_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveSelection, window, cx| {
                this.save_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OcrSelection, window, cx| {
                #[cfg(feature = "ocr")]
                this.ocr_selection(window, cx);
                #[cfg(not(feature = "ocr"))]
                {
                    let _ = (this, window, cx);
                }
            }))
            // Two-stage Esc: while dragging = abandon this drag only (swallow
            // the action, no bubbling); after release (Idle/Selected) =
            // unhandled, bubbles to main.rs's global backstop → exit.
            // Two-stage Esc (handled in place, no reliance on bubbling):
            // measured: dispatch_action inside a gpui window stops at the
            // focus path and never reaches App::on_action — exiting must
            // happen here
            .on_action(cx.listener(|this, _: &QuitOverlay, _, cx| {
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
            // ③ Selection border + size label (live while dragging)
            .children(sel.map(selection_chrome))
            // ④ Toolbar: appears only after release (no flicker while dragging)
            .children(
                if let Selection::Selected { bounds } = self.selection {
                    Some(selection_toolbar(bounds, ws))
                } else {
                    None
                },
            )
            // ⑤ Bottom hint bar
            .child(hint_bar())
    }
}

// ── Debug backdoors (entry points for automated e2e; normal launches are
// unaffected) ─────────────────────────────────────────────────────────

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

/// SHOTORI_DEBUG_ACTION=copy|quit|ocr: fire the action automatically after
/// 1.5s — the only entry point for headless e2e (the virtual pointer is dead
/// on niri, see ROADMAP). quit/ocr go through the real dispatch_action pipeline
fn spawn_debug_action(window: &mut Window, cx: &mut Context<Overlay>) {
    let Some(action) = std::env::var("SHOTORI_DEBUG_ACTION")
        .ok()
        .filter(|a| a == "copy" || a == "quit" || (a == "ocr" && cfg!(feature = "ocr")))
    else {
        return;
    };
    let win = window.window_handle();
    cx.spawn(async move |_, cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(1500))
            .await;
        let _ = win.update(cx, |_, window, cx| {
            let action: Box<dyn gpui_kit::Action> = match action.as_str() {
                "copy" => Box::new(CopySelection),
                #[cfg(feature = "ocr")]
                "ocr" => Box::new(OcrSelection),
                _ => Box::new(QuitOverlay),
            };
            window.dispatch_action(action, cx);
        });
    })
    .detach();
}