//! # 屏幕捕获库：wlr-screencopy 的封装（多输出版）
//!
//! 子模块：
//! - [`pixels`]：纯像素处理（格式转换/transform 旋转），带单元测试
//! - [`wayland`]：wayland 事件状态机（Dispatch 全家）
//!
//! 一次性同步捕获：独立 Wayland 连接，单屏 ~15ms，三屏 ~350ms（含编码）。
//! 与 gpui 的 Wayland 连接互不干扰。

mod pixels;
mod wayland;

/// 一次成功的捕获（一块屏）
pub struct Capture {
    pub output_name: String,
    /// 输出的全局逻辑位置（wl_output::Geometry）
    pub logical_pos: (i32, i32),
    /// wl_output 报的 scale——**整数版**（1.5x 屏会报 2）。gpui 的 display
    /// bounds origin = 逻辑位置 ÷ 这个值（backend 自己除的，对拍实测），
    /// 匹配时必须用同一套算法（见 crate::display）
    pub scale: f32,
    /// 输出几何 transform（DP-2 是 90°：物理 buffer 横的，屏幕竖的）
    pub transform: wayland::OutputTransform,
    /// 物理尺寸（**已按 transform 旋转后**，与屏幕所见方向一致）
    pub width: u32,
    pub height: u32,
    /// 已经转成 RGBA8、已处理 Y 翻转、已按 transform 旋转的像素
    pub rgba: Vec<u8>,
}

impl Capture {
    /// 输出是否带旋转变换（日志/调试用）
    pub fn rotated(&self) -> bool {
        self.transform != wayland::OutputTransform::Normal
    }

    /// 仅供其他模块的单元测试构造（正常路径走 [`capture_all_outputs`]）
    #[cfg(test)]
    pub(crate) fn for_test(logical_pos: (i32, i32), scale: f32) -> Self {
        Self {
            output_name: "测试屏".into(),
            logical_pos,
            scale,
            transform: wayland::OutputTransform::Normal,
            width: 1920,
            height: 1080,
            rgba: Vec::new(),
        }
    }
}

/// 捕获所有输出（至少要有一块，否则报错）
pub fn capture_all_outputs() -> anyhow::Result<Vec<Capture>> {
    use wayland::*;

    let conn = wayland_client::Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());

    let mut app = App::default();
    queue.roundtrip(&mut app)?; // globals 到手（含全部 wl_output）

    let manager = app
        .manager
        .take()
        .ok_or_else(|| anyhow::anyhow!("compositor 不支持 zwlr_screencopy_manager_v1"))?;
    if app.outputs.is_empty() {
        anyhow::bail!("没有可用的 wl_output");
    }
    queue.roundtrip(&mut app)?; // 各输出的 name/geometry/scale 到手

    // 每块屏各开一个捕获帧
    for i in 0..app.outputs.len() {
        let output = app.outputs[i].output.clone();
        let frame = manager.capture_output(0, &output, &qh, i);
        app.outputs[i].frame = Some(FrameState::new(frame));
    }
    queue.roundtrip(&mut app)?; // buffer 事件 → 各自建 shm buffer 并请求拷贝

    while !app.all_frames_done() {
        queue.blocking_dispatch(&mut app)?;
    }

    // 收集成功的那部分（单屏失败不拖累其他屏）
    let mut caps = Vec::new();
    for o in &mut app.outputs {
        let Some(f) = o.frame.as_mut() else { continue };
        if f.failed {
            eprintln!("[saccade] {} 捕获失败，跳过", o.name);
            continue;
        }
        let (format, w, h, stride, y_invert) =
            f.take_frame_info().expect("ready 了必有 buffer 信息");
        let mmap = f.mmap.take().expect("没有 mmap");
        // 物理 buffer 是"躺"的，按输出 transform 旋成屏幕所见方向
        let rgba = pixels::convert_to_rgba(&mmap[..], format, w, h, stride, y_invert);
        let rgba = pixels::rotate_rgba(rgba, w as u32, h as u32, o.transform);
        let (rw, rh) = pixels::rotated_size(w as u32, h as u32, o.transform);
        caps.push(Capture {
            output_name: o.name.clone(),
            logical_pos: o.logical_pos,
            scale: o.scale,
            transform: o.transform,
            width: rw,
            height: rh,
            rgba,
        });
    }
    if caps.is_empty() {
        anyhow::bail!("所有输出的捕获都失败了");
    }
    Ok(caps)
}
