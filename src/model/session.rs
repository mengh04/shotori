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

pub struct ScreenshotSession {
    screens: Vec<Screen>,
    selection: Selection,
    active_output: Option<String>,
    blocked: bool,
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
                    logical_size: size(
                        px(capture.width as f32 / capture.scale),
                        px(capture.height as f32 / capture.scale),
                    ),
                    capture,
                })
                .collect(),
            selection: Selection::Idle,
            active_output: None,
            blocked: false,
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
        self.active_output = Some(name.to_owned());
        self.selection.begin(global);
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
    }

    /// Track the window under the cursor for the hover outline. Returns
    /// whether the hover changed (callers decide on cx.notify()).
    /// Suppressed while dragging — the freehand region takes over — and
    /// while a modal is open.
    pub(crate) fn hover_at(&mut self, name: &str, local: Point<Pixels>) -> bool {
        if self.blocked
            || self.selection.is_dragging()
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

    pub(crate) fn pointer_down(&mut self, name: &str, local: Point<Pixels>) {
        if self.blocked {
            return;
        }
        if self.annotations.enabled() {
            if let Some(selection) = self.selection.bounds() {
                self.annotations
                    .begin(local + self.screen(name).bounds().origin, selection);
            }
        } else {
            self.begin(name, local);
        }
    }

    pub(crate) fn pointer_move(&mut self, name: &str, local: Point<Pixels>, square: bool) -> bool {
        if self.blocked {
            return false;
        }
        if self.annotations.enabled() {
            if let Some(selection) = self.selection.bounds() {
                return self.annotations.drag_to(
                    local + self.screen(name).bounds().origin,
                    selection,
                    square,
                );
            }
            false
        } else {
            self.drag_to(name, local)
        }
    }

    pub(crate) fn pointer_up(&mut self, name: &str, local: Point<Pixels>, square: bool) {
        if self.blocked {
            return;
        }
        if self.annotations.enabled() {
            self.pointer_move(name, local, square);
            self.annotations.end();
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
    pub(crate) fn crop_original(&self, output: &str) -> Option<(u32, u32, Vec<u8>)> {
        self.crop_impl(output, false)
            .map(|r| (r.width, r.height, r.rgba))
    }

    /// Reuse the exported composite on every output whenever pixel filters are present.
    /// Captures are immutable; selection, shapes and display geometry own invalidation.
    pub(crate) fn filtered_preview(
        &self,
        output: &str,
    ) -> Option<(Bounds<Pixels>, Arc<RenderImage>)> {
        use crate::annotation::ShapeKind;
        let mut cache = self.filter_preview.borrow_mut();
        if !self
            .annotations
            .visible()
            .any(|s| matches!(s.kind, ShapeKind::Mosaic | ShapeKind::Blur))
        {
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
        assert!(s.selection().is_selected());
        assert!(s.local_bounds("right").is_some());
        assert_eq!(s.crop("left").unwrap().0, 80);
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
