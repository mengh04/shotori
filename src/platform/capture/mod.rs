//! # Screen capture library: a multi-output wrapper over the Wayland backend
//!
//! Submodules:
//! - [`pixels`]: pure pixel processing (format conversion / transform
//!   rotation), unit-tested
//! - `wayland`: the wlr-screencopy event state machine
//!
//! One-shot synchronous capture: ~15ms for a single output, ~350ms for
//! three on Linux (including encoding). The backend runs on a dedicated
//! Wayland connection and does not interfere with gpui's.

mod pixels;

// Re-exported at crate level for the microbenchmark harness (`--bench`)
pub(crate) use pixels::convert_to_rgba;
pub(crate) use pixels::rotate_rgba;

mod wayland;

/// Output orientation, platform-neutral (converted from
/// `wl_output::Transform`)
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Transform {
    #[default]
    Normal,
    Rot90,
    Rot180,
    Rot270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

/// One successful capture (one output)
pub struct Capture {
    pub output_name: String,
    /// Global logical position of the output (wl_output::Geometry,
    /// refined by zxdg_output_v1::LogicalPosition when available)
    pub logical_pos: (i32, i32),
    /// Logical size: the TRUE size after fractional scale and transform
    /// (zxdg_output_v1's LogicalSize; None without the protocol —
    /// consumers fall back to width÷scale × height÷scale)
    pub logical_size: Option<(i32, i32)>,
    /// The physical-to-logical pixel ratio: the **integer** scale from
    /// wl_output (a 1.5x output reports 2). gpui's display bounds origin
    /// = logical position ÷ this value (see crate::platform::display)
    pub scale: f32,
    /// Output orientation: the wl_output transform (a 90° panel's
    /// physical buffer is landscape)
    pub transform: Transform,
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
        self.transform != Transform::Normal
    }

    /// The TRUE logical size (fractional scale + transform aware):
    /// the authoritative value when present, else width÷scale (the
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
            transform: Transform::Normal,
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

    // A stuck screencopy frame (compositor killed mid-capture, protocol
    // violation) must not hang shotori forever: drain the queue
    // non-blockingly against a 10 s deadline instead of blocking_dispatch.
    const FRAME_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);
    let deadline = std::time::Instant::now() + FRAME_DEADLINE;
    while !app.all_frames_done() {
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("screencopy frame timed out (compositor not responding)");
        }
        queue.dispatch_pending(&mut app)?;
        if !app.all_frames_done() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    // Collect the successes (one output failing must not sink the others)
    let mut caps = Vec::new();
    for o in &mut app.outputs {
        let Some(f) = o.frame.as_mut() else { continue };
        if f.failed {
            eprintln!("[shotori] capture failed for {}, skipping", o.name);
            continue;
        }
        // A ready frame SHOULD have buffer info + a mapped buffer; a
        // protocol-violating compositor can deliver ready without either,
        // in which case this output is skipped instead of panicking.
        let Some((format, w, h, stride, y_invert)) = f.take_frame_info() else {
            eprintln!(
                "[shotori] capture of {} ready without buffer info, skipping",
                o.name
            );
            continue;
        };
        let Some(mmap) = f.mmap.take() else {
            eprintln!(
                "[shotori] capture of {} has no mapped buffer, skipping",
                o.name
            );
            continue;
        };
        // The physical buffer "lies flat"; rotate it per the output transform
        // into the orientation the screen shows
        let rgba = pixels::convert_to_rgba(&mmap[..], format, w, h, stride, y_invert);
        if rgba.len() != (w as usize * h as usize * 4) {
            eprintln!(
                "[shotori] capture of {} returned malformed pixels, skipping",
                o.name
            );
            continue;
        }
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
