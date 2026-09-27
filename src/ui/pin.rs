//! # Pinned screenshot (贴图): the selection as floating layer surfaces
//!
//! A pin is ONE shared state plus one fullscreen transparent layer
//! surface PER OUTPUT that shows its slice of the image — mark-shot's
//! approach, minus its plugin runtime, plus our multi-window twist:
//! because every surface renders its own intersection with the pin,
//! dragging across outputs is seamless (no destroy/rebind flicker, no
//! drag hand-off — the same shared-entity pattern the overlays use
//! with the session).
//!
//! Layer surfaces own their geometry (the compositor does not place
//! them), so the pin sits at the selection's exact spot from the FIRST
//! frame, floats above every toplevel and is never tiled. Pointer
//! input is confined to the image through per-surface input regions —
//! everything else clicks through to the desktop beneath.
//!
//! Interactions (scope: move + zoom): drag the image to move
//! (client-side, clamped so a grabbable slice stays on the desktop),
//! scroll to zoom, `Esc` to close. Keyboard is OnDemand: click the pin
//! first; the desktop never loses its keyboard otherwise.

use std::sync::{Arc, OnceLock};

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;

use crate::ui::image_util;

actions!(pin, [ClosePin]);

/// How far one "line" of scroll zooms.
const ZOOM_STEP: f32 = 1.12;
/// Zoom range: from a postage stamp to deep-pixel-peeping.
const ZOOM_MIN: f32 = 0.05;
const ZOOM_MAX: f32 = 24.;
/// At least this much of the pin must stay on-screen while dragging.
const MIN_VISIBLE: f32 = 24.;

/// One output the pin can live on: global logical bounds + the
/// display the layer surface binds to.
#[derive(Clone)]
struct PinOutput {
    bounds: Bounds<Pixels>,
    display_id: Option<DisplayId>,
}

/// The desktop's outputs, registered by `main` once displays resolve.
static OUTPUTS: OnceLock<std::sync::Mutex<Vec<PinOutput>>> = OnceLock::new();

/// Remember one output (bounds in GLOBAL logical px — the session's
/// coordinate space) so pins can span outputs.
pub fn register_output(bounds: Bounds<Pixels>, display_id: Option<DisplayId>) {
    OUTPUTS
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
        .unwrap()
        .push(PinOutput { bounds, display_id });
}

fn outputs() -> Vec<PinOutput> {
    OUTPUTS
        .get()
        .map(|o| o.lock().unwrap().clone())
        .unwrap_or_default()
}

/// The union of all outputs — the desktop a pin can occupy.
fn desktop() -> Option<Bounds<Pixels>> {
    outputs()
        .iter()
        .map(|o| o.bounds)
        .reduce(|a, b| a.union(&b))
}

/// Scroll lines (pixel deltas pre-normalized by the caller) → the next
/// zoom factor, clamped to the range.
fn next_zoom(zoom: f32, lines: f32) -> f32 {
    (zoom * ZOOM_STEP.powf(lines)).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The on-screen size for a zoom factor.
fn zoomed_size(base: Size<Pixels>, zoom: f32) -> Size<Pixels> {
    size(
        px((f32::from(base.width) * zoom).max(1.)),
        px((f32::from(base.height) * zoom).max(1.)),
    )
}

/// Keep the pin grabbable: at least `MIN_VISIBLE` stays inside the
/// desktop on each axis (parking it mostly off an edge is fine).
fn clamp_origin(origin: Point<Pixels>, pin: Size<Pixels>, desk: Bounds<Pixels>) -> Point<Pixels> {
    let (w, h) = (f32::from(pin.width), f32::from(pin.height));
    let (vw, vh) = (w.min(MIN_VISIBLE), h.min(MIN_VISIBLE));
    let (dl, dt) = (f32::from(desk.left()), f32::from(desk.top()));
    let (dr, db) = (f32::from(desk.right()), f32::from(desk.bottom()));
    point(
        px(f32::from(origin.x).clamp(dl - (w - vw), dr - vw)),
        px(f32::from(origin.y).clamp(dt - (h - vh), db - vh)),
    )
}

/// The pin's shared state — the "session" of its per-output surfaces.
/// The rect is in GLOBAL logical coordinates; each surface renders its
/// own intersection (see `PinSurface::render`).
pub(crate) struct PinState {
    image: Arc<RenderImage>,
    /// Image size at zoom 1, in GLOBAL logical px (matching the
    /// region's original on-screen size).
    base: Size<Pixels>,
    /// The pin's rect in global logical px — THE geometry: rendering,
    /// input regions and the drag clamp all read this one field, so
    /// they cannot drift apart.
    rect: Bounds<Pixels>,
    /// Grab offset while dragging (pointer → rect origin, global).
    drag: Option<Point<Pixels>>,
    /// Every surface of this pin; `Esc` closes them all.
    windows: Vec<AnyWindowHandle>,
}

impl PinState {
    fn zoom_by(&mut self, lines: f32, cx: &mut Context<Self>) {
        if lines == 0. {
            return;
        }
        let zoom = f32::from(self.rect.size.width) / f32::from(self.base.width);
        let size = zoomed_size(self.base, next_zoom(zoom, lines));
        self.rect = Bounds::new(self.rect.origin, size);
        cx.notify();
    }
}

/// One fullscreen transparent surface on one output, showing its
/// slice of the shared pin.
pub(crate) struct PinSurface {
    state: Entity<PinState>,
    focus_handle: FocusHandle,
    /// This output's global bounds (the surface covers it fully).
    output: PinOutput,
}

impl PinSurface {
    fn new(
        state: Entity<PinState>,
        output: PinOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            state,
            focus_handle,
            output,
        }
    }
}

