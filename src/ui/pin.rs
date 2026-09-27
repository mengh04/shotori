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
//! scroll to zoom, right-click to open the Close menu. Keyboard is OnDemand: click the pin
//! first; the desktop never loses its keyboard otherwise.

use std::sync::{Arc, OnceLock};

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;

use crate::ui::image_util;

actions!(pin, [ClosePin, DismissPinMenu, OpenPinMenu]);

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
fn clamp_to_output(
    origin: Point<Pixels>,
    pin: Size<Pixels>,
    desk: Bounds<Pixels>,
) -> Point<Pixels> {
    let (w, h) = (f32::from(pin.width), f32::from(pin.height));
    let (vw, vh) = (
        w.min(MIN_VISIBLE).min(f32::from(desk.size.width)),
        h.min(MIN_VISIBLE).min(f32::from(desk.size.height)),
    );
    let (dl, dt) = (f32::from(desk.left()), f32::from(desk.top()));
    let (dr, db) = (f32::from(desk.right()), f32::from(desk.bottom()));
    point(
        px(f32::from(origin.x).clamp(dl - (w - vw), dr - vw)),
        px(f32::from(origin.y).clamp(dt - (h - vh), db - vh)),
    )
}

/// Choose the nearest position with a grabbable area on an actual output,
/// rather than treating the gaps inside the desktop bounding box as visible.
fn clamp_origin(
    origin: Point<Pixels>,
    pin: Size<Pixels>,
    outputs: &[Bounds<Pixels>],
) -> Point<Pixels> {
    let distance = |p: &Point<Pixels>| f32::from(p.x - origin.x).hypot(f32::from(p.y - origin.y));
    outputs
        .iter()
        .filter(|output| output.size.width > px(0.) && output.size.height > px(0.))
        .map(|output| clamp_to_output(origin, pin, *output))
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .unwrap_or(origin)
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
    /// Every surface of this pin; the Close command closes them all.
    windows: Vec<AnyWindowHandle>,
    outputs: Vec<Bounds<Pixels>>,
}

impl PinState {
    fn zoom_by(&mut self, lines: f32, cx: &mut Context<Self>) {
        if lines == 0. {
            return;
        }
        let zoom = f32::from(self.rect.size.width) / f32::from(self.base.width);
        let size = zoomed_size(self.base, next_zoom(zoom, lines));
        self.rect = Bounds::new(clamp_origin(self.rect.origin, size, &self.outputs), size);
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
    menu_position: Option<Point<Pixels>>,
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
            menu_position: None,
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

        let menu_open = self.menu_position.is_some();
        let sink = canvas(
            move |_, _, _| (),
            move |_, (), window, cx| {
                let slice = state.read(cx).rect.intersect(&output.bounds);
                let local = if slice.size.width > px(0.) && slice.size.height > px(0.) {
                    Bounds::new(slice.origin - output.bounds.origin, slice.size)
                } else {
                    Bounds::new(point(px(0.), px(0.)), size(px(0.), px(0.)))
                };
                // While the menu is open, receive outside clicks to dismiss it.
                let input = if menu_open {
                    Bounds::new(point(px(0.), px(0.)), output.bounds.size)
                } else {
                    local
                };
                window.set_input_region(Some(&[input]));
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
                            s.rect.origin = clamp_origin(global - grab, s.rect.size, &s.outputs);
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
                            if let Some(grab) = s.drag.take() {
                                let global = event.position + output.bounds.origin;
                                s.rect.origin =
                                    clamp_origin(global - grab, s.rect.size, &s.outputs);
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
        let rect = self.state.read(cx).rect;
        let slice = rect.intersect(&self.output.bounds);
        let local = rect.origin - self.output.bounds.origin;
        let pin = div()
            .id("pin-image")
            .debug_selector(|| "pin-image".into())
            .absolute()
            .left(local.x)
            .top(local.y)
            .w(rect.size.width)
            .h(rect.size.height)
            .overflow_hidden()
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
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus_handle, cx);
                    this.menu_position = Some(ev.position);
                    this.state.update(cx, |s, cx| {
                        s.drag = None;
                        cx.notify();
                    });
                    cx.notify();
                    cx.stop_propagation();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let lines = match ev.delta {
                    ScrollDelta::Lines(l) => l.y,
                    ScrollDelta::Pixels(p) => f32::from(p.y) / 40.,
                };
                this.state.update(cx, |s, cx| s.zoom_by(lines, cx));
            }))
            // Keep the original global image geometry on every output. The
            // fullscreen root clips it; resizing to the intersection would
            // display a separate miniature of the entire image on each screen.
            .child(
                img(self.state.read(cx).image.clone())
                    .size_full()
                    .object_fit(ObjectFit::Fill),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .border_1()
                    .border_color(rgba(crate::ui::theme::c().toolbar_border)),
            );

