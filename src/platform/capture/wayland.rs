//! # Wayland event state machine: the Dispatch family for multi-output screencopy
//!
//! Orchestration lives in [`super::capture_all_outputs`]; this file is pure
//! "wiring": one [`Dispatch`] impl per bound protocol object, output state
//! keyed by registry index.

use std::fs::File;
use std::os::fd::AsFd;

use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::{self, ZxdgOutputManagerV1},
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};

use super::Transform;

/// Map the protocol's transform onto the platform-neutral enum (the
/// word around `_90` semantics is preserved by pixels::rotate_rgba)
impl From<wl_output::Transform> for Transform {
    fn from(t: wl_output::Transform) -> Self {
        use wl_output::Transform as T;
        match t {
            T::Normal => Self::Normal,
            T::_90 => Self::Rot90,
            T::_180 => Self::Rot180,
            T::_270 => Self::Rot270,
            T::Flipped => Self::Flipped,
            T::Flipped90 => Self::Flipped90,
            T::Flipped180 => Self::Flipped180,
            T::Flipped270 => Self::Flipped270,
            _ => Self::Normal,
        }
    }
}

#[derive(Default)]
pub(super) struct App {
    pub shm: Option<wl_shm::WlShm>,
    pub manager: Option<ZwlrScreencopyManagerV1>,
    /// xdg-output manager: the authoritative source for an output's
    /// LOGICAL geometry (true fractional scale + transform applied —
    /// wl_output only reports the integer-rounded scale)
    pub xdg_manager: Option<ZxdgOutputManagerV1>,
    /// All outputs in registry order (the index doubles as udata everywhere)
    pub outputs: Vec<OutputState>,
}

pub(super) struct OutputState {
    pub output: wl_output::WlOutput,
    pub name: String,
    pub logical_pos: (i32, i32),
    /// Logical size from zxdg_output_v1 (true scale + transform); None
    /// until the event arrives (or ever, if the protocol is absent)
    pub logical_size: Option<(i32, i32)>,
    pub scale: f32,
    pub transform: Transform,
    pub frame: Option<FrameState>,
}

pub(super) struct FrameState {
    #[allow(dead_code)]
    proxy: ZwlrScreencopyFrameV1,
    info: Option<(wl_shm::Format, i32, i32, i32)>, // (format, w, h, stride)
    y_invert: bool,
    pub failed: bool,
    ready: bool,
    pub mmap: Option<memmap2::MmapMut>,
    #[allow(dead_code)]
    buffer: Option<wl_buffer::WlBuffer>,
    #[allow(dead_code)]
    file: Option<File>,
}

impl FrameState {
    pub fn new(proxy: ZwlrScreencopyFrameV1) -> Self {
        Self {
            proxy,
            info: None,
            y_invert: false,
            failed: false,
            ready: false,
            mmap: None,
            buffer: None,
            file: None,
        }
    }

    pub fn take_frame_info(&mut self) -> Option<(wl_shm::Format, i32, i32, i32, bool)> {
        self.info
            .take()
            .map(|(f, w, h, s)| (f, w, h, s, self.y_invert))
    }
}

impl App {
    /// Are all frames done (succeeded or failed)?
    pub fn all_frames_done(&self) -> bool {
        self.outputs
            .iter()
            .all(|o| o.frame.as_ref().is_some_and(|f| f.ready || f.failed))
    }

    fn frame_mut(&mut self, idx: usize) -> Option<&mut FrameState> {
        self.outputs.get_mut(idx)?.frame.as_mut()
    }

    /// Full Buffer-event handling: record metadata → create shm pool/buffer →
    /// request copy → attach resources back.
    /// (Split into its own method for borrow splitting: creating the pool
    /// needs a shared borrow of self.shm while attaching needs a mutable one)
    #[allow(clippy::too_many_arguments)] // wayland event fields are just this many
    fn handle_buffer(
        &mut self,
        frame: &ZwlrScreencopyFrameV1,
        idx: usize,
        qh: &QueueHandle<Self>,
        fmt: wl_shm::Format,
        w: i32,
        h: i32,
        stride: i32,
    ) {
        if let Some(f) = self.frame_mut(idx) {
            f.info = Some((fmt, w, h, stride));
        }
        let size = (stride as i64 * h as i64) as u64;

        let file = tempfile::tempfile().expect("create temp file");
        file.set_len(size).expect("set file length");
        let mmap = unsafe { memmap2::MmapMut::map_mut(&file).expect("mmap") };

        let shm = self.shm.as_ref().expect("no wl_shm?");
        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
        let buffer = pool.create_buffer(0, w, h, stride, fmt, qh, ());
        frame.copy(&buffer);

        if let Some(f) = self.frame_mut(idx) {
            f.file = Some(file);
            f.mmap = Some(mmap);
            f.buffer = Some(buffer);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_shm" => state.shm = Some(registry.bind(name, version, qh, ())),
                "zwlr_screencopy_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(3), qh, ()))
                }
                "zxdg_output_manager_v1" => {
                    state.xdg_manager = Some(registry.bind(name, version.min(3), qh, ()))
                }
                "wl_output" => {
                    // Bind all outputs; udata = index
                    let idx = state.outputs.len();
                    let output = registry.bind(name, version.min(4), qh, idx);
                    state.outputs.push(OutputState {
                        output,
                        name: String::new(),
                        logical_pos: (0, 0),
                        logical_size: None,
                        scale: 1.,
                        transform: Transform::Normal,
                        frame: None,
                    });
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, usize> for App {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(o) = state.outputs.get_mut(*idx) else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => o.name = name,
            wl_output::Event::Geometry {
                x, y, transform, ..
            } => {
                o.logical_pos = (x, y);
                o.transform = transform
                    .into_result()
                    .map(Transform::from)
                    .unwrap_or(Transform::Normal);
            }
            wl_output::Event::Scale { factor } => o.scale = factor as f32,
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, usize> for App {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(o) = state.outputs.get_mut(*idx) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => o.logical_pos = (x, y),
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                o.logical_size = Some((width, height))
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrScreencopyManagerV1,
        _: zwlr_screencopy_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, usize> for App {
    fn event(
        state: &mut Self,
        frame: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        idx: &usize,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            // The compositor announces this frame's format/size/stride →
            // create the shm buffer and request the copy
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let fmt = format.into_result().unwrap_or(wl_shm::Format::Xrgb8888);
                state.handle_buffer(
                    frame,
                    *idx,
                    qh,
                    fmt,
                    width as i32,
                    height as i32,
                    stride as i32,
                );
            }
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                if let Some(f) = state.frame_mut(*idx) {
                    f.y_invert = flags
                        .into_result()
                        .is_ok_and(|f| f.contains(zwlr_screencopy_frame_v1::Flags::YInvert));
                }
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                if let Some(f) = state.frame_mut(*idx) {
                    f.ready = true;
                }
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                if let Some(f) = state.frame_mut(*idx) {
                    f.failed = true;
                }
            }
            _ => {}
        }
    }
}
