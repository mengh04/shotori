//! # Saccade · Spike #2：裸 wayland-client 实现 wlr-screencopy 截图
//!
//! 这是"协议换像素"的完整教学实现（grim 的极简版）：
//!
//!   连接 Wayland → 绑定 zwlr_screencopy_manager_v1 + wl_shm + wl_output
//!   → capture_output 申请一帧 → compositor 回 buffer(format,w,h,stride)
//!   → 我们造一块共享内存(shm) buffer → frame.copy(buffer) 说"拷给我"
//!   → compositor 把像素写进共享内存 → ready 事件 → 读内存 → 编码 PNG
//!
//! 用法：screencap [输出路径]        （默认 /tmp/opencode/saccade_spike2.png）

use std::fs::File;
use std::io::BufWriter;
use std::os::fd::AsFd;

use wayland_client::{
    protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool},
    Connection, Dispatch, QueueHandle,
};use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};

// wl_shm 格式枚举（wl_shm::Format）直接匹配即可，无需手写 fourcc 常量
// （格式名到内存序的映射见下方转换处的注释）

/// 全局状态：wayland 回调把我们关心的信息攒在这里
#[derive(Default)]
struct App {
    shm: Option<wl_shm::WlShm>,
    manager: Option<ZwlrScreencopyManagerV1>,
    output: Option<wl_output::WlOutput>,
    output_name: String,

    // frame 生命周期
    frame_info: Option<(wl_shm::Format, i32, i32, i32)>, // (format, w, h, stride)
    y_invert: bool,
    ready: bool,
    failed: bool,

    // 共享内存三件套
    file: Option<File>,
    mmap: Option<memmap2::MmapMut>,
    buffer: Option<wl_buffer::WlBuffer>,
}

// ── 事件处理：每个绑定的协议对象一个 Dispatch 实现 ──────────────────

/// registry：compositor 广播能力清单，我们把需要的 global 绑定下来
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

