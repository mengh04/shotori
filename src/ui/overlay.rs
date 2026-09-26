//! # Screenshot overlay: assembly layer for frozen-screen + selection interaction
//!
//! Flow: frozen frame as the base (img) → drag a selection (the selection
//! "sees through", everything around it dims) → Enter copies to clipboard /
//! Ctrl+S saves PNG / Ctrl+O OCR / Esc exits.
//!
//! Division of labor: pure logic lives in [`crate::model::selection`] (state
//! machine) and [`crate::model::export`] (crop/encode), visuals in [`crate::ui::hud`]
//! and [`crate::ui::toolbar`] — this file only does gpui assembly:
//! window, events → state-machine calls, state → rendering.

use std::sync::Arc;

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;

use crate::actions::{CopySelection, OcrSelection, QuitOverlay, SaveSelection};
use crate::model::selection::Selection;
use crate::platform::capture::Capture;
use crate::ui::hud::{hint_bar, hover_outline, selection_backdrop, selection_label};
use crate::ui::image_util;
use crate::ui::toolbar::selection_toolbar;

pub struct Overlay {
    focus_handle: FocusHandle,
    /// The frozen screen image (displayed by the img element)
    frozen: Arc<RenderImage>,
    /// Raw pixels (for cropping)
    capture: Arc<Capture>,
    session: Entity<crate::model::session::ScreenshotSession>,
    _subscriptions: Vec<Subscription>,
    /// First-run OCR setup (confirm → download progress), open while active
    ocr_setup: Option<Box<crate::ui::ocr_setup::OcrSetup>>,
    setup_focus: crate::ui::ocr_setup::SetupFocus,
    /// OCR inference in flight → show the busy badge (spinner)
    ocr_busy: bool,
}

