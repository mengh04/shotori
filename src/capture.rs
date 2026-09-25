//! # 屏幕捕获库：wlr-screencopy 的封装
//!
//! 独立 Wayland 连接 → 绑 global → 申请一帧 → 共享内存收像素 → 转 RGBA8。
//! 与 gpui 的 Wayland 连接互不干扰（一次性同步捕获，~15ms）。
//!
//! v1 限制：只捕获 registry 里第一个 wl_output（与覆盖层落点一致性靠 compositor
//! 的选择，多屏场景待 ext-image-copy-capture / display_id 方案，见 ROADMAP）。

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

/// 一次成功的捕获
pub struct Capture {
    pub output_name: String,
    pub width: u32,
    pub height: u32,
    /// 已经转成 RGBA8、已处理 Y 翻转的像素（可直接喂给 gpui 的 RenderImage）
    pub rgba: Vec<u8>,
}

/// 捕获第一个输出（整屏）
pub fn capture_first_output() -> anyhow::Result<Capture> {
    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());

    let mut app = App::default();
    queue.roundtrip(&mut app)?; // globals 到手

    let manager = app
        .manager
        .take()
        .ok_or_else(|| anyhow::anyhow!("compositor 不支持 zwlr_screencopy_manager_v1"))?;
    let output = app
        .output
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("没有可用的 wl_output"))?
        .clone();

    let _frame = manager.capture_output(0, &output, &qh, ());
    queue.roundtrip(&mut app)?; // buffer 事件 → copy 请求

    while !app.ready && !app.failed {
        queue.blocking_dispatch(&mut app)?;
    }
    if app.failed {
        anyhow::bail!("compositor 拒绝了这次截图（failed 事件）");
    }

    let (format, w, h, stride, y_invert, output_name) = app
        .take_frame_info(app.output_name.clone())
        .expect("没收到 buffer 事件");

    let mmap = app.mmap.take().expect("没有 mmap");
    let rgba = convert_to_rgba(&mmap[..], format, w, h, stride, y_invert);

    Ok(Capture {
        output_name,
        width: w as u32,
        height: h as u32,
        rgba,
    })
}

/// wl_shm 格式名描述 32 位字的位序（MSB→LSB），小端内存字节序正好相反：
///   Xrgb8888（XR24）→ 内存 B,G,R,X   Argb8888 → 内存 B,G,R,A
///   Xbgr8888（XB24）→ 内存 R,G,B,X   Abgr8888 → 内存 R,G,B,A
/// 注意：wl_shm 核心协议的 format 是序号（xrgb8888=1），不是 DRM fourcc！
fn convert_to_rgba(
    bytes: &[u8],
    format: wl_shm::Format,
    w: i32,
    h: i32,
    stride: i32,
    y_invert: bool,
) -> Vec<u8> {
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let is_xrgb = matches!(format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888);

    for y in 0..h as usize {
        let src_row = &bytes[y * stride as usize..][..(w * 4) as usize];
        let dst_y = if y_invert { h as usize - 1 - y } else { y };
        let dst_row = &mut rgba[dst_y * (w * 4) as usize..][..(w * 4) as usize];
        let (src_chunks, _) = src_row.as_chunks::<4>();
        let (dst_chunks, _) = dst_row.as_chunks_mut::<4>();
        for (px, chunk) in src_chunks.iter().zip(dst_chunks) {
            if is_xrgb {
                // B,G,R,(A) → R,G,B,A
                chunk[0] = px[2];
                chunk[1] = px[1];
                chunk[2] = px[0];
                chunk[3] = 255;
            } else {
                // R,G,B,(A) → R,G,B,A
                chunk[0] = px[0];
                chunk[1] = px[1];
                chunk[2] = px[2];
                chunk[3] = 255;
            }
        }
    }
    rgba
}

// ── Wayland 事件处理（每个绑定的协议对象一个 Dispatch 实现）──────────

#[derive(Default)]
struct App {
    shm: Option<wl_shm::WlShm>,
    manager: Option<ZwlrScreencopyManagerV1>,
    output: Option<wl_output::WlOutput>,
    output_name: String,

    frame_info: Option<(wl_shm::Format, i32, i32, i32)>, // (format, w, h, stride)
    y_invert: bool,
    ready: bool,
    failed: bool,

    mmap: Option<memmap2::MmapMut>,
    #[allow(dead_code)]
    buffer: Option<wl_buffer::WlBuffer>,
    #[allow(dead_code)]
    file: Option<File>,
}

impl App {
    fn take_frame_info(
        &mut self,
        output_name: String,
    ) -> Option<(wl_shm::Format, i32, i32, i32, bool, String)> {
        self.frame_info
            .take()
            .map(|(f, w, h, s)| (f, w, h, s, self.y_invert, output_name))
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
                "wl_output" if state.output.is_none() => {
                    state.output = Some(registry.bind(name, version.min(4), qh, ()))
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            state.output_name = name;
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

impl Dispatch<ZwlrScreencopyFrameV1, ()> for App {
    fn event(
        state: &mut Self,
        frame: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
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
                state.frame_info = Some((fmt, width as i32, height as i32, stride as i32));
                let size = (stride as i64 * height as i64) as u64;

                let file = tempfile::tempfile().expect("创建临时文件");
                file.set_len(size).expect("设定文件长度");
                let mmap = unsafe { memmap2::MmapMut::map_mut(&file).expect("mmap") };

                let shm = state.shm.as_ref().expect("没有 wl_shm？");
                let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
                let buffer = pool.create_buffer(
                    0,
                    width as i32,
                    height as i32,
                    stride as i32,
                    fmt,
                    qh,
                    (),
                );
                frame.copy(&buffer);

                state.file = Some(file);
                state.mmap = Some(mmap);
                state.buffer = Some(buffer);
            }
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                state.y_invert = flags
                    .into_result()
                    .is_ok_and(|f| f.contains(zwlr_screencopy_frame_v1::Flags::YInvert));
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => state.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => state.failed = true,
            _ => {}
        }
    }
}