        let menu = self.menu_position.map(|position| {
            let menu_size = size(
                px(160.).min(self.output.bounds.size.width),
                px(40.).min(self.output.bounds.size.height),
            );
            let origin = point(
                position
                    .x
                    .max(px(0.))
                    .min(self.output.bounds.size.width - menu_size.width),
                position
                    .y
                    .max(px(0.))
                    .min(self.output.bounds.size.height - menu_size.height),
            );
            div()
                .id("pin-menu-backdrop")
                .absolute()
                .size_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.menu_position = None;
                        cx.notify();
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _, _, cx| {
                        this.menu_position = None;
                        cx.notify();
                        cx.stop_propagation();
                    }),
                )
                .child(
                    div()
                        .id("pin-menu")
                        .debug_selector(|| "pin-menu".into())
                        .absolute()
                        .left(origin.x)
                        .top(origin.y)
                        .w(menu_size.width)
                        .h(menu_size.height)
                        .p_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgba(crate::ui::theme::c().toolbar_border))
                        .bg(rgba(crate::ui::theme::c().toolbar_bg))
                        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                        .child(
                            gpui_kit::base::Button::new("pin-close")
                                .debug_selector(|| "pin-close".into())
                                .accessibility_label("Close pin")
                                .size_full()
                                .px_2()
                                .rounded_sm()
                                .text_sm()
                                .text_color(rgba(crate::ui::theme::c().toolbar_text))
                                .hover(|s| s.bg(rgba(crate::ui::theme::c().toolbar_hover)))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(ClosePin), cx)
                                })
                                .child("Close"),
                        ),
                )
        });

        div()
            .id("shotori-pin")
            .overflow_hidden()
            .key_context(if menu_open {
                "ShotoriPinMenu"
            } else {
                "ShotoriPin"
            })
            .track_focus(&focus)
            .size_full()
            .on_action(cx.listener(|this, _: &DismissPinMenu, _, cx| {
                this.menu_position = None;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenPinMenu, _, cx| {
                let rect = this.state.read(cx).rect.intersect(&this.output.bounds);
                this.menu_position = Some(rect.origin - this.output.bounds.origin);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ClosePin, window, cx| {
                // Close the whole pin: every surface, everywhere
                let windows = this.state.read(cx).windows.clone();
                let current = window.window_handle();
                window.remove_window();
                for handle in windows {
                    if handle == current {
                        continue;
                    }
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
            }))
            .child(sink)
            .children((slice.size.width > px(0.) && slice.size.height > px(0.)).then_some(pin))
            .children(menu)
    }
}

/// Everything needed to birth one pin.
pub(crate) struct PinSpec {
    /// The crop: device pixels (`w`×`h`).
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// The selection's rect in GLOBAL logical coordinates (the
    /// session's space) — the pin's birth geometry.
    pub rect: Bounds<Pixels>,
}

/// Open a pin: one shared state, one transparent layer surface per
/// registered output. Fails only if no output ever registered.
pub(crate) fn open(spec: PinSpec, cx: &mut App) -> anyhow::Result<()> {
    let outputs = outputs();
    anyhow::ensure!(!outputs.is_empty(), "no outputs registered");
    let PinSpec { w, h, rgba, rect } = spec;
    let base = rect.size;
    let image = image_util::rgba_to_render_image(rgba, w, h);
    let state = cx.new(|_| PinState {
        image,
        base,
        rect,
        drag: None,
        windows: Vec::new(),
        outputs: outputs.iter().map(|output| output.bounds).collect(),
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
    fn dragging_avoids_gaps_and_handles_staggered_outputs() {
        let outputs = [
            Bounds::new(point(px(-200.), px(0.)), size(px(100.), px(100.))),
            Bounds::new(point(px(0.), px(150.)), size(px(120.), px(150.))),
        ];
        for pin in [size(px(20.), px(20.)), size(px(200.), px(100.))] {
            for x in [-600., -150., -50., 0., 200.] {
                for y in [-100., 50., 120., 200., 500.] {
                    let requested = point(px(x), px(y));
                    let origin = clamp_origin(requested, pin, &outputs);
                    let visible = |origin| {
                        outputs.iter().any(|output| {
                            let intersection = Bounds::new(origin, pin).intersect(output);
                            intersection.size.width >= pin.width.min(px(MIN_VISIBLE))
                                && intersection.size.height >= pin.height.min(px(MIN_VISIBLE))
                        })
                    };
                    assert!(visible(origin));
                    if visible(requested) {
                        assert_eq!(origin, requested);
                    }
                }
            }
        }
    }

    #[test]
    fn dragging_keeps_a_grabbable_slice_on_the_desktop() {
        let pin: gpui_kit::Size<Pixels> = size(px(100.), px(60.));

        // fully inside stays put
        let p = clamp_origin(point(px(120.), px(80.)), pin, &[desk()]);
        assert_eq!((p.x, p.y), (px(120.), px(80.)));

        // off the left/top edge only as far as the visible slice allows
        let p = clamp_origin(point(px(-5000.), px(-5000.)), pin, &[desk()]);
        assert_eq!(
            (p.x, p.y),
            (px(-100. - (100. - MIN_VISIBLE)), px(-(60. - MIN_VISIBLE)))
        );

        // right/bottom edge keeps MIN_VISIBLE on the desktop
        let p = clamp_origin(point(px(9000.), px(9000.)), pin, &[desk()]);
        assert_eq!(
            (p.x, p.y),
            (px(1000. - MIN_VISIBLE), px(500. - MIN_VISIBLE))
        );

        // a pin smaller than the slice simply stays fully visible
        let tiny: gpui_kit::Size<Pixels> = size(px(10.), px(8.));
        let p = clamp_origin(point(px(-500.), px(-500.)), tiny, &[desk()]);
        assert_eq!((p.x, p.y), (px(-100.), px(0.)));
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::{PinOutput, PinState, PinSurface};
    use gpui_kit::{AppContext, Bounds, TestAppContext, point, px, size};

    #[gpui_kit::test]
    fn shrinking_at_left_and_top_edges_keeps_pin_visible(cx: &mut TestAppContext) {
        let output = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(400.)));
        let base = size(px(200.), px(100.));
        let state = cx.new(|_| PinState {
            image: crate::ui::image_util::rgba_to_render_image(vec![255; 200 * 100 * 4], 200, 100),
            base,
            rect: Bounds::new(
                super::clamp_origin(point(px(-500.), px(-500.)), base, &[output]),
                base,
            ),
            drag: None,
            windows: Vec::new(),
            outputs: vec![output],
        });
        state.update(cx, |s, cx| {
            for _ in 0..20 {
                s.zoom_by(-2., cx);
                let visible = s.rect.intersect(&output);
                assert!(visible.size.width >= s.rect.size.width.min(px(super::MIN_VISIBLE)));
                assert!(visible.size.height >= s.rect.size.height.min(px(super::MIN_VISIBLE)));
            }
        });
    }

    #[gpui_kit::test]
    fn cross_output_layout_keeps_the_entire_image_at_a_shared_scale(cx: &mut TestAppContext) {
        let rect = Bounds::new(point(px(150.), px(20.)), size(px(100.), px(60.)));
        let state = cx.new(|_| PinState {
            image: crate::ui::image_util::rgba_to_render_image(vec![255; 200 * 120 * 4], 200, 120),
            base: rect.size,
            rect,
            drag: None,
            windows: Vec::new(),
            outputs: Vec::new(),
        });
        for output_x in [0., 200., 400.] {
            let mut window_cx = cx.clone();
            let (_, vcx) = window_cx.add_window_view(|window, cx| {
                PinSurface::new(
                    state.clone(),
                    PinOutput {
                        bounds: Bounds::new(point(px(output_x), px(0.)), size(px(200.), px(200.))),
                        display_id: None,
                    },
                    window,
                    cx,
                )
            });
            vcx.simulate_resize(size(px(200.), px(200.)));
            vcx.update(|window, cx| window.draw(cx).clear(cx));
            if output_x == 400. {
                assert!(vcx.debug_bounds("pin-image").is_none());
            } else {
                let bounds = vcx.debug_bounds("pin-image").unwrap();
                assert_eq!(bounds.origin, point(px(150. - output_x), px(20.)));
                assert_eq!(bounds.size, rect.size);
            }
        }
    }

    #[gpui_kit::test]
    fn drag_uses_the_release_position_even_without_a_final_move(cx: &mut TestAppContext) {
        let output = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(400.)));
        let state = cx.new(|_| PinState {
            image: crate::ui::image_util::rgba_to_render_image(vec![255; 100 * 60 * 4], 100, 60),
            base: size(px(100.), px(60.)),
            rect: Bounds::new(point(px(20.), px(20.)), size(px(100.), px(60.))),
            drag: None,
            windows: Vec::new(),
            outputs: vec![output],
        });
        let (_, vcx) = cx.add_window_view(|window, cx| {
            PinSurface::new(
                state.clone(),
                PinOutput {
                    bounds: output,
                    display_id: None,
                },
                window,
                cx,
            )
        });
        vcx.simulate_resize(output.size);
        vcx.update(|window, cx| window.draw(cx).clear(cx));
        vcx.simulate_mouse_down(
            point(px(30.), px(25.)),
            gpui_kit::MouseButton::Left,
            Default::default(),
        );
        vcx.simulate_mouse_move(
            point(px(40.), px(40.)),
            gpui_kit::MouseButton::Left,
            Default::default(),
        );
        vcx.simulate_mouse_up(
            point(px(50.), px(45.)),
            gpui_kit::MouseButton::Left,
            Default::default(),
        );
        vcx.update(|_, cx| {
            let state = state.read(cx);
            assert_eq!(state.rect.origin, point(px(40.), px(40.)));
            assert!(state.drag.is_none());
        });
    }

    #[gpui_kit::test]
    fn menu_closes_all_surfaces_but_escape_only_dismisses_menu(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::base::init(cx);
            crate::actions::bind_keys(cx);
        });
        let state = cx.new(|_| PinState {
            image: crate::ui::image_util::rgba_to_render_image(vec![255; 100 * 60 * 4], 100, 60),
            base: size(px(100.), px(60.)),
            rect: Bounds::new(point(px(20.), px(20.)), size(px(100.), px(60.))),
            drag: None,
            windows: Vec::new(),
            outputs: vec![
                Bounds::new(point(px(0.), px(0.)), size(px(400.), px(400.))),
                Bounds::new(point(px(400.), px(0.)), size(px(400.), px(400.))),
            ],
        });
        let mut other = cx.clone();
        let (_, first) = cx.add_window_view(|window, cx| {
            PinSurface::new(
                state.clone(),
                PinOutput {
                    bounds: Bounds::new(point(px(0.), px(0.)), size(px(400.), px(400.))),
                    display_id: None,
                },
                window,
                cx,
            )
        });
        let (_, second) = other.add_window_view(|window, cx| {
            PinSurface::new(
                state.clone(),
                PinOutput {
                    bounds: Bounds::new(point(px(400.), px(0.)), size(px(400.), px(400.))),
                    display_id: None,
                },
                window,
                cx,
            )
        });
        let mut handles = Vec::new();
        for context in [&mut *first, &mut *second] {
            handles.push(context.update(|window, cx| {
                let handle = window.window_handle();
                state.update(cx, |s, _| s.windows.push(handle));
                window.draw(cx).clear(cx);
                handle
            }));
        }
        first.simulate_resize(size(px(400.), px(400.)));
        first.simulate_keystrokes("escape");
        assert!(first.windows().contains(&handles[0]));
        first.simulate_mouse_down(
            point(px(110.), px(70.)),
            gpui_kit::MouseButton::Right,
            Default::default(),
        );
        first.update(|window, cx| window.draw(cx).clear(cx));
        assert!(first.debug_bounds("pin-menu").is_some());
        first.simulate_keystrokes("escape");
        first.update(|window, cx| window.draw(cx).clear(cx));
        assert!(first.debug_bounds("pin-menu").is_none());
        assert!(first.windows().contains(&handles[0]));
        first.simulate_keystrokes("shift-f10");
        first.update(|window, cx| window.draw(cx).clear(cx));
        assert!(first.debug_bounds("pin-menu").is_some());
        first.simulate_mouse_down(
            point(px(390.), px(390.)),
            gpui_kit::MouseButton::Left,
            Default::default(),
        );
        first.update(|window, cx| window.draw(cx).clear(cx));
        assert!(first.debug_bounds("pin-menu").is_none());
        first.update(|_, cx| assert!(state.read(cx).drag.is_none()));
        first.simulate_mouse_down(
            point(px(110.), px(70.)),
            gpui_kit::MouseButton::Right,
            Default::default(),
        );
        first.update(|window, cx| window.draw(cx).clear(cx));
        // The command extends outside the image, and must still receive clicks.
        let button = first.debug_bounds("pin-close").unwrap();
        first.simulate_click(button.center(), Default::default());
        first.run_until_parked();
        for handle in handles {
            assert!(!first.windows().contains(&handle));
        }
    }
}