impl Overlay {
    pub fn new(
        capture: Arc<Capture>,
        session: Entity<crate::model::session::ScreenshotSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let frozen =
            image_util::rgba_to_render_image(capture.rgba.clone(), capture.width, capture.height);

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        let debug_targeted = crate::ui::e2e::debug_targeted(&capture.output_name);
        if debug_targeted {
            crate::ui::e2e::spawn_debug_action(window, cx);
        }

        session.update(cx, |session, cx| {
            session.set_size(&capture.output_name, window.bounds().size);
            if let Selection::Selected { bounds } = crate::ui::e2e::debug_selection(debug_targeted)
            {
                session.begin(&capture.output_name, bounds.origin);
                session.end(&capture.output_name, bounds.bottom_right());
            }
            cx.notify();
        });
        let subscriptions = vec![
            cx.observe_in(&session, window, |_, _, _, cx| cx.notify()),
            cx.observe_window_bounds(window, |this, window, cx| {
                this.session.update(cx, |session, cx| {
                    if session.set_size(&this.capture.output_name, window.bounds().size) {
                        cx.notify();
                    }
                });
            }),
        ];
        Self {
            focus_handle,
            frozen,
            capture,
            session,
            _subscriptions: subscriptions,
            ocr_setup: None,
            setup_focus: crate::ui::ocr_setup::SetupFocus::new(cx),
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

    fn crop(&self, cx: &App) -> Option<(u32, u32, Vec<u8>)> {
        self.session.read(cx).crop(&self.capture.output_name)
    }

    /// Enter / Ctrl+C / toolbar [Copy]: crop → PNG → clipboard (resident
    /// daemon) → exit. The primary exit of daily use.
    fn copy_selection(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let (w, h, rgba) = match self.crop(cx) {
            Some(x) => x,
            None => {
                println!("[shotori] empty selection, ignoring");
                return;
            }
        };
        let png = match crate::model::export::encode_png(w, h, &rgba) {
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

    /// Ctrl+S / toolbar [Save]: crop → stash pixels → quit the overlay.
    /// The overlay is a layer-shell surface that would cover the native file
    /// dialog, so it exits first; the dialog itself (xdg-desktop-portal
    /// SaveFile), the write and the notification run on the main thread
    /// afterwards — see [`crate::save_dialog`]
    fn save_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((w, h, rgba)) = self.crop(cx) else {
            println!("[shotori] empty selection, ignoring");
            return;
        };
        println!(
            "[shotori] save: handing {w}x{h} (from {}) to the file dialog",
            self.capture.output_name
        );
        crate::save_dialog::stash(w, h, rgba);
        // Unmap all overlays — they would cover the save dialog (layer-shell
        // Overlay layer + exclusive keyboard). Quit is delayed a beat: the
        // run loop needs a few iterations to flush the surface-destroy
        // requests to the compositor — quitting immediately leaves frozen
        // frames mapped on screen (measured)
        cx.set_quit_mode(gpui_kit::QuitMode::Explicit);
        crate::save_dialog::close_overlays(window, cx);
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    }

    /// Ctrl+O / toolbar [OCR]. With cached models this runs immediately; on
    /// the very first use it opens the setup dialog (confirm → progress →
    /// cancel) instead — [`crate::ui::ocr_setup`].
    fn ocr_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.read(cx).blocked() {
            return; // dialog open or an OCR already running
        }
        let Some((w, h, rgba)) = self.crop(cx) else {
            println!("[shotori] empty selection, ignoring");
            return;
        };
        let snapshot = crate::ui::ocr_setup::Snapshot { w, h, rgba };

        self.session.update(cx, |s, cx| {
            s.set_blocked(true);
            cx.notify();
        });
        if crate::ocr::models_missing() {
            self.ocr_setup = Some(Box::new(crate::ui::ocr_setup::OcrSetup::new(snapshot)));
            self.setup_focus.focus_confirm(window, cx);
            cx.notify();
        } else {
            self.spawn_ocr(snapshot, window, cx);
        }
    }

    /// Run OCR on a snapshot (the Ctrl+O fast path, and the tail of the
    /// first-run setup once models are in): background inference → text to
    /// clipboard → quit. Free of `self` so both the action handler and the
    /// download poll loop can call it.
    fn spawn_ocr(
        &mut self,
        snapshot: crate::ui::ocr_setup::Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let crate::ui::ocr_setup::Snapshot { w, h, rgba } = snapshot;
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
    fn ocr_setup_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Retry after a failure needs a fresh progress arc
        let fresh = std::sync::Arc::new(crate::ocr::DownloadProgress::default());
        if let Some(setup) = self.ocr_setup.as_mut() {
            if !matches!(setup.stage, crate::ui::ocr_setup::Stage::Confirm)
                && !matches!(setup.stage, crate::ui::ocr_setup::Stage::Failed(_))
            {
                return; // already downloading
            }
            setup.progress = fresh;
            setup.stage = crate::ui::ocr_setup::Stage::Downloading;
        } else {
            return; // nothing to confirm (no dialog open)
        }
        let Some(setup) = self.ocr_setup.as_ref() else {
            return;
        };
        let progress = setup.progress.clone();
        self.setup_focus.focus_cancel(window, cx);
        crate::ocr::spawn_download(progress.clone());
        cx.notify();

        let entity = cx.entity();
        let window_handle = window.window_handle();

        cx.spawn(async move |_, cx| {
            let mut last = (0u64, 0u64, 0u8);
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(80))
                    .await;
                if !entity.update(cx, |this, _| {
                    this.ocr_setup
                        .as_ref()
                        .is_some_and(|s| s.owns_download(&progress))
                }) {
                    return; // cancelled, closed, or replaced by a newer attempt
                }
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
                    if this
                        .ocr_setup
                        .as_ref()
                        .is_some_and(|s| s.owns_download(&progress))
                    {
                        this.ocr_setup.take().map(|s| s.snapshot)
                    } else {
                        None
                    }
                });
                if let Some(crate::ui::ocr_setup::Snapshot { w, h, rgba }) = snap {
                    let _ = window_handle.update(cx, |_, window, cx| {
                        let focus = entity.read(cx).focus_handle.clone();
                        window.focus(&focus, cx);
                    });
                    ocr_to_clipboard(w, h, rgba, window_handle, entity, cx).await;
                }
            } else {
                let err = progress.error();
                let _ = window_handle.update(cx, |_, window, cx| {
                    entity.update(cx, |this, cx| {
                        if let Some(setup) = this.ocr_setup.as_mut()
                            && setup.owns_download(&progress)
                        {
                            setup.stage = crate::ui::ocr_setup::Stage::Failed(err);
                            this.setup_focus.focus_confirm(window, cx);
                            cx.notify();
                        }
                    });
                });
            }
        })
        .detach();
    }

    /// Setup dialog [Cancel]/[Close] and Esc: abort the download, clean up,
    /// back to plain selection mode.
    fn ocr_setup_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(setup) = self.ocr_setup.take() else {
            return;
        };
        setup
            .progress
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.session.update(cx, |s, cx| {
            s.set_blocked(false);
            cx.notify();
        });
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }
}