impl Render for PinSurface {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The window-level drag listeners (re-registered every paint —
        // the overlay's pointer_event_sink pattern) plus the input
        // region: only this surface's slice of the pin takes input,
        // everything else (including every other output) clicks
        // through. Both take effect at this frame's commit.
        let state = self.state.clone();
        let output = self.output.clone();
        let focus = self.focus_handle.clone();
        let view = cx.entity().downgrade();

        let sink = canvas(
            move |_, _, _| (),
            move |_, (), window, cx| {
                let slice = state.read(cx).rect.intersect(&output.bounds);
                let local = if slice.size.width > px(0.) && slice.size.height > px(0.) {
                    Bounds::new(slice.origin - output.bounds.origin, slice.size)
                } else {
                    Bounds::new(point(px(0.), px(0.)), size(px(0.), px(0.)))
                };
                window.set_input_region(Some(&[local]));
                let move_view = view.clone();
                let move_state = state.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let _ = move_view.update(cx, |_, cx| {
                        let global = event.position + output.bounds.origin;
                        move_state.update(cx, |s, cx| {
                            let Some(grab) = s.drag else {
                                return;
                            };
                            if let Some(desk) = desktop() {
                                s.rect.origin = clamp_origin(global - grab, s.rect.size, desk);
                            }
                            cx.notify();
                        });
                    });
                });
                let up_view = view.clone();
                let up_state = state.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                        return;
                    }
                    let _ = up_view.update(cx, |_, cx| {
                        up_state.update(cx, |s, cx| {
                            if s.drag.take().is_some() {
                                cx.notify();
                            }
                        });
                    });
                });
            },
        )
        .absolute()
        .size_full();

        // This surface's visible slice of the pin, in local coords
        let slice = self.state.read(cx).rect.intersect(&self.output.bounds);
        let local = slice.origin - self.output.bounds.origin;
        let pin = div()
            .absolute()
            .left(local.x)
            .top(local.y)
            .w(slice.size.width)
            .h(slice.size.height)
            .overflow_hidden()
            // a thin frame keeps the pin readable on same-colored
            // backgrounds; it follows the global rect, so the frame
            // "breaks" at output seams exactly like the image does
            .border_1()
            .border_color(rgba(crate::ui::theme::c().toolbar_border))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus_handle, cx); // OnDemand keyboard
                    let global = ev.position + this.output.bounds.origin;
                    this.state.update(cx, |s, cx| {
                        s.drag = Some(global - s.rect.origin);
                        cx.notify();
                    });
                }),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let lines = match ev.delta {
                    ScrollDelta::Lines(l) => l.y,
                    ScrollDelta::Pixels(p) => f32::from(p.y) / 40.,
                };
                this.state.update(cx, |s, cx| s.zoom_by(lines, cx));
            }))
            .child(img(self.state.read(cx).image.clone()).size_full());

        div()
            .id("shotori-pin")
            .key_context("ShotoriPin")
            .track_focus(&focus)
            .size_full()
            .on_action(cx.listener(|this, _: &ClosePin, _, cx| {
                // one Esc closes the whole pin: every surface, everywhere
                let windows = this.state.read(cx).windows.clone();
                for handle in windows {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
            }))
            .child(sink)
            .child(pin)
    }
}

/// Everything needed to birth one pin.
pub(crate) struct PinSpec {
    /// The crop: device pixels (`w`×`h`).
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// The source output's scale factor — zoom 1.0 then shows the
    /// region at its original on-screen size.
    pub source_scale: f32,
    /// The selection's rect in GLOBAL logical coordinates (the
    /// session's space) — the pin's birth geometry.
    pub rect: Bounds<Pixels>,
}

