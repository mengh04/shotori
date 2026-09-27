//! Pinned images share one ordered scene across all output surfaces.
//! Clicking a pin raises it without recreating any native windows.

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

/// All pins share one ordered scene and one surface per output. The last
/// entry is selected and painted last on EVERY output.
struct Pin {
    id: u64,
    image: Arc<RenderImage>,
    base: Size<Pixels>,
    rect: Bounds<Pixels>,
}

struct PinBoard {
    pins: Vec<Pin>,
    next_id: u64,
    drag: Option<(u64, Point<Pixels>)>,
    menu: Option<(u64, Point<Pixels>)>,
    windows: Vec<AnyWindowHandle>,
    outputs: Vec<Bounds<Pixels>>,
}

impl PinBoard {
    fn new(outputs: Vec<Bounds<Pixels>>) -> Self {
        Self {
            pins: Vec::new(),
            next_id: 0,
            drag: None,
            menu: None,
            windows: Vec::new(),
            outputs,
        }
    }

    fn add(&mut self, spec: PinSpec, cx: &mut Context<Self>) {
        self.next_id += 1;
        let mut rect = spec.rect;
        rect.origin = clamp_origin(rect.origin, rect.size, &self.outputs);
        self.pins.push(Pin {
            id: self.next_id,
            image: image_util::rgba_to_render_image(spec.rgba, spec.w, spec.h),
            base: spec.rect.size,
            rect,
        });
        self.menu = None;
        self.drag = None;
        cx.notify();
    }

    fn raise(&mut self, id: u64) {
        if let Some(ix) = self.pins.iter().position(|pin| pin.id == id) {
            let pin = self.pins.remove(ix);
            self.pins.push(pin);
        }
    }

    fn move_drag(&mut self, position: Point<Pixels>) {
        if let Some((id, grab)) = self.drag
            && let Some(pin) = self.pins.iter_mut().find(|pin| pin.id == id)
        {
            pin.rect.origin = clamp_origin(position - grab, pin.rect.size, &self.outputs);
        }
    }

    fn zoom(&mut self, id: u64, lines: f32) {
        if let Some(pin) = self.pins.iter_mut().find(|pin| pin.id == id) {
            let zoom = f32::from(pin.rect.size.width) / f32::from(pin.base.width);
            let size = zoomed_size(pin.base, next_zoom(zoom, lines));
            pin.rect = Bounds::new(clamp_origin(pin.rect.origin, size, &self.outputs), size);
        }
    }
}

struct PinSurface {
    board: Entity<PinBoard>,
    focus_handle: FocusHandle,
    output: PinOutput,
    _subscription: Subscription,
}

impl PinSurface {
    fn new(
        board: Entity<PinBoard>,
        output: PinOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let subscription = cx.observe(&board, |_, _, cx| cx.notify());
        Self {
            board,
            focus_handle,
            output,
            _subscription: subscription,
        }
    }

    fn close_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let empty = self.board.update(cx, |board, cx| {
            let Some((id, _)) = board.menu.take() else {
                return false;
            };
            board.pins.retain(|pin| pin.id != id);
            if board.drag.is_some_and(|(drag_id, _)| drag_id == id) {
                board.drag = None;
            }
            cx.notify();
            board.pins.is_empty()
        });
        if empty {
            let handles = self.board.read(cx).windows.clone();
            let current = window.window_handle();
            window.remove_window();
            for handle in handles {
                if handle != current {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
            }
        }
    }
}

