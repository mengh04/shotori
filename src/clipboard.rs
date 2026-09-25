//! # 剪贴板：zwlr_data_control 复制 + 后台驻留分身
//!
//! **Wayland 剪贴板是"驻留 offer"模型**：数据源进程活着，剪贴板才有内容；
//! 截图工具复制完就退出 = 剪贴板瞬间清空。解法（wl-copy 同款）：
//!
//! - 复制 = 把数据经 stdin 交给一个**后台分身**（re-exec 自身
//!   [`DAEMON_ARG`]，避开多线程进程里裸 fork 的坑）
//! - 分身连接 compositor，挂数据源；谁粘贴就写数据给谁
//! - 剪贴板被别人覆盖时分身收到 `cancelled`，功成身退退出
//!
//! 于是可以连拍 N 张：每只新分身上岗，前一只自动退场，不堆积。

use std::io::{Read as _, Write as _};
use std::process::{Command, Stdio};

use anyhow::Context as _;
use wayland_client::{
    event_created_child,
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::{self, ZwlrDataControlManagerV1},
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

/// 分身模式的命令行参数（main.rs 据此分流）
pub const DAEMON_ARG: &str = "--clipboard-daemon";
/// 图片 MIME（GTK/Qt/浏览器都认）
pub const IMAGE_MIME: &str = "image/png";
/// 文本 MIME（带 charset 后缀，GTK/Qt 都能正确粘贴）
pub const TEXT_MIME: &str = "text/plain;charset=utf-8";

/// 把 PNG 字节放上剪贴板：spawn 后台分身驻留服务，立即返回。
/// 分身的生死由 compositor 的 cancelled 事件管理，调用方无需关心。
pub fn copy_image(png: Vec<u8>) -> anyhow::Result<()> {
    spawn_daemon(IMAGE_MIME, &png)
}

/// 把纯文本放上剪贴板（OCR 结果出口）。
pub fn copy_text(text: String) -> anyhow::Result<()> {
    spawn_daemon(TEXT_MIME, text.as_bytes())
}

fn spawn_daemon(mime: &str, data: &[u8]) -> anyhow::Result<()> {
    if data.is_empty() {
        anyhow::bail!("空数据，不复制");
    }
    if !manager_available() {
        anyhow::bail!("compositor 不支持 zwlr_data_control_manager_v1，无法复制到剪贴板");
    }

    let exe = std::env::current_exe().context("找不到自身可执行文件")?;
    let mut child = Command::new(exe)
        .arg(DAEMON_ARG)
        .arg(mime) // 分身入口由此决定 offer 什么
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("启动剪贴板分身失败")?;

    // 写完即关（drop）——分身读到 EOF 就去挂 offer。
    // 几 MB > 管道缓冲没关系：分身第一时间在读，不会死锁。
    child
        .stdin
        .take()
        .expect("刚指定的 piped stdin")
        .write_all(data)
        .context("向剪贴板分身传数据失败")?;
    Ok(())
}

/// 快速探一眼 compositor 是否提供 data-control（一次 roundtrip，~1ms），
/// 让"不支持"在复制的瞬间变成人话报错，而不是分身里无声失败
fn manager_available() -> bool {
    let Ok(conn) = Connection::connect_to_env() else {
        return false;
    };
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut probe = Probe::default();
    queue.roundtrip(&mut probe).is_ok() && probe.manager
}

#[derive(Default)]
struct Probe {
    manager: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Probe {
    fn event(
        state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = event
            && interface == "zwlr_data_control_manager_v1"
        {
            state.manager = true;
        }
    }
}

// ── 分身本体 ──────────────────────────────────────────────────────────

/// 分身入口：`shotori --clipboard-daemon <MIME>`
/// stdin 读全量 → 连 compositor 挂 offer → 服务粘贴请求直到被覆盖
pub fn daemon_main() -> anyhow::Result<()> {
    let mime = std::env::args()
        .nth(2)
        .unwrap_or_else(|| IMAGE_MIME.to_string());

    let mut payload = Vec::new();
    std::io::stdin()
        .read_to_end(&mut payload)
        .context("分身读 stdin 失败")?;
    if payload.is_empty() {
        anyhow::bail!("分身收到空数据，不上岗");
    }

    let conn = Connection::connect_to_env().context("分身连不上 compositor")?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());

    // 文本模式也 offer 一个降级 text/plain（某些应用只认不带 charset 后缀的）
    let fallback = if mime.starts_with("text/") {
        Some("text/plain".to_string())
    } else {
        None
    };

    let mut app = Daemon {
        payload,
        mime: mime.clone(),
        ..Daemon::default()
    };
    queue.roundtrip(&mut app)?;

    let seat = app
        .seat
        .clone()
        .ok_or_else(|| anyhow::anyhow!("没有 wl_seat"))?;
    let manager = app
        .manager
        .take()
        .ok_or_else(|| anyhow::anyhow!("compositor 不支持 zwlr_data_control_manager_v1"))?;

    let device = manager.get_data_device(&seat, &qh, ());
    let source = manager.create_data_source(&qh, ());
    source.offer(mime.to_string());
    if let Some(fb) = &fallback {
        source.offer(fb.clone());
    }
    device.set_selection(Some(&source));
    app.device = Some(device);
    app.source = Some(source);
    queue.roundtrip(&mut app)?; // 确保 set_selection 已发出

    while !app.cancelled {
        queue.blocking_dispatch(&mut app)?;
    }

    // 退场前销毁代理（连接 drop 也会清，显式做是给 compositor 好脸色）
    if let Some(s) = app.source.take() {
        s.destroy();
    }
    if let Some(d) = app.device.take() {
        d.destroy();
    }
    Ok(())
}

#[derive(Default)]
struct Daemon {
    payload: Vec<u8>,
    mime: String,
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ZwlrDataControlManagerV1>,
    device: Option<ZwlrDataControlDeviceV1>,
    source: Option<ZwlrDataControlSourceV1>,
    cancelled: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Daemon {
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
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(7), qh, ()))
                }
                "zwlr_data_control_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(2), qh, ()))
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Daemon {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for Daemon {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: zwlr_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for Daemon {
    fn event(
        _state: &mut Self,
        device: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // 我们只写不读；DataOffer/Selection 是"别人剪贴板里有啥"的通知，用不上。
        // 注意：我们 set_selection 后 compositor 会把新选区镜像回给我们，
        // 这条 DataOffer 事件也会到达——同样礼貌销毁。
        if let zwlr_data_control_device_v1::Event::DataOffer { id } = event {
            id.destroy();
            let _ = device;
        }
    }

    // compositor 会在 DataOffer 事件里替我们创建 offer 新对象，
    // wayland-rs 要求父接口特化 event_created_child（默认实现直接 panic——
    // 踩过的坑：复制后 roundtrip 立即崩，wl-paste 报 "Nothing is copied"）
    event_created_child!(Daemon, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for Daemon {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlOfferV1,
        _: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for Daemon {
    fn event(
        state: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // 有人粘贴：往对方给的 fd 里写数据（fd 由 compositor 转手，写完关闭）
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => {
                if mime_type == state.mime
                    || mime_type == "text/plain"
                {
                    let mut file = std::fs::File::from(fd); // File drop 时关闭 fd
                    let _ = file.write_all(&state.payload); // 对方管道断裂等：忽略即可
                }
                // mime 不匹配：fd（OwnedFd）在本分支末尾 drop，自动关闭
                let _ = source;
            }
            // 剪贴板被别人覆盖：退场
            zwlr_data_control_source_v1::Event::Cancelled => {
                state.cancelled = true;
            }
            _ => {}
        }
    }
}