//! Shared screenshot selection and captures for every overlay in one session.
use std::sync::Arc;

use gpui_kit::*;

use crate::{capture::Capture, selection::Selection};

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

pub struct ScreenshotSession {
    screens: Vec<Screen>,
    selection: Selection,
    active_output: Option<String>,
    blocked: bool,
}

impl ScreenshotSession {
    pub fn new(captures: Vec<Arc<Capture>>) -> Self {
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
        self.active_output = Some(name.to_owned());
        self.selection
            .begin(local + self.screen(name).bounds().origin);
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
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.selection.cancel_drag();
    }

    /// Keep the original single-output crop when possible. Spanning selections
    /// use the highest participating pixel density; desktop gaps stay transparent.
    pub(crate) fn crop(&self, fallback_output: &str) -> Option<(u32, u32, Vec<u8>)> {
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
            return crate::export::crop(
                &cap.rgba,
                cap.width,
                cap.height,
                bounds,
                cap.width as f32 / f32::from(screen.logical_size.width),
            );
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
            let (cw, ch, pixels) = crate::export::crop(
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
        Some((w, h, out.into_raw()))
    }
}

#[cfg(test)]
mod tests {
    use super::ScreenshotSession;
    use crate::capture::Capture;
    use gpui_kit::{point, px, size};
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
        ScreenshotSession::new(vec![
            screen("left", (-100, 20), 1., [255, 0, 0, 255]),
            screen("right", (0, 0), 2., [0, 255, 0, 255]),
        ])
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
        let mut s = ScreenshotSession::new(vec![
            screen("top", (0, 0), 1.25, [255, 0, 0, 255]),
            screen("bottom", (0, 120), 1.5, [0, 255, 0, 255]),
        ]);
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
}
