//! Shared screenshot selection and captures for every overlay in one session.
use std::sync::Arc;

use gpui_kit::*;

use crate::{model::selection::Selection, platform::capture::Capture};

struct Screen {
    capture: Arc<Capture>,
    logical_size: Size<Pixels>,
}

impl Screen {
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: point(
                px(self.capture.logical_pos.0 as f32),
                px(self.capture.logical_pos.1 as f32),
            ),
            size: self.logical_size,
        }
    }
}

struct RasterSelection {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    bounds: Bounds<Pixels>,
}

struct FilterPreview {
    selection: Option<Bounds<Pixels>>,
    shapes: Vec<crate::annotation::Shape>,
    bounds: Bounds<Pixels>,
    image: Arc<RenderImage>,
}

/// An in-flight toolbar drag: `grab` is the press offset from the
/// toolbar's origin (press − origin, window-local) so the toolbar
/// follows the pointer without jumping; `restore` is the pre-drag
/// position override (None = anchored) for Esc.
#[derive(Clone, Copy)]
struct ToolbarDrag {
    grab: Point<Pixels>,
    restore: Option<Point<Pixels>>,
}

pub struct ScreenshotSession {
    screens: Vec<Screen>,
    selection: Selection,
    active_output: Option<String>,
    /// The pointer's last known position in GLOBAL desktop coordinates,
    /// reported by whichever window actually receives its events. Under
    /// Wayland's implicit grab a gesture's events all go to the PRESS
    /// window — the window the pointer physically sits over may have
    /// never seen a single move. Cursor affordances therefore read THIS
    /// (mapped into their own window), not a per-window position, or a
    /// state flip that lands chrome under a "fresh" window shows a
    /// stale cursor until the user wiggles the mouse.
    pointer_global: Option<Point<Pixels>>,
    blocked: bool,
    /// The user-dragged toolbar position (window-local to the ACTIVE
    /// output); None → the placement anchor decides. Reset by a NEW
    /// selection or a change of host window — moving/resizing the
    /// current selection keeps it (the user put it there deliberately).
    toolbar_pos: Option<Point<Pixels>>,
    /// Set while the toolbar is being dragged by an edge grip.
    toolbar_drag: Option<ToolbarDrag>,
    /// Window-snap targets in global logical coordinates; empty when the
    /// compositor exposes no supported IPC — see [`crate::platform::windowsnap`]
    snaps: Vec<crate::platform::windowsnap::SnapRect>,
    /// Index into `snaps`: the window under the cursor (hover outline)
    hovered: Option<usize>,
    /// Press point of the ongoing interaction (global). The click-snap
    /// hit-tests the PRESS position, not wherever a jittery release lands
    press: Option<Point<Pixels>>,
    annotations: crate::annotation::Annotations,
    filter_preview: std::cell::RefCell<Option<FilterPreview>>,
}

impl ScreenshotSession {
    pub fn new(
        captures: Vec<Arc<Capture>>,
        snaps: Vec<crate::platform::windowsnap::SnapRect>,
    ) -> Self {
        Self {
            screens: captures
                .into_iter()
                .map(|capture| Screen {
                    logical_size: {
                        let (w, h) = capture.logical_size_f32();
                        size(px(w), px(h))
                    },
                    capture,
                })
                .collect(),
            selection: Selection::Idle,
            active_output: None,
            pointer_global: None,
            blocked: false,
            toolbar_pos: None,
            toolbar_drag: None,
            snaps,
            hovered: None,
            press: None,
            annotations: Default::default(),
            filter_preview: Default::default(),
        }
    }

    fn screen(&self, name: &str) -> &Screen {
        self.screens
            .iter()
            .find(|s| s.capture.output_name == name)
            .expect("registered overlay output")
    }

    /// Full-screen selection: the union of every screen's bounds — the
    /// `full` subcommand's non-interactive path. A cross-screen union
    /// exports at the highest participating density with transparent
    /// gaps, exactly like a user-drawn spanning selection.
    pub fn select_all(&mut self) {
        if let Some(bounds) = self
            .screens
            .iter()
            .map(|s| s.bounds())
            .reduce(|a, b| a.union(&b))
        {
            let first = self
                .screens
                .first()
                .map(|s| s.capture.output_name.clone())
                .unwrap_or_default();
            self.set_active_output(&first);
            self.selection = Selection::Selected { bounds };
        }
    }

    /// Ctrl+A: cycle the selection between "this whole screen" and
    /// "every screen". Any state (idle, partial drag, another screen's
    /// selection) lands on the current screen first — that reads as
    /// "select all" to a user looking at one monitor — the second press
    /// spans everything, the third wraps back. Returns whether the
    /// selection changed.
    pub(crate) fn cycle_select_all(&mut self, name: &str) -> bool {
        let Some(screen_bounds) = self
            .screens
            .iter()
            .find(|s| s.capture.output_name == name)
            .map(|s| s.bounds())
        else {
            return false; // unregistered output — overlay contract broken
        };
        let Some(union) = self
            .screens
            .iter()
            .map(|s| s.bounds())
            .reduce(|a, b| a.union(&b))
        else {
            return false;
        };
        let current = self.selection.bounds();
        let next = if current == Some(union) {
            screen_bounds // all → this screen (wrap)
        } else if current == Some(screen_bounds) {
            union // this screen → all
        } else {
            screen_bounds // idle / partial / elsewhere → this screen
        };
        self.toolbar_pos = None; // the selection was replaced: re-anchor
        self.set_active_output(name);
        self.selection = Selection::Selected { bounds: next };
        true
    }

    pub(crate) fn set_size(&mut self, name: &str, logical_size: Size<Pixels>) -> bool {
        if logical_size.width <= px(0.) || logical_size.height <= px(0.) {
            return false;
        }
        let screen = self
            .screens
            .iter_mut()
            .find(|s| s.capture.output_name == name)
            .unwrap();
        if screen.logical_size == logical_size {
            return false;
        }
        screen.logical_size = logical_size;
        self.filter_preview.get_mut().take();
        true
    }

