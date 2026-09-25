//! # wayland 事件状态机：多输出 screencopy 的 Dispatch 全家
//!
//! 编排在 [`super::capture_all_outputs`]；这里只有"接线"：
//! 每个绑定的协议对象一个 [`Dispatch`] 实现，输出状态按 registry 索引关联。

use std::fs::File;
use std::os::fd::AsFd;

use wayland_client::{
    protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};

pub(super) use wl_output::Transform as OutputTransform;

#[derive(Default)]
pub(super) struct App {
    pub shm: Option<wl_shm::WlShm>,
    pub manager: Option<ZwlrScreencopyManagerV1>,
    /// registry 顺序的全部输出（索引即各处 udata）
    pub outputs: Vec<OutputState>,
}

pub(super) struct OutputState {
    pub output: wl_output::WlOutput,
    pub name: String,
    pub logical_pos: (i32, i32),
    pub scale: f32,
    pub transform: OutputTransform,
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
    /// 所有帧都完成（成功或失败）了吗
    pub fn all_frames_done(&self) -> bool {
        self.outputs
            .iter()
            .all(|o| o.frame.as_ref().is_some_and(|f| f.ready || f.failed))
    }

    fn frame_mut(&mut self, idx: usize) -> Option<&mut FrameState> {
        self.outputs.get_mut(idx)?.frame.as_mut()
    }

    /// Buffer 事件的完整处理：记录元信息 → 建 shm 池/缓冲 → 请求拷贝 → 资源挂回。
    /// （建池要只读借用 self.shm、挂回要可变借用——拆成独立方法分段借用）
    #[allow(clippy::too_many_arguments)] // wayland 事件字段本来就这么多
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

        let file = tempfile::tempfile().expect("创建临时文件");
        file.set_len(size).expect("设定文件长度");
        let mmap = unsafe { memmap2::MmapMut::map_mut(&file).expect("mmap") };

        let shm = self.shm.as_ref().expect("没有 wl_shm？");
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
                "wl_output" => {
                    // 绑全部输出；udata = 索引
                    let idx = state.outputs.len();
                    let output = registry.bind(name, version.min(4), qh, idx);
                    state.outputs.push(OutputState {
                        output,
                        name: String::new(),
                        logical_pos: (0, 0),
                        scale: 1.,
                        transform: OutputTransform::Normal,
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
                o.transform = transform.into_result().unwrap_or(OutputTransform::Normal);
            }
            wl_output::Event::Scale { factor } => o.scale = factor as f32,
            _ => {}
        }
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
            // compositor 告知这帧的格式/尺寸/行距 → 造 shm buffer 并请求拷贝
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