impl Render for PinSurface {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let board = self.board.clone();
        let output = self.output.bounds;
        let menu = board
            .read(cx)
            .menu
            .filter(|(_, position)| output.contains(position));
        let any_menu = board.read(cx).menu.is_some();
        let sink = canvas(
            |_, _, _| (),
            move |_, (), window, cx| {
                let regions: Vec<_> = if any_menu {
                    vec![Bounds::new(point(px(0.), px(0.)), output.size)]
                } else {
                    board
                        .read(cx)
                        .pins
                        .iter()
                        .filter_map(|pin| {
                            let slice = pin.rect.intersect(&output);
                            (slice.size.width > px(0.) && slice.size.height > px(0.))
                                .then_some(Bounds::new(slice.origin - output.origin, slice.size))
                        })
                        .collect()
                };
                window.set_input_region(Some(&regions));
                let moving = board.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && moving.read(cx).drag.is_some() {
                        moving.update(cx, |board, cx| {
                            board.move_drag(event.position + output.origin);
                            cx.notify();
                        });
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && board.read(cx).drag.is_some()
                    {
                        board.update(cx, |board, cx| {
                            board.move_drag(event.position + output.origin);
                            board.drag = None;
                            cx.notify();
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full();

        let pins: Vec<_> = self
            .board
            .read(cx)
            .pins
            .iter()
            .filter_map(|pin| {
                let slice = pin.rect.intersect(&output);
                if slice.size.width <= px(0.) || slice.size.height <= px(0.) {
                    return None;
                }
                let id = pin.id;
                let local = pin.rect.origin - output.origin;
                Some(
                    div()
                        .id(("pin-image", id))
                        .debug_selector(move || format!("pin-image-{id}"))
                        .absolute()
                        .left(local.x)
                        .top(local.y)
                        .w(pin.rect.size.width)
                        .h(pin.rect.size.height)
                        .overflow_hidden()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                window.focus(&this.focus_handle, cx);
                                this.board.update(cx, |board, cx| {
                                    board.raise(id);
                                    let pin = board.pins.last().unwrap();
                                    board.drag = Some((
                                        id,
                                        event.position + output.origin - pin.rect.origin,
                                    ));
                                    board.menu = None;
                                    cx.notify();
                                });
                                cx.stop_propagation();
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                window.focus(&this.focus_handle, cx);
                                this.board.update(cx, |board, cx| {
                                    board.raise(id);
                                    board.drag = None;
                                    board.menu = Some((id, event.position + output.origin));
                                    cx.notify();
                                });
                                cx.stop_propagation();
                            }),
                        )
                        .on_scroll_wheel(cx.listener(
                            move |this, event: &ScrollWheelEvent, _, cx| {
                                let lines = match event.delta {
                                    ScrollDelta::Lines(lines) => lines.y,
                                    ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / 40.,
                                };
                                this.board.update(cx, |board, cx| {
                                    board.zoom(id, lines);
                                    cx.notify();
                                });
                                cx.stop_propagation();
                            },
                        ))
                        .child(
                            img(pin.image.clone())
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
                        ),
                )
            })
            .collect();

        let menu_el = menu.map(|(_, position)| {
            let menu_size = size(
                px(160.).min(output.size.width),
                px(40.).min(output.size.height),
            );
            let local = position - output.origin;
            let origin = point(
                local.x.max(px(0.)).min(output.size.width - menu_size.width),
                local
                    .y
                    .max(px(0.))
                    .min(output.size.height - menu_size.height),
            );
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
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(ClosePin), cx))
                        .child("Close"),
                )
        });
        // All outputs consume the dismissing click, including outputs without the menu.
        let backdrop = any_menu.then(|| {
            div()
                .id("pin-menu-backdrop")
                .absolute()
                .size_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.board.update(cx, |board, cx| {
                            board.menu = None;
                            cx.notify();
                        });
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _, _, cx| {
                        this.board.update(cx, |board, cx| {
                            board.menu = None;
                            cx.notify();
                        });
                        cx.stop_propagation();
                    }),
                )
        });

        div()
            .id("shotori-pin")
            .overflow_hidden()
            .size_full()
            .track_focus(&self.focus_handle)
            .key_context(if menu.is_some() {
                "ShotoriPinMenu"
            } else {
                "ShotoriPin"
            })
            .on_action(cx.listener(|this, _: &DismissPinMenu, _, cx| {
                this.board.update(cx, |board, cx| {
                    board.menu = None;
                    cx.notify();
                });
            }))
            .on_action(cx.listener(move |this, _: &OpenPinMenu, _, cx| {
                this.board.update(cx, |board, cx| {
                    if let Some(pin) = board.pins.last() {
                        let visible = pin.rect.intersect(&output);
                        if visible.size.width > px(0.) && visible.size.height > px(0.) {
                            board.menu = Some((pin.id, visible.origin));
                            cx.notify();
                        }
                    }
                });
            }))
            .on_action(
                cx.listener(|this, _: &ClosePin, window, cx| this.close_selected(window, cx)),
            )
            .child(sink)
            .children(pins)
            .children(backdrop)
            .children(menu_el)
    }
}

