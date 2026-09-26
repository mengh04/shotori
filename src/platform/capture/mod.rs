//! # Screen capture library: a multi-output wrapper over per-platform backends
//!
//! Submodules:
//! - [`pixels`]: pure pixel processing (format conversion / transform
//!   rotation), unit-tested
//! - `wayland` (Linux): the wlr-screencopy event state machine
//! - `windows` (Windows): one GDI `BitBlt` per monitor
//!
//! One-shot synchronous capture: ~15ms for a single output, ~350ms for
//! three on Linux (including encoding). The Linux backend runs on a
//! dedicated Wayland connection and does not interfere with gpui's.

mod pixels;

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "windows")]
mod windows;

/// Output orientation, platform-neutral (Linux converts from
/// `wl_output::Transform`; Windows GDI pixels are already in displayed
/// orientation, so that backend always reports [`Transform::Normal`])
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
    /// Global logical position of the output. Linux: wl_output::Geometry,
    /// refined by zxdg_output_v1::LogicalPosition when available.
    /// Windows: the monitor's physical origin in the virtual-desktop
    /// space (see the coordinate-space note in `windows.rs`)
    pub logical_pos: (i32, i32),
    /// Logical size: the TRUE size after fractional scale and transform.
    /// Linux: zxdg_output_v1's LogicalSize (None without the protocol —
    /// consumers fall back to width÷scale × height÷scale). Windows:
    /// always None — the fallback **is** the true value there (the
    /// effective scale is exact, unlike wl_output's integer rounding)
    pub logical_size: Option<(i32, i32)>,
    /// The physical-to-logical pixel ratio. Linux: the **integer** scale
    /// from wl_output (a 1.5x output reports 2). Windows: the effective
    /// DPI ÷ 96 (fractional values such as 1.5 are exact). gpui's display
    /// bounds origin = logical position ÷ this value on both platforms
    /// (verified against both backends' source), so display matching
    /// uses the same algorithm everywhere (see crate::platform::display)
    pub scale: f32,
    /// Output orientation. Linux: the wl_output transform (a 90° panel's
    /// physical buffer is landscape). Windows: always Normal — GDI
    /// already delivers displayed-orientation pixels
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
#[cfg(target_os = "linux")]
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

/// Capture all monitors (GDI `BitBlt`, one per monitor — see `windows.rs`)
#[cfg(target_os = "windows")]
pub fn capture_all_outputs() -> anyhow::Result<Vec<Capture>> {
    windows::capture_all()
}

/// Windows: switch the process to per-monitor-v2 DPI awareness before
/// any window opens or GDI call runs (no-op on other platforms; see
/// `windows.rs` for why it matters)
#[cfg(target_os = "windows")]
pub fn enable_per_monitor_dpi_awareness() {
    windows::enable_per_monitor_dpi_awareness()
}