    pub(crate) fn selection(&self) -> Selection {
        self.selection
    }
    pub(crate) fn blocked(&self) -> bool {
        self.blocked
    }
    pub(crate) fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }
    pub(crate) fn active_on(&self, name: &str) -> bool {
        self.active_output.as_deref() == Some(name)
    }

    /// Change the host window of the toolbar-carrying selection. A
    /// dragged toolbar position is LOCAL to its host window — a new host
    /// must re-anchor (the same coordinates would mean somewhere else
    /// entirely on another screen).
    fn set_active_output(&mut self, name: &str) {
        if self.active_output.as_deref() != Some(name) {
            self.toolbar_pos = None;
        }
        self.active_output = Some(name.to_owned());
    }

    /// A finalized selection must carry its chrome with it. The size
    /// label and toolbar render only on the active output — but a
    /// move/resize edit (or a fresh drag) released with the selection
    /// living on a DIFFERENT output leaves `active_output` stale:
    /// Wayland's implicit grab delivers the whole gesture to the window
    /// where the press happened, and only `pointer_down` re-hosts. The
    /// result was both screens blank (the old one no longer intersects
    /// the selection, the new one is not "active") until the next click
    /// bailed it out. Follow the selection to the output holding its
    /// largest intersection. STICKY: the incumbent host wins ties — an
    /// ambiguous straddle across the seam must not churn the chrome to
    /// the other window (and a same-host call is a no-op, keeping any
    /// dragged toolbar position).
    fn follow_selection_host(&mut self) {
        let Some(bounds) = self.selection.bounds() else {
            return; // no finalized selection: nothing to follow
        };
        let overlap = |screen: &Screen| {
            let o = screen.bounds().intersect(&bounds);
            f32::from(o.size.width) * f32::from(o.size.height)
        };
        let incumbent = self
            .active_output
            .as_deref()
            .and_then(|name| self.screens.iter().find(|s| s.capture.output_name == name))
            .map(overlap)
            .unwrap_or(0.);
        // only a STRICTLY larger intersection dethrones the incumbent
        let challenger = self
            .screens
            .iter()
            .filter(|s| overlap(s) > incumbent)
            .map(|s| (s.capture.output_name.clone(), overlap(s)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        if let Some((name, _)) = challenger {
            self.set_active_output(&name);
        }
    }

    pub(crate) fn local_bounds(&self, name: &str) -> Option<Bounds<Pixels>> {
        let screen = self.screen(name).bounds();
        let mut bounds = self.selection.bounds()?.intersect(&screen);
        if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
            return None;
        }
        bounds.origin -= screen.origin;
        Some(bounds)
    }

    pub(crate) fn backdrop_bounds(&self, name: &str) -> Option<Bounds<Pixels>> {
        self.local_bounds(name)?;
        let mut bounds = self.selection.bounds()?;
        bounds.origin -= self.screen(name).bounds().origin;
        Some(bounds)
    }

    pub(crate) fn begin(&mut self, name: &str, local: Point<Pixels>) {
        if self.blocked {
            return;
        }
        let global = local + self.screen(name).bounds().origin;
        self.press = Some(global);
        self.hovered = None; // the outline steps aside for the real interaction
        self.annotations.reset();
        self.toolbar_pos = None; // a NEW selection re-anchors the toolbar
        self.set_active_output(name);
        self.selection.begin(global);
    }

    /// The union of every screen's bounds — the "desktop" a selection can
    /// occupy. Move/resize clamps to it (a selection cannot leave the
    /// captured area).
    fn desktop_bounds(&self) -> Option<Bounds<Pixels>> {
        self.screens
            .iter()
            .map(|s| s.bounds())
            .reduce(|a, b| a.union(&b))
    }

    /// Window-local → global logical coordinates for one output.
    pub(crate) fn to_global(&self, name: &str, local: Point<Pixels>) -> Point<Pixels> {
        local + self.screen(name).bounds().origin
    }

    /// The pointer's last known global position — see the field docs.
    pub(crate) fn pointer_global(&self) -> Option<Point<Pixels>> {
        self.pointer_global
    }

    /// That position mapped into this output's LOCAL coordinates
    /// (unclamped: the pointer may legitimately sit over another
    /// output, in which case the mapped point lies outside this
    /// window — hit-tests simply miss).
    pub(crate) fn pointer_in(&self, name: &str) -> Option<Point<Pixels>> {
        Some(self.pointer_global? - self.screen(name).bounds().origin)
    }

    /// This output's overlay window size (logical px), as last reported by
    /// [`ScreenshotSession::set_size`]. Chrome geometry (toolbar anchor)
    /// needs it outside render — e.g. the cursor's toolbar hit-test.
    pub(crate) fn overlay_size(&self, name: &str) -> Option<Size<Pixels>> {
        self.screens
            .iter()
            .find(|s| s.capture.output_name == name)
            .map(|s| s.logical_size)
            .filter(|s| s.width > px(0.) && s.height > px(0.))
    }

    // ── Toolbar geometry + dragging ────────────────────────────────
    // One source of truth: render, the cursor hit-test and the drag
    // clamp all read THIS, so the three cannot drift apart.

    /// The toolbar's rect in this output's LOCAL coordinates — the
    /// placement anchor, or wherever the user last dragged it (clamped
    /// inside the window). None when this output hosts no toolbar.
    pub(crate) fn toolbar_bounds(&self, name: &str) -> Option<Bounds<Pixels>> {
        if !self.selection.is_selected() || !self.active_on(name) {
            return None;
        }
        let ws = self.overlay_size(name)?;
        let sel = self
            .local_bounds(name)
            .map(crate::model::placement::round_px)?;
        let height = if self.annotations.enabled() {
            crate::model::placement::TB_H
        } else {
            crate::model::placement::ROW_H
        };
        let mut b = crate::model::placement::toolbar_bounds(&sel, ws, height);
        if let Some(pos) = self.toolbar_pos {
            b.origin = point(
                px(f32::from(pos.x).clamp(
                    8.,
                    (f32::from(ws.width) - f32::from(b.size.width) - 8.).max(8.),
                )),
                px(f32::from(pos.y).clamp(
                    8.,
                    (f32::from(ws.height) - f32::from(b.size.height) - 8.).max(8.),
                )),
            );
        }
        Some(b)
    }

    /// The left/right drag-grip strips (local coords) — for the cursor's
    /// grab affordance and nothing else; the elements themselves live in
    /// `ui::toolbar`. ROW ONE ONLY (the settings row does not grab), and
    /// inset by the bar's padding — this is the grip ELEMENT's exact
    /// rect, so the hand cursor and the drag trigger coincide
    /// pixel-for-pixel (a mismatch in either axis shows up instantly as
    /// "draggable but not a hand" or vice versa — user-reported).
    pub(crate) fn toolbar_grips(&self, name: &str) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        let b = self.toolbar_bounds(name)?;
        let (w, h) = (
            px(crate::model::placement::GRIP_W),
            px(crate::model::placement::ROW_H),
        );
        let pad = px(crate::model::placement::BAR_PAD);
        Some((
            Bounds {
                origin: point(b.origin.x + pad, b.origin.y),
                size: size(w, h),
            },
            Bounds {
                origin: point(b.right() - pad - w, b.origin.y),
                size: size(w, h),
            },
        ))
    }

    /// Press on an edge grip: start dragging the toolbar. `press` is in
    /// the host window's local coordinates. Returns false when there is
    /// no draggable toolbar here (or a modal owns the session).
    pub(crate) fn toolbar_drag_begin(&mut self, name: &str, press: Point<Pixels>) -> bool {
        if self.blocked {
            return false;
        }
        let Some(b) = self.toolbar_bounds(name) else {
            return false;
        };
        self.pointer_global = Some(self.to_global(name, press));
        self.toolbar_drag = Some(ToolbarDrag {
            grab: press - b.origin,
            restore: self.toolbar_pos,
        });
        true
    }

    /// Drag the toolbar under the pointer (window-local coords),
    /// clamped inside the window — the toolbar cannot leave its layer.
    /// Returns whether the position changed (callers decide on notify).
    pub(crate) fn toolbar_drag_move(&mut self, name: &str, local: Point<Pixels>) -> bool {
        let Some(grab) = self.toolbar_drag.map(|d| d.grab) else {
            return false;
        };
        let Some(ws) = self.overlay_size(name) else {
            return false;
        };
        self.pointer_global = Some(self.to_global(name, local));
        let height = if self.annotations.enabled() {
            crate::model::placement::TB_H
        } else {
            crate::model::placement::ROW_H
        };
        let w = crate::model::placement::TB_W.min((f32::from(ws.width) - 16.).max(1.));
        let next = point(
            px((f32::from(local.x) - f32::from(grab.x))
                .clamp(8., (f32::from(ws.width) - w - 8.).max(8.))),
            px((f32::from(local.y) - f32::from(grab.y))
                .clamp(8., (f32::from(ws.height) - height - 8.).max(8.))),
        );
        if self.toolbar_pos == Some(next) {
            return false;
        }
        self.toolbar_pos = Some(next);
        true
    }

    /// Release: the dragged position becomes the toolbar's new home.
    pub(crate) fn toolbar_drag_end(&mut self) {
        self.toolbar_drag = None;
    }

    /// A grip drag is in flight?
    pub(crate) fn toolbar_drag_active(&self) -> bool {
        self.toolbar_drag.is_some()
    }

    pub(crate) fn drag_to(&mut self, name: &str, local: Point<Pixels>) -> bool {
        if self.blocked {
            return false;
        }
        self.selection
            .drag_to(local + self.screen(name).bounds().origin)
    }

    pub(crate) fn end(&mut self, name: &str, local: Point<Pixels>) {
        if self.blocked {
            return;
        }
        self.selection
            .end(local + self.screen(name).bounds().origin);
        // An in-place click (< selection::MIN_SIZE) leaves Idle — if a
        // window sits under the PRESS point, select its rect instead
        // (window snapping). A real drag still wins: freehand beats snap.
        if !self.selection.is_dragging()
            && !self.selection.is_selected()
            && let Some(press) = self.press
            && let Some(hit) = crate::platform::windowsnap::hit_test(&self.snaps, press)
        {
            self.selection = Selection::Selected {
                bounds: self.snaps[hit].bounds,
            };
        }
        self.press = None;
        // A fresh drag can finish over the seam on another output — the
        // chrome host must follow the selection there, exactly like an
        // edit-release does (see `follow_selection_host`).
        self.follow_selection_host();
    }

    /// Track the window under the cursor for the hover outline. Returns
    /// whether the hover changed (callers decide on cx.notify()).
    /// Suppressed while dragging — the freehand region takes over — and
    /// while a modal is open.
    pub(crate) fn hover_at(&mut self, name: &str, local: Point<Pixels>) -> bool {
        if self.blocked
            || self.selection.is_dragging()
            || self.selection.is_editing()
            || self.toolbar_drag.is_some()
            || self.snaps.is_empty()
            || self.annotations.enabled()
        {
            return false;
        }
        let global = local + self.screen(name).bounds().origin;
        let hit = crate::platform::windowsnap::hit_test(&self.snaps, global);
        if hit == self.hovered {
            return false;
        }
        self.hovered = hit;
        true
    }

    /// The hovered window's rect in this output's local coordinates, for
    /// the hover outline; None when not hovering anything visible here.
    pub(crate) fn hover_bounds(&self, name: &str) -> Option<Bounds<Pixels>> {
        if self.blocked {
            return None;
        }
        let screen = self.screen(name).bounds();
        let mut b = self.snaps.get(self.hovered?)?.bounds.intersect(&screen);
        if b.size.width <= px(0.) || b.size.height <= px(0.) {
            return None;
        }
        b.origin -= screen.origin;
        Some(b)
    }

    pub(crate) fn cancel_drag(&mut self) {
        if let Some(drag) = self.toolbar_drag.take() {
            // Esc mid-toolbar-drag: back to where it was. One Esc, one
            // thing — the selection below is untouched.
            self.toolbar_pos = drag.restore;
            return;
        }
        self.press = None;
        self.hovered = None;
        self.selection.cancel_drag();
    }

    pub(crate) fn annotations(&self) -> &crate::annotation::Annotations {
        &self.annotations
    }

    pub(crate) fn edit_annotations(
        &mut self,
        edit: impl FnOnce(&mut crate::annotation::Annotations),
    ) {
        if !self.blocked && self.selection.is_selected() {
            edit(&mut self.annotations);
        }
    }

    pub(crate) fn edit_annotation_settings(
        &mut self,
        edit: impl FnOnce(&mut crate::annotation::Annotations),
    ) {
        if self.annotations.has_text_preview() {
            edit(&mut self.annotations);
        } else {
            self.edit_annotations(edit);
        }
    }

    pub(crate) fn preview_text(&mut self, bounds: Bounds<Pixels>, value: String) {
        self.annotations.preview_text(bounds, value);
    }
    pub(crate) fn clear_text_preview(&mut self) {
        self.annotations.clear_text_preview();
    }

    pub(crate) fn text_bounds(&self, output: &str, local: Point<Pixels>) -> Option<Bounds<Pixels>> {
        if self.blocked || !self.selection.is_selected() {
            return None;
        }
        let bounds = self.selection.bounds()?;
        let origin = local + self.screen(output).bounds().origin;
        if !bounds.contains(&origin) {
            return None;
        }
        let available = Bounds::from_corners(origin, bounds.bottom_right());
        (available.size.width >= px(16.) && available.size.height >= px(16.)).then_some(available)
    }

    pub(crate) fn pointer_down(&mut self, name: &str, local: Point<Pixels>) {
        if self.blocked {
            return;
        }
        self.pointer_global = Some(self.to_global(name, local));
        if self.annotations.enabled() {
            if let Some(selection) = self.selection.bounds() {
                self.annotations
                    .begin(local + self.screen(name).bounds().origin, selection);
            }
        } else {
            // A finalized selection is editable in place: an edge/corner
            // band grabs a resize handle, the interior starts a move. Only
            // a press OUTSIDE starts a fresh selection (which also clears
            // the annotations — an edit must not).
            if self
                .selection
                .begin_edit(local + self.screen(name).bounds().origin)
            {
                self.hovered = None;
                self.set_active_output(name);
                return;
            }
            self.begin(name, local);
        }
    }

    pub(crate) fn pointer_move(&mut self, name: &str, local: Point<Pixels>, square: bool) -> bool {
        if self.blocked {
            return false;
        }
        self.pointer_global = Some(self.to_global(name, local));
        if self.annotations.enabled() {
            if let Some(selection) = self.selection.bounds() {
                return self.annotations.drag_to(
                    local + self.screen(name).bounds().origin,
                    selection,
                    square,
                );
            }
            false
        } else if self.selection.is_editing() {
            let Some(desktop) = self.desktop_bounds() else {
                return false;
            };
            self.selection
                .edit_to(local + self.screen(name).bounds().origin, desktop)
        } else {
            self.drag_to(name, local)
        }
    }

    pub(crate) fn pointer_up(&mut self, name: &str, local: Point<Pixels>, square: bool) {
        if self.blocked {
            return;
        }
        self.pointer_global = Some(self.to_global(name, local));
        if self.annotations.enabled() {
            self.pointer_move(name, local, square);
            self.annotations.end();
        } else if self.selection.is_editing() {
            self.selection.end_edit();
            self.follow_selection_host();
            self.press = None;
        } else {
            self.end(name, local);
        }
    }

    pub(crate) fn cancel_annotation(&mut self) -> bool {
        !self.blocked && self.annotations.cancel()
    }

    pub(crate) fn local_annotations(&self, name: &str) -> Vec<crate::annotation::Shape> {
        let origin = self.screen(name).bounds().origin;
        self.annotations
            .visible()
            .cloned()
            .map(|mut shape| {
                shape.bounds.origin -= origin;
                for point in &mut shape.points {
                    *point -= origin;
                }
                shape
            })
            .collect()
    }

    pub(crate) fn crop(&self, output: &str) -> Option<(u32, u32, Vec<u8>)> {
        self.crop_impl(output, true)
            .map(|r| (r.width, r.height, r.rgba))
    }
    /// Export without annotations — also the `full` subcommand's path
    pub fn crop_original(&self, output: &str) -> Option<(u32, u32, Vec<u8>)> {
        self.crop_impl(output, false)
            .map(|r| (r.width, r.height, r.rgba))
    }

    /// Reuse the exported composite on every output whenever pixel filters or text are present.
    /// Captures are immutable; selection, shapes and display geometry own invalidation.
    pub(crate) fn filtered_preview(
        &self,
        output: &str,
    ) -> Option<(Bounds<Pixels>, Arc<RenderImage>)> {
        use crate::annotation::ShapeKind;
        let mut cache = self.filter_preview.borrow_mut();
        if !self.annotations.visible().any(|s| {
            matches!(
                s.kind,
                ShapeKind::Mosaic
                    | ShapeKind::Blur
                    | ShapeKind::Text
                    | ShapeKind::Eraser
                    | ShapeKind::EraserRect
            )
        }) {
            *cache = None;
            return None;
        }
        let shapes: Vec<_> = self.annotations.visible().cloned().collect();
        let selection = self.selection.bounds();
        if !cache
            .as_ref()
            .is_some_and(|c| c.selection == selection && c.shapes == shapes)
        {
            let raster = self.crop_impl(output, true)?;
            *cache = Some(FilterPreview {
                selection,
                shapes,
                bounds: raster.bounds,
                image: crate::ui::image_util::rgba_to_render_image(
                    raster.rgba,
                    raster.width,
                    raster.height,
                ),
            });
        }
        let cached = cache.as_ref()?;
        let mut bounds = cached.bounds;
        bounds.origin -= self.screen(output).bounds().origin;
        Some((bounds, cached.image.clone()))
    }

    /// Keep the original single-output crop when possible. Spanning selections
    /// use the highest participating pixel density; desktop gaps stay transparent.
    fn crop_impl(&self, fallback_output: &str, marked: bool) -> Option<RasterSelection> {
        let selected = self
            .selection
            .bounds()
            .unwrap_or_else(|| self.screen(fallback_output).bounds());
        let participating: Vec<_> = self
            .screens
            .iter()
            .filter_map(|screen| {
                let intersection = selected.intersect(&screen.bounds());
                (intersection.size.width > px(0.) && intersection.size.height > px(0.))
                    .then_some((screen, intersection))
            })
            .collect();
        if participating.len() == 1 {
            let (screen, mut bounds) = participating[0];
            bounds.origin -= screen.bounds().origin;
            let cap = &screen.capture;
            let scale = cap.width as f32 / f32::from(screen.logical_size.width);
            let (w, h, mut rgba) =
                crate::model::export::crop(&cap.rgba, cap.width, cap.height, bounds, scale)?;
            // crop() rounds the source offset to native pixels. Use the
            // same rounded origin when placing logical annotation edges.
            let origin = screen.bounds().origin
                + point(
                    px((f32::from(bounds.left()) * scale).round() / scale),
                    px((f32::from(bounds.top()) * scale).round() / scale),
                );
            if marked {
                self.annotations.rasterize(&mut rgba, w, h, origin, scale);
            }
            return Some(RasterSelection {
                width: w,
                height: h,
                rgba,
                bounds: Bounds::new(origin, size(px(w as f32 / scale), px(h as f32 / scale))),
            });
        }
        if participating.is_empty() {
            return None;
        }
        let extent = participating
            .iter()
            .map(|(_, b)| *b)
            .reduce(|a, b| a.union(&b))?;
        let scale = participating
            .iter()
            .map(|(s, _)| s.capture.width as f32 / f32::from(s.logical_size.width))
            .fold(0., f32::max);
        let w = (f32::from(extent.size.width) * scale).round() as u32;
        let h = (f32::from(extent.size.height) * scale).round() as u32;
        if w == 0 || h == 0 {
            return None;
        }
        let mut out = image::RgbaImage::new(w, h);
        for (screen, intersection) in participating {
            let mut local = intersection;
            local.origin -= screen.bounds().origin;
            let cap = &screen.capture;
            let (cw, ch, pixels) = crate::model::export::crop(
                &cap.rgba,
                cap.width,
                cap.height,
                local,
                cap.width as f32 / f32::from(screen.logical_size.width),
            )?;
            let image = image::RgbaImage::from_raw(cw, ch, pixels)?;
            let x = (f32::from(intersection.left() - extent.left()) * scale).round() as u32;
            let y = (f32::from(intersection.top() - extent.top()) * scale).round() as u32;
            let right = (f32::from(intersection.right() - extent.left()) * scale).round() as u32;
            let bottom = (f32::from(intersection.bottom() - extent.top()) * scale).round() as u32;
            let image = image::imageops::resize(
                &image,
                right - x,
                bottom - y,
                image::imageops::FilterType::Triangle,
            );
            image::imageops::replace(&mut out, &image, x.into(), y.into());
        }
        let mut rgba = out.into_raw();
        if marked {
            self.annotations
                .rasterize(&mut rgba, w, h, extent.origin, scale);
        }
        Some(RasterSelection {
            width: w,
            height: h,
            rgba,
            bounds: Bounds::new(
                extent.origin,
                size(px(w as f32 / scale), px(h as f32 / scale)),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ScreenshotSession;
    use crate::model::selection::Selection;
    use crate::platform::capture::Capture;
    use gpui_kit::{Bounds, point, px, size};
    use std::sync::Arc;

    fn screen(name: &str, pos: (i32, i32), scale: f32, color: [u8; 4]) -> Arc<Capture> {
        let mut cap = Capture::for_test(pos, scale);
        cap.output_name = name.into();
        cap.width = (100. * scale) as u32;
        cap.height = (100. * scale) as u32;
        cap.rgba = color.repeat((cap.width * cap.height) as usize);
        Arc::new(cap)
    }

    fn session() -> ScreenshotSession {
        ScreenshotSession::new(
            vec![
                screen("left", (-100, 20), 1., [255, 0, 0, 255]),
                screen("right", (0, 0), 2., [0, 255, 0, 255]),
            ],
            Vec::new(),
        )
    }

    fn snap(x: f32, y: f32, w: f32, h: f32) -> crate::platform::windowsnap::SnapRect {
        crate::platform::windowsnap::SnapRect {
            bounds: Bounds {
                origin: point(px(x), px(y)),
                size: size(px(w), px(h)),
            },
            app_id: "fixture".into(),
            focused: false,
            recency: 0,
        }
    }

    #[test]
    fn ctrl_a_cycles_screen_then_union_then_wraps() {
        let mut s = session();
        let right = s.screens[1].bounds(); // "right" is the fixture's focused-ish screen
        let union = s
            .screens
            .iter()
            .map(|sc| sc.bounds())
            .reduce(|a, b| a.union(&b))
            .unwrap();

        // idle → this screen
        assert!(s.cycle_select_all("right"));
        assert_eq!(s.selection().bounds(), Some(right));
        // this screen → every screen
        assert!(s.cycle_select_all("right"));
        assert_eq!(s.selection().bounds(), Some(union));
        // all → wraps back to this screen
        assert!(s.cycle_select_all("right"));
        assert_eq!(s.selection().bounds(), Some(right));
    }

    #[test]
    fn ctrl_a_from_a_partial_selection_lands_on_the_whole_screen() {
        let mut s = session();
        // a user-drawn partial rectangle somewhere else
        s.selection = Selection::Selected {
            bounds: Bounds {
                origin: point(px(-40.), px(60.)),
                size: size(px(100.), px(50.)),
            },
        };
        assert!(s.cycle_select_all("left"));
        assert_eq!(s.selection().bounds(), Some(s.screens[0].bounds()));
        // and anchors the active output
        assert_eq!(s.active_output.as_deref(), Some("left"));
    }

    #[test]
    fn ctrl_a_on_an_unknown_output_is_a_no_op() {
        let mut s = session();
        assert!(!s.cycle_select_all("nope"));
        assert!(s.selection().bounds().is_none());
    }

    #[test]
    fn select_all_spans_every_screen_with_a_ready_selection() {
        let mut s = session();
        assert!(s.selection().bounds().is_none());
        s.select_all();
        let expected = s
            .screens
            .iter()
            .map(|sc| sc.bounds())
            .reduce(|a, b| a.union(&b))
            .unwrap();
        assert_eq!(s.selection().bounds(), Some(expected));
        // And it must rasterize through the normal export path
        let (w, h, _) = s.crop_original("right").unwrap();
        assert!(w > 0 && h > 0);
    }

    #[test]
    fn selecting_another_output_replaces_the_previous_selection() {
        let mut s = session();
        s.begin("left", point(px(10.), px(10.)));
        s.end("left", point(px(30.), px(30.)));
        assert!(s.local_bounds("left").is_some());
        s.begin("right", point(px(10.), px(10.)));
        s.end("right", point(px(30.), px(30.)));
        assert!(s.local_bounds("left").is_none());
        assert!(s.local_bounds("right").is_some());
        assert!(!s.active_on("left"));
        // A shortcut delivered to the old window still exports the new selection.
        let (w, h, pixels) = s.crop("left").unwrap();
        assert_eq!((w, h), (40, 40));
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 255, 0, 255])
        );
    }

    #[test]
    fn cross_output_drag_normalizes_negative_origins_and_mixed_dpi() {
        let mut s = session();
        s.begin("right", point(px(20.), px(60.)));
        assert!(s.drag_to("left", point(px(80.), px(20.))));
        s.end("left", point(px(80.), px(20.)));
        let bounds = s.selection().bounds().unwrap();
        assert_eq!(bounds.origin, point(px(-20.), px(40.)));
        assert_eq!(bounds.size, size(px(40.), px(20.)));
        let (w, h, pixels) = s.crop("left").unwrap();
        assert_eq!((w, h), (80, 40));
        for row in pixels.chunks_exact(w as usize * 4) {
            assert!(
                row[..40 * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| *p == [255, 0, 0, 255])
            );
            assert!(
                row[40 * 4..]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| *p == [0, 255, 0, 255])
            );
        }
    }

    #[test]
    fn drag_grab_can_finish_outside_its_original_window() {
        let mut s = session();
        s.begin("left", point(px(80.), px(20.)));
        s.end("left", point(px(120.), px(40.)));
        assert!(s.selection.is_selected());
        assert!(s.local_bounds("right").is_some());
        assert_eq!(s.crop("left").unwrap().0, 80);
    }

    #[test]
    fn chrome_follows_a_selection_moved_to_another_output() {
        let mut s = session();
        s.set_size("left", size(px(100.), px(100.)));
        s.set_size("right", size(px(100.), px(100.)));
        // a selection fully on "right", made there: chrome lives there
        s.begin("right", point(px(10.), px(10.)));
        s.end("right", point(px(60.), px(60.)));
        assert!(s.active_on("right"));
        assert!(s.toolbar_bounds("right").is_some());

        // move it across the seam: press inside on right's window, drag
        // and release with the pointer already on "left" — Wayland's
        // implicit grab delivers the WHOLE gesture to the press window
        s.pointer_down("right", point(px(30.), px(30.)));
        assert!(s.selection.is_editing());
        s.pointer_move("right", point(px(-40.), px(50.)), false);
        s.pointer_up("right", point(px(-40.), px(50.)), false);

        // regression: without the rehost both screens rendered nothing
        // (old host no longer intersects, new host not "active") until
        // the next click re-hosted via pointer_down
        assert!(s.selection.is_selected());
        assert!(s.local_bounds("left").is_some(), "label render input");
        assert!(s.active_on("left"), "the chrome host follows the selection");
        assert!(
            s.toolbar_bounds("left").is_some(),
            "toolbar re-hosts on release"
        );
        assert!(s.toolbar_bounds("right").is_none());

        // a FRESH drag released with the bulk on another output rehosts
        // too (press screen ≠ host screen from the start)
        s.begin("right", point(px(5.), px(40.)));
        s.drag_to("right", point(px(-50.), px(60.)));
        s.end("right", point(px(-50.), px(60.)));
        assert!(s.active_on("left"), "largest-intersection output wins");
        assert!(s.toolbar_bounds("left").is_some());
    }

    #[test]
    fn desktop_gaps_are_transparent_and_fractional_scale_uses_window_size() {
        let mut s = ScreenshotSession::new(
            vec![
                screen("top", (0, 0), 1.25, [255, 0, 0, 255]),
                screen("bottom", (0, 120), 1.5, [0, 255, 0, 255]),
            ],
            Vec::new(),
        );
        s.set_size("top", size(px(100.), px(100.)));
        s.set_size("bottom", size(px(100.), px(100.)));
        s.begin("top", point(px(10.), px(90.)));
        s.end("bottom", point(px(30.), px(10.)));
        let (w, h, pixels) = s.crop("top").unwrap();
        assert_eq!((w, h), (30, 60));
        assert!(pixels[15 * 30 * 4..45 * 30 * 4].iter().all(|b| *b == 0));
    }

    #[test]
    fn modal_state_blocks_all_outputs_and_cancel_is_shared() {
        let mut s = session();
        s.begin("left", point(px(10.), px(10.)));
        s.drag_to("left", point(px(40.), px(40.)));
        s.set_blocked(true);
        s.begin("right", point(px(20.), px(20.)));
        assert!(!s.drag_to("right", point(px(70.), px(70.))));
        s.end("right", point(px(70.), px(70.)));
        assert!(s.active_on("left"));
        assert!(s.selection().is_dragging());
        s.set_blocked(false);
        s.cancel_drag();
        assert!(s.local_bounds("left").is_none());
        assert!(s.local_bounds("right").is_none());
    }

    // ── Window snapping ────────────────────────────────────────────

    /// "right" spans global (0,0)-(100,100) logical; one snap window at
    /// (20,30) sized 40x50 sits on it.
    fn snapped_session() -> ScreenshotSession {
        ScreenshotSession::new(
            vec![screen("right", (0, 0), 2., [0, 255, 0, 255])],
            vec![snap(20., 30., 40., 50.)],
        )
    }

    #[test]
    fn click_on_a_window_snaps_the_selection_to_its_rect() {
        let mut s = snapped_session();
        s.begin("right", point(px(50.), px(50.)));
        s.end("right", point(px(51.), px(51.))); // < 2px: a click, not a drag
        assert!(s.selection().is_selected());
        let b = s.selection().bounds().unwrap();
        assert_eq!(b.origin, point(px(20.), px(30.)));
        assert_eq!(b.size, size(px(40.), px(50.)));
        // crop lands in physical pixels (scale 2)
        assert_eq!(s.crop("right").unwrap().0, 80);
    }

    #[test]
    fn click_off_windows_still_clears() {
        let mut s = snapped_session();
        s.begin("right", point(px(5.), px(5.))); // click outside every window
        s.end("right", point(px(6.), px(6.)));
        assert!(!s.selection().is_selected());
    }

    #[test]
    fn real_drag_over_a_window_still_wins() {
        let mut s = snapped_session();
        s.begin("right", point(px(50.), px(50.))); // inside the window
        s.end("right", point(px(90.), px(90.))); // real drag: freehand beats snap
        let b = s.selection().bounds().unwrap();
        assert_eq!(b.origin, point(px(50.), px(50.)));
        assert_eq!(b.size, size(px(40.), px(40.)));
    }

    #[test]
    fn hover_tracks_windows_and_yields_local_bounds() {
        let mut s = ScreenshotSession::new(
            vec![
                screen("left", (-100, 20), 1., [255, 0, 0, 255]),
                screen("right", (0, 0), 2., [0, 255, 0, 255]),
            ],
            vec![snap(20., 30., 40., 50.)],
        );
        // entering / leaving flips the hover
        assert!(s.hover_at("right", point(px(30.), px(40.))));
        assert!(!s.hover_at("right", point(px(31.), px(41.)))); // same window
        let b = s.hover_bounds("right").unwrap();
        assert_eq!(b.origin, point(px(20.), px(30.)));
        assert_eq!(b.size, size(px(40.), px(50.)));
        assert!(s.hover_bounds("left").is_none()); // rect lives on right
        assert!(s.hover_at("right", point(px(5.), px(5.)))); // leave → None
        assert!(s.hover_bounds("right").is_none());

        // suppressed while dragging; press clears the outline
        s.hover_at("right", point(px(30.), px(40.)));
        s.begin("right", point(px(30.), px(40.)));
        assert!(!s.hover_at("right", point(px(35.), px(45.))));
        assert!(s.hover_bounds("right").is_none());
    }

    // ── In-place selection editing (move / resize) ────────────────

    #[test]
    fn move_selection_after_release_preserves_annotations() {
        let mut s = session();
        s.begin("left", point(px(10.), px(10.)));
        s.end("left", point(px(30.), px(30.))); // global (-90,30) 20×20
        s.edit_annotations(|a| a.toggle(crate::annotation::ShapeKind::Rectangle));
        s.pointer_down("left", point(px(12.), px(12.)));
        s.pointer_up("left", point(px(28.), px(28.)), false);
        assert_eq!(s.annotations().visible().count(), 1);
        // untoggle: back to selection mode (editing only works without a tool)
        s.edit_annotations(|a| a.toggle(crate::annotation::ShapeKind::Rectangle));

        // press the interior, drag, release → translated, annotations intact
        s.pointer_down("left", point(px(20.), px(20.))); // global (-80,40): interior
        assert!(s.selection().is_editing());
        assert!(!s.selection().is_selected()); // toolbar hides mid-edit
        s.pointer_move("left", point(px(40.), px(40.)), false); // global (-60,60)
        s.pointer_up("left", point(px(40.), px(40.)), false);
        let b = s.selection().bounds().unwrap();
        assert_eq!(b.origin, point(px(-70.), px(50.))); // +20,+20
        assert_eq!(b.size, size(px(20.), px(20.)));
        assert!(s.selection().is_selected());
        assert_eq!(s.annotations().visible().count(), 1); // NOT reset by the edit
    }

    #[test]
    fn resize_selection_by_corner_handle() {
        let mut s = session();
        s.begin("left", point(px(10.), px(10.)));
        s.end("left", point(px(30.), px(30.))); // global (-90,30) 20×20
        // press right at the bottom-right corner (within the 8px band)
        s.pointer_down("left", point(px(30.), px(30.)));
        assert!(s.selection().is_editing());
        s.pointer_move("left", point(px(50.), px(60.)), false); // global (-50,80)
        s.pointer_up("left", point(px(50.), px(60.)), false);
        let b = s.selection().bounds().unwrap();
        assert_eq!(b.origin, point(px(-90.), px(30.))); // top-left pinned
        assert_eq!(b.size, size(px(40.), px(50.)));
        // and the export path follows the new bounds
        assert_eq!(s.crop("left").unwrap().0, 40);
    }

    #[test]
    fn press_outside_selection_still_restarts_and_clears_annotations() {
        let mut s = session();
        s.begin("left", point(px(10.), px(10.)));
        s.end("left", point(px(30.), px(30.)));
        s.edit_annotations(|a| a.toggle(crate::annotation::ShapeKind::Rectangle));
        s.pointer_down("left", point(px(15.), px(15.)));
        s.pointer_up("left", point(px(25.), px(25.)), false);
        assert_eq!(s.annotations().visible().count(), 1);
        // untoggle the tool, then press well outside the box: fresh
        // selection, annotations wiped
        s.edit_annotations(|a| a.toggle(crate::annotation::ShapeKind::Rectangle));
        s.pointer_down("left", point(px(60.), px(60.)));
        assert!(s.selection().is_dragging());
        s.pointer_move("left", point(px(80.), px(80.)), false);
        s.pointer_up("left", point(px(80.), px(80.)), false);
        assert_eq!(s.annotations().visible().count(), 0);
        let b = s.selection().bounds().unwrap();
        assert_eq!(b.origin, point(px(-40.), px(80.)));
    }

    #[test]
    fn esc_reverts_an_inflight_move() {
        let mut s = session();
        s.begin("left", point(px(10.), px(10.)));
        s.end("left", point(px(30.), px(30.)));
        s.pointer_down("left", point(px(20.), px(20.)));
        s.pointer_move("left", point(px(60.), px(60.)), false);
        s.cancel_drag();
        assert!(s.selection().is_selected());
        let b = s.selection().bounds().unwrap();
        assert_eq!(b.origin, point(px(-90.), px(30.)));
        assert_eq!(b.size, size(px(20.), px(20.)));
    }

    #[test]
    fn cross_screen_move_translates_globally() {
        let mut s = session();
        s.begin("left", point(px(80.), px(20.)));
        s.end("right", point(px(20.), px(60.))); // global (-20,40) 40×20, spans the seam
        // grab the part that lives on the RIGHT screen…
        s.pointer_down("right", point(px(10.), px(50.))); // global (10,50): interior
        // …and the drag continues with the LEFT overlay delivering events
        // (left origin is (-100,20): local (90,40) → global (-10,60))
        s.pointer_move("left", point(px(90.), px(40.)), false);
        s.pointer_up("left", point(px(90.), px(40.)), false);
        let b = s.selection().bounds().unwrap();
        assert_eq!(b.origin, point(px(-40.), px(50.)));
        assert_eq!(b.size, size(px(40.), px(20.)));
        assert_eq!(
            s.local_bounds("left").unwrap(),
            Bounds {
                origin: point(px(60.), px(30.)),
                size: size(px(40.), px(20.))
            }
        );
        assert!(s.local_bounds("right").is_none()); // fully on the left now
    }

    #[test]
    fn in_place_click_inside_selection_keeps_it_even_over_a_snap_window() {
        let mut s = snapped_session();
        s.begin("right", point(px(25.), px(35.)));
        s.end("right", point(px(55.), px(75.)));
        // a click (no drag) at a point that ALSO sits on a snap window:
        // editing semantics win — the current selection is kept, no re-snap
        s.pointer_down("right", point(px(40.), px(55.)));
        s.pointer_up("right", point(px(40.), px(55.)), false);
        assert!(s.selection().is_selected());
        let b = s.selection().bounds().unwrap();
        assert_eq!(b.origin, point(px(25.), px(35.)));
        assert_eq!(b.size, size(px(30.), px(40.)));
    }

    #[test]
    fn editing_suppresses_the_window_hover_outline() {
        let mut s = snapped_session();
        s.begin("right", point(px(25.), px(35.)));
        s.end("right", point(px(55.), px(75.)));
        s.pointer_down("right", point(px(40.), px(55.))); // move grab
        assert!(!s.hover_at("right", point(px(30.), px(40.)))); // over a window, but editing
        assert!(s.hover_bounds("right").is_none());
        s.pointer_up("right", point(px(40.), px(55.)), false);
        // released: hover tracking resumes
        assert!(s.hover_at("right", point(px(30.), px(40.))));
        assert!(s.hover_bounds("right").is_some());
    }

    // ── Toolbar dragging ───────────────────────────────────────────

    #[test]
    fn toolbar_drag_moves_clamps_and_reverts() {
        let mut s = session();
        s.set_size("right", size(px(1200.), px(800.)));
        s.begin("right", point(px(50.), px(50.)));
        s.end("right", point(px(200.), px(150.))); // (50,50)-(200,150), right is host

        // anchored below the box by default; grips line both edges of ROW ONE
        let anchored = s.toolbar_bounds("right").unwrap();
        assert_eq!(anchored.origin, point(px(50.), px(158.)));
        assert_eq!(anchored.size.width, px(crate::model::placement::TB_W));
        let (lg, rg) = s.toolbar_grips("right").unwrap();
        let pad = px(crate::model::placement::BAR_PAD);
        // the strips are the grip ELEMENTS' rects: inset by the bar
        // padding, one row tall — pixel-identical to what renders
        assert_eq!(lg.left(), anchored.left() + pad);
        assert_eq!(rg.right(), anchored.right() - pad);
        assert_eq!(lg.size.width, px(crate::model::placement::GRIP_W));
        assert_eq!(lg.size.height, px(crate::model::placement::ROW_H));
        // a different output hosts nothing
        assert!(s.toolbar_bounds("left").is_none());

        // drag from the middle of the left grip: toolbar follows, no jump
        let press = point(px(56.), px(177.)); // grab = (6, 19)
        assert!(s.toolbar_drag_begin("right", press));
        assert!(s.toolbar_drag_active());
        assert!(s.toolbar_drag_move("right", point(px(200.), px(120.))));
        let b = s.toolbar_bounds("right").unwrap();
        assert_eq!(b.origin, point(px(194.), px(101.)));
        assert_eq!(b.size, anchored.size); // size never changes
        // unchanged position reports false (no duplicate notify)
        assert!(!s.toolbar_drag_move("right", point(px(200.), px(120.))));

        // clamped inside the window on every side
        assert!(s.toolbar_drag_move("right", point(px(2000.), px(2000.))));
        assert_eq!(
            s.toolbar_bounds("right").unwrap().origin,
            point(
                px(1200. - crate::model::placement::TB_W - 8.),
                px(800. - crate::model::placement::ROW_H - 8.),
            )
        );
        assert!(s.toolbar_drag_move("right", point(px(-999.), px(-999.))));
        assert_eq!(
            s.toolbar_bounds("right").unwrap().origin,
            point(px(8.), px(8.))
        );

        // Esc mid-drag: back to the anchor (the pre-drag override was None)
        s.cancel_drag();
        assert!(!s.toolbar_drag_active());
        assert_eq!(s.toolbar_bounds("right").unwrap(), anchored);

        // a completed drag stays put…
        assert!(s.toolbar_drag_begin("right", point(px(56.), px(177.))));
        assert!(s.toolbar_drag_move("right", point(px(300.), px(300.))));
        s.toolbar_drag_end();
        let dropped = s.toolbar_bounds("right").unwrap().origin;
        assert_eq!(dropped, point(px(294.), px(281.)));
        // …and Esc with no drag in flight does NOT move it (stage two: quit)
        s.cancel_drag();
        assert_eq!(s.toolbar_bounds("right").unwrap().origin, dropped);
    }

    #[test]
    fn editing_the_selection_keeps_a_dragged_toolbar_but_a_new_one_reanchors() {
        let mut s = session();
        s.set_size("right", size(px(1200.), px(800.)));
        s.begin("right", point(px(50.), px(50.)));
        s.end("right", point(px(200.), px(150.)));
        let anchored = s.toolbar_bounds("right").unwrap().origin;
        assert!(s.toolbar_drag_begin("right", point(px(56.), px(177.))));
        assert!(s.toolbar_drag_move("right", point(px(400.), px(500.))));
        s.toolbar_drag_end();
        let dragged = s.toolbar_bounds("right").unwrap().origin;
        assert_ne!(dragged, anchored);

        // moving the SELECTION (an edit) keeps the user's placement
        s.pointer_down("right", point(px(120.), px(100.))); // interior
        s.pointer_move("right", point(px(220.), px(200.)), false);
        s.pointer_up("right", point(px(220.), px(200.)), false);
        assert_eq!(s.toolbar_bounds("right").unwrap().origin, dragged);

        // a NEW selection re-anchors
        s.pointer_down("right", point(px(500.), px(500.)));
        s.pointer_move("right", point(px(700.), px(650.)), false);
        s.pointer_up("right", point(px(700.), px(650.)), false);
        assert_ne!(s.toolbar_bounds("right").unwrap().origin, dragged);

        // and so does a change of host window (the override is local!)
        s.set_size("left", size(px(1200.), px(800.)));
        s.begin("left", point(px(300.), px(50.)));
        s.end("left", point(px(700.), px(150.)));
        assert!(s.toolbar_drag_begin("left", point(px(306.), px(177.))));
        assert!(s.toolbar_drag_move("left", point(px(400.), px(500.))));
        s.toolbar_drag_end();
        assert!(s.toolbar_bounds("left").unwrap().origin.y > px(158.)); // dragged
        // selection replaced from the right screen → left re-anchors
        s.begin("right", point(px(50.), px(50.)));
        s.end("right", point(px(200.), px(150.)));
        assert!(s.toolbar_bounds("left").is_none());
    }

    #[test]
    fn rectangle_crosses_mixed_dpi_outputs_and_is_encoded_but_not_used_for_ocr() {
        let mut s = session();
        s.begin("left", point(px(80.), px(20.)));
        s.end("right", point(px(20.), px(60.)));
        s.edit_annotations(|a| a.toggle(crate::annotation::ShapeKind::Rectangle));
        s.pointer_down("left", point(px(90.), px(25.)));
        s.pointer_up("right", point(px(10.), px(55.)), false);
        let (w, h, pixels) = s.crop("right").unwrap();
        let png = crate::model::export::encode_png(w, h, &pixels).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        let color = s.annotations().color().0.to_be_bytes();
        assert_eq!(decoded.get_pixel(20, 10).0, color);
        assert_eq!(decoded.get_pixel(40, 10).0, color);
        assert_eq!(decoded.get_pixel(40, 20).0, [0, 255, 0, 255]);
        let (_, _, original) = s.crop_original("left").unwrap();
        assert_eq!(
            &original[(10 * w as usize + 40) * 4..(10 * w as usize + 40) * 4 + 4],
            &[0, 255, 0, 255]
        );
    }
    #[test]
    fn ellipse_crosses_mixed_dpi_outputs_with_an_unmarked_center_and_ocr_source() {
        let mut s = session();
        s.begin("left", point(px(80.), px(20.)));
        s.end("right", point(px(20.), px(60.)));
        s.edit_annotations(|a| {
            a.toggle(crate::annotation::ShapeKind::Ellipse);
            a.set_color(4);
        });
        s.pointer_down("left", point(px(90.), px(25.)));
        s.pointer_up("right", point(px(10.), px(55.)), false);
        let left = s.local_annotations("left")[0].clone();
        let right = s.local_annotations("right")[0].clone();
        assert_eq!(left.bounds.origin, point(px(90.), px(25.)));
        assert_eq!(right.bounds.origin, point(px(-10.), px(45.)));
        let (w, h, pixels) = s.crop("right").unwrap();
        let png = crate::model::export::encode_png(w, h, &pixels).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        let color = s.annotations().color().0.to_be_bytes();
        for (x, y) in [(21, 20), (58, 20), (40, 11), (40, 28)] {
            assert_eq!(decoded.get_pixel(x, y).0, color);
        }
        assert_eq!(decoded.get_pixel(40, 20).0, [0, 255, 0, 255]);
        assert_eq!(decoded.get_pixel(20, 10).0, [255, 0, 0, 255]);
        let (_, _, original) = s.crop_original("left").unwrap();
        assert_eq!(
            &original[(11 * w as usize + 40) * 4..(11 * w as usize + 40) * 4 + 4],
            &[0, 255, 0, 255]
        );
    }
    #[test]
    fn line_and_polyline_cross_outputs_and_export_without_floating_preview() {
        for kind in [
            crate::annotation::ShapeKind::Line,
            crate::annotation::ShapeKind::Arrow,
            crate::annotation::ShapeKind::Polyline,
            crate::annotation::ShapeKind::Pencil,
        ] {
            let mut s = session();
            s.begin("left", point(px(80.), px(20.)));
            s.end("right", point(px(20.), px(60.)));
            s.edit_annotations(|a| {
                a.toggle(kind);
                a.set_color(4);
            });
            s.pointer_down("left", point(px(90.), px(30.)));
            if kind == crate::annotation::ShapeKind::Polyline {
                s.pointer_up("left", point(px(90.), px(30.)), false);
                s.pointer_down("right", point(px(10.), px(50.)));
            }
            s.pointer_up("right", point(px(10.), px(50.)), false);
            s.pointer_move("right", point(px(10.), px(58.)), false);
            s.edit_annotations(|a| a.finish_polyline());
            let left = s.local_annotations("left");
            let right = s.local_annotations("right");
            assert_eq!(left[0].points[0], point(px(90.), px(30.)));
            assert_eq!(right[0].points[0], point(px(-10.), px(50.)));
            let (w, h, pixels) = s.crop("left").unwrap();
            let png = crate::model::export::encode_png(w, h, &pixels).unwrap();
            let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
            let color = s.annotations().color().0.to_be_bytes();
            assert_eq!(decoded.get_pixel(25, 20).0, color);
            assert_eq!(decoded.get_pixel(55, 20).0, color);
            assert_eq!(decoded.get_pixel(60, 35).0, [0, 255, 0, 255]);
            let (_, _, original) = s.crop_original("right").unwrap();
            assert_eq!(
                &original[(20 * w as usize + 55) * 4..(20 * w as usize + 55) * 4 + 4],
                &[0, 255, 0, 255]
            );
        }
    }
    #[test]
    fn text_crosses_outputs_with_shared_preview_and_history() {
        let mut s = session();
        s.begin("left", point(px(80.), px(20.)));
        s.end("right", point(px(40.), px(90.)));
        let bounds = s.text_bounds("left", point(px(85.), px(25.))).unwrap();
        s.edit_annotations(|a| {
            a.set_text_size(0);
            a.set_color(4);
            a.add_text(bounds, "MMMM 中文".into());
        });
        let original = s.crop_original("left").unwrap().2;
        let (w, _, pixels) = s.crop("left").unwrap();
        let mut sides = [false; 2];
        for (i, (before, after)) in original
            .as_chunks::<4>()
            .0
            .iter()
            .zip(pixels.as_chunks::<4>().0.iter())
            .enumerate()
        {
            if before != after {
                sides[usize::from(i % w as usize >= 40)] = true;
            }
        }
        assert_eq!(sides, [true, true]);
        let (_, left) = s.filtered_preview("left").unwrap();
        let (_, right) = s.filtered_preview("right").unwrap();
        assert!(Arc::ptr_eq(&left, &right));
        s.edit_annotations(|a| a.undo());
        assert_eq!(s.crop("left").unwrap().2, original);
        s.edit_annotations(|a| a.redo());
        assert_eq!(s.crop("right").unwrap().2, pixels);
    }

    #[test]
    fn eraser_crosses_outputs_and_preview_matches_export() {
        use crate::annotation::ShapeKind;
        for kind in [ShapeKind::Eraser, ShapeKind::EraserRect] {
            let mut s = session();
            s.begin("left", point(px(80.), px(20.)));
            s.end("right", point(px(40.), px(90.)));
            s.edit_annotations(|a| a.toggle(ShapeKind::Rectangle));
            s.pointer_down("left", point(px(82.), px(25.)));
            s.pointer_up("right", point(px(38.), px(60.)), false);
            let marked = s.crop("left").unwrap().2;
            s.edit_annotations(|a| a.toggle(kind));
            s.pointer_down("left", point(px(81.), px(22.)));
            s.pointer_up("right", point(px(39.), px(70.)), false);
            let pixels = s.crop("left").unwrap().2;
            assert_ne!(pixels, marked);
            let (_, left) = s.filtered_preview("left").unwrap();
            let (_, right) = s.filtered_preview("right").unwrap();
            assert!(Arc::ptr_eq(&left, &right));
            let bgra: Vec<_> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect();
            assert_eq!(left.as_bytes(0).unwrap(), bgra);
            s.edit_annotations(|a| a.undo());
            assert_eq!(s.crop("left").unwrap().2, marked);
            assert!(s.filtered_preview("left").is_none());
            s.edit_annotations(|a| a.redo());
            assert_eq!(s.crop("right").unwrap().2, pixels);
        }
    }

    #[test]
    fn filters_share_export_pixels_across_outputs_and_invalidate_on_undo() {
        for kind in [
            crate::annotation::ShapeKind::Mosaic,
            crate::annotation::ShapeKind::Blur,
        ] {
            let mut s = session();
            s.begin("left", point(px(80.), px(20.)));
            s.end("right", point(px(20.), px(60.)));
            let original = s.crop_original("left").unwrap().2;
            s.edit_annotations(|a| a.toggle(kind));
            s.pointer_down("right", point(px(18.), px(58.)));
            s.pointer_up("left", point(px(82.), px(22.)), false);
            let (w, h, pixels) = s.crop("left").unwrap();
            assert_ne!(pixels, original);
            let (left_bounds, left) = s.filtered_preview("left").unwrap();
            let (right_bounds, right) = s.filtered_preview("right").unwrap();
            assert!(Arc::ptr_eq(&left, &right));
            assert_eq!(
                left_bounds.origin - right_bounds.origin,
                point(px(100.), px(-20.))
            );
            let bgra: Vec<_> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect();
            assert_eq!(left.as_bytes(0).unwrap(), bgra);
            let encoded = crate::model::export::encode_png(w, h, &pixels).unwrap();
            assert_eq!(
                image::load_from_memory(&encoded)
                    .unwrap()
                    .into_rgba8()
                    .into_raw(),
                pixels
            );
            assert_eq!(s.crop_original("right").unwrap().2, original);
            s.edit_annotations(|a| a.undo());
            assert!(s.filtered_preview("left").is_none());
            assert_eq!(s.crop("left").unwrap().2, original);
            s.edit_annotations(|a| a.redo());
            assert_eq!(s.crop("left").unwrap().2, pixels);
            s.pointer_down("left", point(px(82.), px(22.)));
            s.pointer_move("right", point(px(5.), px(55.)), false);
            let (_, draft) = s.filtered_preview("left").unwrap();
            assert!(!Arc::ptr_eq(&left, &draft));
            s.cancel_annotation();
            assert_eq!(s.crop("left").unwrap().2, pixels);
        }
    }

    #[test]
    fn highlighter_crosses_mixed_dpi_outputs_and_leaves_ocr_unmarked() {
        let mut s = session();
        s.begin("left", point(px(80.), px(20.)));
        s.end("right", point(px(20.), px(60.)));
        s.edit_annotations(|a| a.toggle(crate::annotation::ShapeKind::Highlighter));
        s.pointer_down("left", point(px(90.), px(30.)));
        s.pointer_up("right", point(px(10.), px(50.)), false);
        assert_eq!(
            s.local_annotations("right")[0].points[0],
            point(px(-10.), px(50.))
        );
        let (w, h, marked) = s.crop("left").unwrap();
        let (_, _, original) = s.crop_original("right").unwrap();
        let png = crate::model::export::encode_png(w, h, &marked).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        let color = s.annotations().color().0.to_be_bytes();
        for (x, y) in [(25, 20), (55, 20)] {
            let at = ((y * w + x) * 4) as usize;
            for channel in 0..3 {
                let expected = (original[at + channel] as f32 * (159. / 255.)
                    + color[channel] as f32 * (96. / 255.))
                    .round() as u8;
                assert_eq!(decoded.get_pixel(x, y)[channel], expected);
            }
            assert_ne!(&marked[at..at + 3], &original[at..at + 3]);
        }
        s.edit_annotations(|a| a.undo());
        assert_eq!(s.crop("left").unwrap().2, original);
        s.edit_annotations(|a| a.redo());
        assert_eq!(s.crop("left").unwrap().2, marked);
    }

    #[test]
    fn sequence_numbers_are_global_across_screens_and_export_across_the_seam() {
        let mut s = session();
        s.begin("left", point(px(80.), px(0.)));
        s.end("right", point(px(20.), px(100.)));
        s.edit_annotations(|a| {
            a.toggle(crate::annotation::ShapeKind::Number);
            a.set_color(4);
        });
        s.pointer_down("left", point(px(98.), px(20.)));
        s.pointer_up("left", point(px(98.), px(20.)), false);
        s.pointer_down("right", point(px(2.), px(75.)));
        s.pointer_up("right", point(px(2.), px(75.)), false);
        assert_eq!(
            s.annotations()
                .visible()
                .map(|mark| mark.number.unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            s.local_annotations("left")[0].bounds.origin,
            point(px(82.), px(4.))
        );
        assert_eq!(
            s.local_annotations("right")[0].bounds.origin,
            point(px(-18.), px(24.))
        );
        let (w, h, pixels) = s.crop("right").unwrap();
        let png = crate::model::export::encode_png(w, h, &pixels).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        let color = s.annotations().color().0.to_be_bytes();
        assert_eq!(decoded.get_pixel(10, 40).0, color);
        assert_eq!(decoded.get_pixel(56, 40).0, color);
        assert_ne!(s.crop_original("left").unwrap().2, pixels);
        s.edit_annotations(|a| a.undo());
        assert_eq!(s.annotations().next_number(), 2);
    }
}