/// Background OCR → text to clipboard → quit. Shared by the Ctrl+O action
/// path and the post-download handover. Flips `ocr_busy` on the view for the
/// duration (spinner badge).
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
        this.session.update(cx, |s, cx| {
            s.set_blocked(false);
            cx.notify();
        });
        this.ocr_busy = false;
        cx.notify();
    });
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep display geometry stable while dragging. The backdrop paints
        // shared edges directly so fractional DPI cannot open layout seams.
        let shared = self.session.read(cx);
        let selection = shared.selection();
        let sel = shared.local_bounds(&self.capture.output_name).map(round_px);
        let backdrop = shared
            .backdrop_bounds(&self.capture.output_name)
            .map(round_px);
        let hover = shared.hover_bounds(&self.capture.output_name).map(round_px);
        let active = shared.active_on(&self.capture.output_name);
        let input_view = cx.entity().downgrade();
        let ws = window.bounds().size; // window logical size (= output logical size)

        // Bind the base chain, then attach feature-gated handlers via
        // shadowing — cfg attributes are illegal in the middle of a method
        // chain (see ROADMAP v0.6.2 pitfall notes)
        let base = div()
            .id("shotori-overlay")
            .key_context(if self.ocr_setup.is_some() {
                "ShotoriOcrSetup"
            } else {
                "ShotoriOverlay"
            })
            .size_full()
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &CopySelection, window, cx| {
                if this.session.read(cx).blocked() {
                    return; // setup dialog is modal
                }
                this.copy_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveSelection, window, cx| {
                if this.session.read(cx).blocked() {
                    return; // setup dialog is modal
                }
                this.save_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OcrSelection, window, cx| {
                this.ocr_selection(window, cx);
            }))
            .on_action(cx.listener(
                |this, _: &crate::ui::ocr_setup::OcrSetupConfirm, window, cx| {
                    this.ocr_setup_confirm(window, cx);
                },
            ))
            .on_action(cx.listener(
                |this, _: &crate::ui::ocr_setup::OcrSetupCancel, window, cx| {
                    this.ocr_setup_cancel(window, cx);
                    cx.stop_propagation();
                },
            ));

        // ⑥ First-run OCR setup dialog (confirm / progress), topmost
        let setup_el: Option<AnyElement> = self
            .ocr_setup
            .as_ref()
            .map(|s| crate::ui::ocr_setup::setup_card(s, &self.setup_focus).into_any_element());

        // OCR-in-flight spinner badge, centered on the selection
        let busy_el: Option<AnyElement> = self
            .ocr_busy
            .then(|| crate::ui::hud::ocr_busy_badge(sel, ws).into_any_element());

        base
            // Two-stage Esc (handled in place, no reliance on bubbling):
            // measured: dispatch_action inside a gpui window stops at the
            // focus path and never reaches App::on_action — exiting must
            // happen here. With the setup dialog open, Esc cancels the
            // dialog instead (aborting any download).
            .on_action(cx.listener(|this, _: &QuitOverlay, window, cx| {
                if this.ocr_setup.is_some() {
                    this.ocr_setup_cancel(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.session.read(cx).selection().is_dragging() {
                    this.session.update(cx, |s, cx| {
                        s.cancel_drag();
                        cx.notify();
                    }); // stage one: abandon this drag
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
                    if this.session.read(cx).blocked() {
                        return; // modal dialog: no new selections
                    }
                    this.session.update(cx, |s, cx| {
                        s.begin(&this.capture.output_name, ev.position);
                        cx.notify();
                    });
                    cx.notify();
                }),
            )
            // ── Layer stack (bottom to top) ─────────────────────────────
            // ① The frozen screen image (opaque, filling the window)
            .child(img(self.frozen.clone()).size_full())
            // ② Dim layer and selection border share painted edges.
            .child(selection_backdrop(backdrop))
            // ②½ Window-snap hover outline (above the dim, below all
            // selection chrome: it is a hint, not a selection)
            .children(hover.map(hover_outline))
            // Wayland may keep delivering a drag to its original surface even
            // outside its bounds. Element hover handlers would drop these events.
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, (), window, _| {
                        let view = input_view.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let _ = view.update(cx, |this, cx| {
                                this.session.update(cx, |s, cx| {
                                    // drag (with press held) or hover
                                    // tracking (idle pointer) — one event
                                    // feed drives both
                                    let dragged =
                                        s.drag_to(&this.capture.output_name, event.position);
                                    let hovered =
                                        s.hover_at(&this.capture.output_name, event.position);
                                    if dragged || hovered {
                                        cx.notify();
                                    }
                                });
                            });
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                                return;
                            }
                            let _ = input_view.update(cx, |this, cx| {
                                this.session.update(cx, |s, cx| {
                                    s.end(&this.capture.output_name, event.position);
                                    cx.notify();
                                });
                            });
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            // ③ Size label stays independent so narrow selections cannot wrap it.
            .children(
                sel.filter(|_| active)
                    .map(|b| selection_label(b, ws, round_px(selection.bounds().unwrap()).size)),
            )
            // ④ Toolbar: appears only after release (no flicker while dragging)
            .children(
                if selection.is_selected()
                    && active
                    && self.ocr_setup.is_none()
                    && let Some(bounds) = sel
                {
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

// ── Render helpers ──────────────────────────────────────────────────

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

#[cfg(test)]
mod multi_output_tests {
    use super::Overlay;
    use crate::{model::session::ScreenshotSession, platform::capture::Capture};
    use gpui_kit::{AppContext, MouseButton, TestAppContext, point, px, size};
    use std::sync::Arc;

    #[gpui_kit::test]
    fn pointer_events_share_selection_and_handle_release_outside_window(cx: &mut TestAppContext) {
        cx.update(gpui_kit::base::init);
        let make_capture = |name: &str, x| {
            let mut capture = Capture::for_test((x, 0), 1.);
            capture.output_name = name.into();
            capture.width = 400;
            capture.height = 400;
            capture.rgba = vec![255; 400 * 400 * 4];
            Arc::new(capture)
        };
        let left = make_capture("left", 0);
        let right = make_capture("right", 400);
        let session =
            cx.new(|_| ScreenshotSession::new(vec![left.clone(), right.clone()], Vec::new()));
        let mut second_context = cx.clone();
        let (_, left_cx) =
            cx.add_window_view(|window, cx| Overlay::new(left, session.clone(), window, cx));
        let (_, right_cx) = second_context
            .add_window_view(|window, cx| Overlay::new(right, session.clone(), window, cx));
        for context in [&mut *left_cx, &mut *right_cx] {
            context.simulate_resize(size(px(400.), px(400.)));
            context.update(|window, cx| window.draw(cx).clear(cx));
            context.run_until_parked();
        }
        left_cx.simulate_mouse_down(
            point(px(300.), px(80.)),
            MouseButton::Left,
            Default::default(),
        );
        left_cx.simulate_mouse_move(
            point(px(450.), px(180.)),
            MouseButton::Left,
            Default::default(),
        );
        left_cx.simulate_mouse_up(
            point(px(450.), px(180.)),
            MouseButton::Left,
            Default::default(),
        );
        left_cx.update(|_, cx| {
            let shared = session.read(cx);
            assert!(shared.selection().is_selected());
            assert_eq!(
                shared.local_bounds("right").unwrap().size,
                size(px(50.), px(100.))
            );
            assert_eq!(shared.crop("right").unwrap().0, 150);
        });
        right_cx.run_until_parked();
        right_cx.simulate_mouse_down(
            point(px(100.), px(80.)),
            MouseButton::Left,
            Default::default(),
        );
        right_cx.simulate_mouse_move(
            point(px(200.), px(180.)),
            MouseButton::Left,
            Default::default(),
        );
        right_cx.simulate_mouse_up(
            point(px(200.), px(180.)),
            MouseButton::Left,
            Default::default(),
        );
        left_cx.run_until_parked();
        left_cx.update(|_, cx| {
            let shared = session.read(cx);
            assert!(shared.local_bounds("left").is_none());
            assert!(shared.active_on("right"));
            assert_eq!(shared.crop("left").unwrap().0, 100);
        });
        // Some compositors transfer pointer events to the destination surface.
        // Convert its local coordinates using that output's desktop origin.
        left_cx.simulate_mouse_down(
            point(px(350.), px(80.)),
            MouseButton::Left,
            Default::default(),
        );
        right_cx.simulate_mouse_move(
            point(px(50.), px(180.)),
            MouseButton::Left,
            Default::default(),
        );
        right_cx.simulate_mouse_up(
            point(px(50.), px(180.)),
            MouseButton::Left,
            Default::default(),
        );
        right_cx.update(|_, cx| {
            let shared = session.read(cx);
            assert!(shared.selection().is_selected());
            assert_eq!(
                shared.selection().bounds().unwrap().size,
                size(px(100.), px(100.))
            );
            assert_eq!(shared.crop("right").unwrap().0, 100);
        });
    }
}
