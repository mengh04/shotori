//! # Saccade 入口：装配、键位、开窗
//!
//! 模块结构（避免 god file）：
//! - `capture`：screencopy 捕获库（独立 wayland 连接）
//! - `clipboard`：剪贴板复制（zwlr_data_control + 后台分身驻留）
//! - `overlay`：覆盖层（选区状态机、冻结背景、复制/保存/贴图动作）
//! - `pin`：贴图窗口（挂起中）
//! - `theme`：视觉常量

use gpui_kit::*;

use saccade::capture::{self, Capture};
use saccade::clipboard;
use saccade::overlay::{CopySelection, Overlay, PinSelection, QuitOverlay, SaveSelection};
use saccade::pin::ClosePin;

fn main() {
    // 剪贴板分身：复制动作的后台驻留进程（见 clipboard.rs 的驻留 offer 模型）
    if std::env::args().nth(1).as_deref() == Some(clipboard::DAEMON_ARG) {
        if let Err(e) = clipboard::daemon_main() {
            eprintln!("[saccade] 剪贴板分身退场：{e:#}");
            std::process::exit(1);
        }
        return;
    }

    // ① 冻结屏幕（必须在覆盖层出现之前完成）
    let cap = match capture::capture_first_output() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[saccade] 捕获失败：{e:#}");
            std::process::exit(1);
        }
    };
    println!(
        "[saccade] 已冻结 {}（{}x{}）",
        cap.output_name, cap.width, cap.height
    );

    // ② 覆盖层
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);

    app.run(move |cx| {
        gpui_kit::base::init(cx);

        // 键位按 key_context 分域：覆盖层和贴图各有自己的 Esc 语义
        cx.bind_keys([
            KeyBinding::new("escape", QuitOverlay, Some("SaccadeOverlay")),
            KeyBinding::new("enter", CopySelection, Some("SaccadeOverlay")),
            KeyBinding::new("ctrl-c", CopySelection, Some("SaccadeOverlay")),
            KeyBinding::new("ctrl-s", SaveSelection, Some("SaccadeOverlay")),
            KeyBinding::new("p", PinSelection, Some("SaccadeOverlay")),
            KeyBinding::new("escape", ClosePin, Some("SaccadePin")),
        ]);
        // 兜底：覆盖层焦点意外丢失时 Esc 仍能退出
        cx.on_action(|_: &QuitOverlay, cx| cx.quit());

        // 开窗放进异步任务：displays() 在同步启动阶段恒为空（上游 zed#46378），
        // 但事件循环首圈后即可用——在这里能拿到 display_id 把覆盖层钉在捕获的屏上
        cx.spawn(async move |cx| {
            let display_id = wait_display(&cap, cx).await;
            cx.update(|cx| {
                cx.open_window(Overlay::window_options(display_id), |window, cx| {
                    cx.new(|cx| Overlay::new(cap, display_id, window, cx))
                })
                .expect("打开 layer-shell 窗口失败");
            });
        })
        .detach();
    });
}

/// 等 displays() 可用并挑出捕获的那块屏（bounds 尺寸匹配；上限 1s，
/// 超时退回 None——沿用 compositor 自选落点的旧行为，至少能开窗）
async fn wait_display(cap: &Capture, cx: &gpui_kit::AsyncApp) -> Option<DisplayId> {
    let want_w = px(cap.width as f32);
    let want_h = px(cap.height as f32);
    for _ in 0..20 {
        let picked = cx.update(|cx| {
            let displays = cx.displays();
            displays
                .iter()
                .find(|d| {
                    let b = d.bounds();
                    // scale=1 的屏逻辑=物理，精确匹配（HDMI 主屏；异缩放匹配待 vendor 补 name）
                    b.size.width == want_w && b.size.height == want_h
                })
                .map(|d| d.id())
                .or_else(|| displays.first().map(|d| d.id()))
        });
        if let Some(id) = picked {
            return Some(id);
        }
        cx.background_executor()
            .timer(std::time::Duration::from_millis(50))
            .await;
    }
    None
}