/// wl_output：拿输出名字（HDMI-A-1 / eDP-1 ...），其余事件忽略
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
    fn event(_: &mut Self, _: &wl_shm::WlShm, _: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for App {
    fn event(_: &mut Self, _: &wl_shm_pool::WlShmPool, _: wl_shm_pool::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for App {
    fn event(_: &mut Self, _: &ZwlrScreencopyManagerV1, _: zwlr_screencopy_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_buffer::WlBuffer, ()> for App {
    fn event(_: &mut Self, _: &wl_buffer::WlBuffer, _: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// ⭐ screencopy frame：一次截图会话的全部事件
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
            // ① compositor 告诉我们这帧的格式/尺寸/行距
            zwlr_screencopy_frame_v1::Event::Buffer { format, width, height, stride } => {
                // 直接保留 wl_shm::Format 枚举（WEnum→u32 的转换不可靠，踩过）
                let fmt = format.into_result().unwrap_or(wl_shm::Format::Xrgb8888);
                state.frame_info = Some((fmt, width as i32, height as i32, stride as i32));
                let (_, h, stride) = (width, height as i32, stride as i32);
                let size = (stride * h) as u64;

                // ② 造共享内存：tempfile 当载体，mmap 拿到可读写的内存
                let file = tempfile::tempfile().expect("创建临时文件");
                file.set_len(size).expect("设定文件长度");
                let mmap = unsafe { memmap2::MmapMut::map_mut(&file).expect("mmap") };

                // ③ wl_shm_pool → wl_buffer：把这块内存包装成 wayland buffer
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

                // ④ 关键请求："把这帧拷进我的 buffer"
                frame.copy(&buffer);

                state.file = Some(file);
                state.mmap = Some(mmap);
                state.buffer = Some(buffer);
                // pool 可以 drop：协议保证 buffer 独立存活
            }
            // ⑤ 可选：compositor 提示这帧是 Y 翻转的（某些 GPU 布局）
            zwlr_screencopy_frame_v1::Event::Flags { flags } => {
                state.y_invert = flags
                    .into_result()
                    .is_ok_and(|f| f.contains(zwlr_screencopy_frame_v1::Flags::YInvert));
            }
            // ⑥ 像素已就位（在共享内存里）
            zwlr_screencopy_frame_v1::Event::Ready { .. } => state.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => state.failed = true,
            _ => {}
        }
    }
}

fn main() -> anyhow::Result<()> {
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/opencode/saccade_spike2.png".into());
    let t0 = std::time::Instant::now();

    // ── 1. 连接 compositor（就是第 1 讲说的那条 Unix socket）──
    let conn = Connection::connect_to_env()?;

    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());

    let mut app = App::default();
    queue.roundtrip(&mut app)?; // 同步一轮：globals 到手

    let manager = app
        .manager
        .take()
        .ok_or_else(|| anyhow::anyhow!("compositor 不支持 zwlr_screencopy_manager_v1"))?;

    // ── 2. 申请一帧（overlay_cursor=0：不画鼠标指针；指定第一个 output）──
    let output = app
        .output
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("没绑定到 wl_output"))?;
    let _frame = manager.capture_output(0, output, &qh, ());
    queue.roundtrip(&mut app)?; // 触发 buffer 事件 → 里面会发出 copy 请求

    // ── 3. 等待像素就位 ──
    while !app.ready && !app.failed {
        queue.blocking_dispatch(&mut app)?;
    }
    if app.failed {
        anyhow::bail!("compositor 拒绝了这次截图（failed 事件）");
    }

    let (format, w, h, stride) = app.frame_info.expect("没收到 buffer 事件");
    println!(
        "[screencap] 格式 {:?}(0x{:08X}) {}x{} stride={} y_invert={} 耗时 {:?}",
        format,
        u32::from(format),
        w,
        h,
        stride,
        app.y_invert,
        t0.elapsed()
    );

    // ── 4. 读共享内存 → 转 RGBA8 ──
    let mmap = app.mmap.take().expect("没有 mmap");
    let bytes = &mmap[..];
    let mut rgba = vec![0u8; (w * h * 4) as usize];

    // wl_shm 的格式名描述的是 32 位字的位序（MSB→LSB），小端内存里正好反过来：
    //   Xrgb8888（XR24）→ 内存 B,G,R,X   Argb8888（AR24）→ 内存 B,G,R,A
    //   Xbgr8888（XB24）→ 内存 R,G,B,X   Abgr8888（AB24）→ 内存 R,G,B,A
    let is_xrgb = matches!(format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888);
    for y in 0..h as usize {
        let src_row = &bytes[y * stride as usize..][..(w * 4) as usize];
        // Y 翻转：从底部往上读
        let dst_y = if app.y_invert { h as usize - 1 - y } else { y };
        let dst_row = &mut rgba[dst_y * (w * 4) as usize..][..(w * 4) as usize];
        for (px, chunk) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
            if is_xrgb {
                // B,G,R,X/A → R,G,B,A
                dst_row_left_right(px, chunk);
            } else {
                // R,G,B,X/A → R,G,B,A
                chunk[0] = px[0];
                chunk[1] = px[1];
                chunk[2] = px[2];
                chunk[3] = if format == wl_shm::Format::Abgr8888 { px[3] } else { 255 };
            }
        }
    }

    // ── 5. 编码 PNG ──
    let file = File::create(&out_path)?;
    let mut enc = png::Encoder::new(BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;

    println!("[screencap] 已保存 → {out_path}（{} KB）", std::fs::metadata(&out_path)?.len() / 1024);
    Ok(())
}

#[inline(always)]
fn dst_row_left_right(px: &[u8], chunk: &mut [u8]) {
    chunk[0] = px[2];
    chunk[1] = px[1];
    chunk[2] = px[0];
    chunk[3] = 255;
}