pub(crate) struct PinSpec {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub rect: Bounds<Pixels>,
}

/// Build the scene once. Later requests add to this same scene rather than
/// opening more layer surfaces whose stacking would be compositor-dependent.
fn open_board(spec: PinSpec, cx: &mut App) -> anyhow::Result<Entity<PinBoard>> {
    let outputs = outputs();
    anyhow::ensure!(!outputs.is_empty(), "no outputs registered");
    let board = cx.new(|_| PinBoard::new(outputs.iter().map(|output| output.bounds).collect()));
    board.update(cx, |board, cx| board.add(spec, cx));
    let mut windows: Vec<AnyWindowHandle> = Vec::new();
    for output in outputs {
        let st = board.clone();
        let handle = cx.open_window(
            WindowOptions {
                titlebar: None,
                window_background: WindowBackgroundAppearance::Transparent,
                focus: false,
                display_id: output.display_id,
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    output.bounds.size,
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
            move |window, cx| cx.new(|cx| PinSurface::new(st, output, window, cx)),
        );
        match handle {
            Ok(handle) => windows.push(handle.into()),
            Err(error) => {
                for handle in windows {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
                return Err(error);
            }
        }
    }
    board.update(cx, |board, _| board.windows = windows);
    Ok(board)
}

#[cfg(target_os = "linux")]
mod transport;

/// IPC preparation runs away from the UI thread. The first process retains
/// ownership; other screenshot sessions transfer their pixels and exit.
#[cfg(target_os = "linux")]
pub(crate) use transport::{Prepared, prepare};

#[cfg(not(target_os = "linux"))]
pub(crate) struct Prepared(PinSpec);
#[cfg(not(target_os = "linux"))]
pub(crate) fn prepare(spec: PinSpec) -> anyhow::Result<Prepared> {
    Ok(Prepared(spec))
}

pub(crate) fn open(prepared: Prepared, cx: &mut App) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    match prepared {
        Prepared::Forwarded => Ok(()),
        Prepared::Owner(spec, server) => {
            let requests = server.start()?;
            let board = open_board(spec, cx)?;
            cx.spawn(async move |cx| {
                while let Ok(request) = requests.recv().await {
                    let result = cx.update(|cx| {
                        // Closing the last pin wins over a request still in transit.
                        anyhow::ensure!(
                            !board.read(cx).pins.is_empty(),
                            "pin session is closing; try again"
                        );
                        board.update(cx, |board, cx| board.add(request.spec, cx));
                        Ok(())
                    });
                    let _ = request
                        .reply
                        .send(result.map_err(|error: anyhow::Error| error.to_string()));
                }
            })
            .detach();
            Ok(())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        open_board(prepared.0, cx).map(|_| ())
    }
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
    use super::{PinBoard, PinOutput, PinSpec, PinSurface};
    use gpui_kit::{AppContext, Bounds, Entity, MouseButton, TestAppContext, point, px, size};

    fn spec(x: f32, y: f32) -> PinSpec {
        PinSpec {
            w: 100,
            h: 60,
            rgba: vec![255; 100 * 60 * 4],
            rect: Bounds::new(point(px(x), px(y)), size(px(100.), px(60.))),
        }
    }
    fn board(cx: &mut TestAppContext) -> Entity<PinBoard> {
        cx.update(|cx| {
            gpui_kit::base::init(cx);
            crate::actions::bind_keys(cx);
        });
        cx.new(|_| {
            PinBoard::new(vec![
                Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.))),
                Bounds::new(point(px(200.), px(0.)), size(px(200.), px(200.))),
            ])
        })
    }

    #[gpui_kit::test]
    fn selecting_an_exposed_pin_raises_it_and_overlap_targets_the_new_top(cx: &mut TestAppContext) {
        let board = board(cx);
        board.update(cx, |board, cx| {
            board.add(spec(20., 20.), cx);
            board.add(spec(60., 40.), cx);
        });
        let (_, vcx) = cx.add_window_view(|window, cx| {
            PinSurface::new(
                board.clone(),
                PinOutput {
                    bounds: Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.))),
                    display_id: None,
                },
                window,
                cx,
            )
        });
        vcx.simulate_resize(size(px(200.), px(200.)));
        vcx.update(|window, cx| window.draw(cx).clear(cx));
        vcx.simulate_click(point(px(30.), px(30.)), Default::default());
        vcx.update(|window, cx| {
            assert_eq!(
                board.read(cx).pins.iter().map(|p| p.id).collect::<Vec<_>>(),
                vec![2, 1]
            );
            window.draw(cx).clear(cx);
        });
        // Both images contain this point: only the raised image may grab it.
        vcx.simulate_mouse_down(
            point(px(80.), px(50.)),
            MouseButton::Left,
            Default::default(),
        );
        vcx.simulate_mouse_up(
            point(px(90.), px(60.)),
            MouseButton::Left,
            Default::default(),
        );
        vcx.update(|window, cx| {
            let state = board.read(cx);
            assert_eq!(state.pins[0].rect.origin, point(px(60.), px(40.)));
            assert_eq!(state.pins[1].rect.origin, point(px(30.), px(30.)));
            assert!(state.drag.is_none());
            window.draw(cx).clear(cx);
        });
        vcx.simulate_mouse_down(
            point(px(90.), px(60.)),
            MouseButton::Right,
            Default::default(),
        );
        vcx.update(|window, cx| window.draw(cx).clear(cx));
        vcx.simulate_keystrokes("enter");
        vcx.update(|_, cx| {
            assert_eq!(board.read(cx).pins.len(), 1);
            assert_eq!(board.read(cx).pins[0].id, 2);
        });
        assert_eq!(vcx.windows().len(), 1);
    }

    #[gpui_kit::test]
    fn cross_output_layout_and_order_share_the_same_geometry(cx: &mut TestAppContext) {
        let board = board(cx);
        board.update(cx, |board, cx| {
            board.add(spec(150., 20.), cx);
            board.add(spec(160., 30.), cx);
        });
        let mut other = cx.clone();
        let (_, first) = cx.add_window_view(|window, cx| {
            PinSurface::new(
                board.clone(),
                PinOutput {
                    bounds: Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.))),
                    display_id: None,
                },
                window,
                cx,
            )
        });
        let (_, second) = other.add_window_view(|window, cx| {
            PinSurface::new(
                board.clone(),
                PinOutput {
                    bounds: Bounds::new(point(px(200.), px(0.)), size(px(200.), px(200.))),
                    display_id: None,
                },
                window,
                cx,
            )
        });
        for vcx in [&mut *first, &mut *second] {
            vcx.simulate_resize(size(px(200.), px(200.)));
            vcx.update(|window, cx| window.draw(cx).clear(cx));
        }
        assert_eq!(
            first.debug_bounds("pin-image-1").unwrap().origin,
            point(px(150.), px(20.))
        );
        assert_eq!(
            second.debug_bounds("pin-image-1").unwrap().origin,
            point(px(-50.), px(20.))
        );
        assert_eq!(
            second.debug_bounds("pin-image-1").unwrap().size,
            size(px(100.), px(60.))
        );
        first.simulate_click(point(px(155.), px(25.)), Default::default());
        second.update(|window, cx| window.draw(cx).clear(cx));
        second.simulate_mouse_down(
            point(px(20.), px(50.)),
            MouseButton::Left,
            Default::default(),
        );
        second.update(|_, cx| assert_eq!(board.read(cx).drag.unwrap().0, 1));
        second.simulate_mouse_up(
            point(px(180.), px(150.)),
            MouseButton::Left,
            Default::default(),
        );
        first.update(|window, cx| window.draw(cx).clear(cx));
        assert!(first.debug_bounds("pin-image-1").is_none());
    }

    #[gpui_kit::test]
    fn menu_escape_outside_click_and_close_on_all_outputs(cx: &mut TestAppContext) {
        let board = board(cx);
        board.update(cx, |board, cx| board.add(spec(150., 150.), cx));
        let mut other = cx.clone();
        let (_, first) = cx.add_window_view(|window, cx| {
            PinSurface::new(
                board.clone(),
                PinOutput {
                    bounds: Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.))),
                    display_id: None,
                },
                window,
                cx,
            )
        });
        let (_, second) = other.add_window_view(|window, cx| {
            PinSurface::new(
                board.clone(),
                PinOutput {
                    bounds: Bounds::new(point(px(200.), px(0.)), size(px(200.), px(200.))),
                    display_id: None,
                },
                window,
                cx,
            )
        });
        let mut handles = Vec::new();
        for vcx in [&mut *first, &mut *second] {
            vcx.simulate_resize(size(px(200.), px(200.)));
            handles.push(vcx.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.window_handle()
            }));
        }
        board.update(first, |board, _| board.windows = handles.clone());
        first.simulate_keystrokes("escape");
        assert!(first.windows().contains(&handles[0]));
        first.simulate_mouse_down(
            point(px(190.), px(190.)),
            MouseButton::Right,
            Default::default(),
        );
        first.update(|window, cx| window.draw(cx).clear(cx));
        let menu = first.debug_bounds("pin-menu").unwrap();
        assert_eq!(menu.bottom_right(), point(px(200.), px(200.)));
        first.simulate_keystrokes("escape");
        first.update(|window, cx| window.draw(cx).clear(cx));
        assert!(first.debug_bounds("pin-menu").is_none());
        first.simulate_keystrokes("shift-f10");
        second.update(|window, cx| window.draw(cx).clear(cx));
        second.simulate_click(point(px(180.), px(10.)), Default::default());
        first.update(|window, cx| {
            assert!(board.read(cx).menu.is_none());
            assert!(board.read(cx).drag.is_none());
            window.draw(cx).clear(cx);
        });
        first.simulate_keystrokes("shift-f10");
        first.update(|window, cx| window.draw(cx).clear(cx));
        let button = first.debug_bounds("pin-close").unwrap();
        first.simulate_click(button.center(), Default::default());
        first.run_until_parked();
        for handle in handles {
            assert!(!first.windows().contains(&handle));
        }
    }

    #[gpui_kit::test]
    fn shrinking_at_the_top_left_remains_grabbable(cx: &mut TestAppContext) {
        let board = board(cx);
        board.update(cx, |board, cx| {
            board.add(spec(-100., -100.), cx);
            for _ in 0..20 {
                board.zoom(1, -2.);
                let rect = board.pins[0].rect;
                let visible = rect.intersect(&board.outputs[0]);
                assert!(visible.size.width >= rect.size.width.min(px(super::MIN_VISIBLE)));
                assert!(visible.size.height >= rect.size.height.min(px(super::MIN_VISIBLE)));
            }
        });
    }
}
