//! # Screen capture library: a multi-output wrapper over wlr-screencopy
//!
//! Submodules:
//! - [`pixels`]: pure pixel processing (format conversion / transform
//!   rotation), unit-tested
//! - [`wayland`]: the wayland event state machine (the Dispatch family)
//!
//! One-shot synchronous capture on a dedicated Wayland connection: ~15ms for
//! a single output, ~350ms for three (including encoding). Does not interfere
//! with gpui's Wayland connection.

mod pixels;
mod wayland;

/// One successful capture (one output)
pub struct Capture {
    pub output_name: String,
    /// Global logical position of the output (wl_output::Geometry,
    /// refined by zxdg_output_v1::LogicalPosition when available)
    pub logical_pos: (i32, i32),
    /// Logical size from zxdg_output_v1: the TRUE size after fractional
    /// scale and transform (a 1.5x 90° output reports 720x1280 here while
    /// width/scale gives 540x960). None if the compositor lacks the
    /// protocol — consumers fall back to width/scale × height/scale
    pub logical_size: Option<(i32, i32)>,
    /// The scale reported by wl_output — **the integer version** (a 1.5x
    /// output reports 2). gpui's display bounds origin = logical position ÷
    /// this value (the backend does the division; verified by comparison),
    /// so matching must use the same algorithm (see crate::platform::display)
    pub scale: f32,
    /// Output geometry transform (DP-2 is 90°: the physical buffer is
    /// landscape while the panel is portrait)
    pub transform: wayland::OutputTransform,
    /// Physical size (**already rotated per transform**, matching what the
    /// screen shows)
    pub width: u32,
    pub height: u32,
    /// Pixels already converted to RGBA8, Y-flip applied, rotated per transform
    pub rgba: Vec<u8>,
}

impl Capture {
    /// Whether the output has a rotation transform (for logs/debugging)
    pub fn rotated(&self) -> bool {
        self.transform != wayland::OutputTransform::Normal
    }

    /// The TRUE logical size (fractional scale + transform aware):
    /// zxdg_output_v1's value when present, else width÷scale (the
    /// integer-scale fallback — wrong on fractional outputs, corrected
    /// later by the actual window bounds)
    pub fn logical_size_f32(&self) -> (f32, f32) {
        self.logical_size
            .map(|(w, h)| (w as f32, h as f32))
            .unwrap_or((
                self.width as f32 / self.scale,
                self.height as f32 / self.scale,
            ))
    }

    /// Constructor for unit tests in other modules (the normal path is
    /// [`capture_all_outputs`])
    #[cfg(test)]
    pub(crate) fn for_test(logical_pos: (i32, i32), scale: f32) -> Self {
        Self {
            output_name: "test-output".into(),
            logical_pos,
            logical_size: None,
            scale,
            transform: wayland::OutputTransform::Normal,
            width: 1920,
            height: 1080,
            rgba: Vec::new(),
        }
    }
}

/// Capture all outputs (at least one is required, otherwise error)
pub fn capture_all_outputs() -> anyhow::Result<Vec<Capture>> {
    use wayland::*;

    let conn = wayland_client::Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());

    let mut app = App::default();
    queue.roundtrip(&mut app)?; // globals in hand (including all wl_outputs)

    let manager = app
        .manager
        .take()
        .ok_or_else(|| anyhow::anyhow!("compositor does not support zwlr_screencopy_manager_v1"))?;
    if app.outputs.is_empty() {
        anyhow::bail!("no wl_output available");
    }
    // Ask for each output's logical geometry (true scale + transform) via
    // zxdg_output_v1 — the events arrive during the next roundtrip below
    if let Some(xdg) = app.xdg_manager.as_ref() {
        for (i, o) in app.outputs.iter().enumerate() {
            xdg.get_xdg_output(&o.output, &qh, i);
        }
    }
    queue.roundtrip(&mut app)?; // each output's name/geometry/scale/logical-size in hand

    // One capture frame per output
    for i in 0..app.outputs.len() {
        let output = app.outputs[i].output.clone();
        let frame = manager.capture_output(0, &output, &qh, i);
        app.outputs[i].frame = Some(FrameState::new(frame));
    }
    queue.roundtrip(&mut app)?; // buffer events → create shm buffers, request copy

    while !app.all_frames_done() {
        queue.blocking_dispatch(&mut app)?;
    }

    // Collect the successes (one output failing must not sink the others)
    let mut caps = Vec::new();
    for o in &mut app.outputs {
        let Some(f) = o.frame.as_mut() else { continue };
        if f.failed {
            eprintln!("[shotori] capture failed for {}, skipping", o.name);
            continue;
        }
        let (format, w, h, stride, y_invert) = f
            .take_frame_info()
            .expect("a ready frame always has buffer info");
        let mmap = f.mmap.take().expect("no mmap");
        // The physical buffer "lies flat"; rotate it per the output transform
        // into the orientation the screen shows
        let rgba = pixels::convert_to_rgba(&mmap[..], format, w, h, stride, y_invert);
        let rgba = pixels::rotate_rgba(rgba, w as u32, h as u32, o.transform);
        let (rw, rh) = pixels::rotated_size(w as u32, h as u32, o.transform);
        caps.push(Capture {
            output_name: o.name.clone(),
            logical_pos: o.logical_pos,
            logical_size: o.logical_size,
            scale: o.scale,
            transform: o.transform,
            width: rw,
            height: rh,
            rgba,
        });
    }
    if caps.is_empty() {
        anyhow::bail!("all output captures failed");
    }
    Ok(caps)
}
