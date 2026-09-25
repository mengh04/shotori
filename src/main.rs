//! # Saccade 入口：装配、键位、开窗
//!
//! 模块结构（避免 god file）：
//! - `capture`：screencopy 捕获库（独立 wayland 连接）
//! - `overlay`：覆盖层（选区状态机、冻结背景、保存/贴图动作）
//! - `pin`：贴图窗口（拖动、生命周期）
//! - `theme`：视觉常量

use gpui_kit::*;

use saccade::capture;
use saccade::overlay::{ConfirmSelection, Overlay, PinSelection, QuitOverlay};
use saccade::pin::ClosePin;

fn main() {
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
            KeyBinding::new("enter", ConfirmSelection, Some("SaccadeOverlay")),
            KeyBinding::new("p", PinSelection, Some("SaccadeOverlay")),
            KeyBinding::new("escape", ClosePin, Some("SaccadePin")),
        ]);
        // 兜底：覆盖层焦点意外丢失时 Esc 仍能退出
        cx.on_action(|_: &QuitOverlay, cx| cx.quit());

        cx.open_window(Overlay::window_options(), |window, cx| {
            cx.new(|cx| Overlay::new(cap, window, cx))
        })
        .expect("打开 layer-shell 窗口失败");
    });
}