/// Open a pin: one shared state, one transparent layer surface per
/// registered output. Fails only if no output ever registered.
pub(crate) fn open(spec: PinSpec, cx: &mut App) -> anyhow::Result<()> {
    let outputs = outputs();
    anyhow::ensure!(!outputs.is_empty(), "no outputs registered");
    let PinSpec {
        w,
        h,
        rgba,
        source_scale,
        rect,
    } = spec;
    let scale = if source_scale > 0. { source_scale } else { 1. };
    let base = size(px(w as f32 / scale), px(h as f32 / scale));
    let image = image_util::rgba_to_render_image(rgba, w, h);
    let rect = Bounds::new(rect.origin, base);
    let state = cx.new(|_| PinState {
        image,
        base,
        rect,
        drag: None,
        windows: Vec::new(),
    });

    let mut windows = Vec::new();
    for output in outputs {
        let o = output.clone();
        let st = state.clone();
        let handle = cx.open_window(
            WindowOptions {
                titlebar: None,
                window_background: WindowBackgroundAppearance::Transparent,
                focus: false, // OnDemand: click the pin to give it the keyboard
                display_id: output.display_id,
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    o.bounds.size,
                ))),
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "shotori-pin".into(),
                    layer: Layer::Top,
                    anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                    exclusive_zone: Some(px(-1.)),
                    keyboard_interactivity: KeyboardInteractivity::OnDemand,
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| PinSurface::new(st, o, window, cx)),
        )?;
        windows.push(handle.into());
    }
    state.update(cx, |s, cx| {
        s.windows = windows;
        cx.notify();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    // NO `use super::*` here: chained globs (this → pin → gpui_kit::*)
    // blow rustc's macro-expansion recursion inside #[test] (rustc
    // 1.98, "recursion limit reached while expanding #[test]"). Import
    // exactly what the tests touch instead.
    use super::{MIN_VISIBLE, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP, clamp_origin, next_zoom, zoomed_size};
    use gpui_kit::{Bounds, Pixels, point, px, size};

    fn desk() -> Bounds<Pixels> {
        Bounds::new(point(px(-100.), px(0.)), size(px(1100.), px(500.)))
    }

    #[test]
    fn zoom_tracks_the_base_size_and_clamps() {
        let base = size(px(100.), px(60.));
        let mut zoom = next_zoom(1., 3.);
        assert!((zoom - ZOOM_STEP.powi(3)).abs() < 1e-4);
        let s = zoomed_size(base, zoom);
        assert!((f32::from(s.width) - 100. * zoom).abs() < 0.01);
        assert!((f32::from(s.height) - 60. * zoom).abs() < 0.01);

        for _ in 0..200 {
            zoom = next_zoom(zoom, 3.);
        }
        assert_eq!(zoom, ZOOM_MAX);
        for _ in 0..400 {
            zoom = next_zoom(zoom, -3.);
        }
        assert_eq!(zoom, ZOOM_MIN);
        let s = zoomed_size(size(px(3.), px(3.)), ZOOM_MIN);
        assert_eq!(
            (f32::from(s.width) as u32, f32::from(s.height) as u32),
            (1, 1)
        );
    }

    #[test]
    fn dragging_keeps_a_grabbable_slice_on_the_desktop() {
        let pin: gpui_kit::Size<Pixels> = size(px(100.), px(60.));

        // fully inside stays put
        let p = clamp_origin(point(px(120.), px(80.)), pin, desk());
        assert_eq!((p.x, p.y), (px(120.), px(80.)));

        // off the left/top edge only as far as the visible slice allows
        let p = clamp_origin(point(px(-5000.), px(-5000.)), pin, desk());
        assert_eq!(
            (p.x, p.y),
            (px(-100. - (100. - MIN_VISIBLE)), px(-(60. - MIN_VISIBLE)))
        );

        // right/bottom edge keeps MIN_VISIBLE on the desktop
        let p = clamp_origin(point(px(9000.), px(9000.)), pin, desk());
        assert_eq!(
            (p.x, p.y),
            (px(1000. - MIN_VISIBLE), px(500. - MIN_VISIBLE))
        );

        // a pin smaller than the slice simply stays fully visible
        let tiny: gpui_kit::Size<Pixels> = size(px(10.), px(8.));
        let p = clamp_origin(point(px(-500.), px(-500.)), tiny, desk());
        assert_eq!((p.x, p.y), (px(-100.), px(0.)));
    }
}
