//! # Shotori 入口：装配、键位、开窗
//!
//! 模块结构（避免 god file）：
//! - `capture`：screencopy 捕获（多输出，独立 wayland 连接）
//! - `display`：捕获 ↔ gpui display 的匹配与等待
//! - `clipboard`：剪贴板复制（zwlr_data_control + 后台分身驻留）
//! - `overlay`：覆盖层装配（每屏一个窗口）
//! - `hud` / `toolbar`：覆盖层的视觉件
//! - `selection` / `export` / `image_util`：纯逻辑
//! - `theme`：视觉常量

use gpui_kit::*;

use shotori::capture;
use shotori::clipboard;
use shotori::display;
use shotori::overlay::{CopySelection, Overlay, QuitOverlay, SaveSelection};

fn main() {
    // 剪贴板分身：复制动作的后台驻留进程（见 clipboard.rs 的驻留 offer 模型）
    if std::env::args().nth(1).as_deref() == Some(clipboard::DAEMON_ARG) {
        if let Err(e) = clipboard::daemon_main() {
            eprintln!("[shotori] 剪贴板分身退场：{e:#}");
            std::process::exit(1);
        }
        return;
    }

    // ① 冻结全部屏幕（必须在覆盖层出现之前完成）
    let caps = match capture::capture_all_outputs() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[shotori] 捕获失败：{e:#}");
            std::process::exit(1);
        }
    };
    println!(
        "[shotori] 已冻结 {} 块屏：{}",
        caps.len(),
        caps.iter()
            .map(|c| format!(
                "{} {}x{}{}",
                c.output_name,
                c.width,
                c.height,
                if c.rotated() { "(旋转)" } else { "" }
            ))
            .collect::<Vec<_>>()
            .join(" · ")
    );

    // ② 覆盖层（每屏一个）
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::base::init(cx);

            // 键位按 key_context 分域
            cx.bind_keys([
                KeyBinding::new("escape", QuitOverlay, Some("ShotoriOverlay")),
                KeyBinding::new("enter", CopySelection, Some("ShotoriOverlay")),
                KeyBinding::new("ctrl-c", CopySelection, Some("ShotoriOverlay")),
                KeyBinding::new("ctrl-s", SaveSelection, Some("ShotoriOverlay")),
            ]);
            // 兜底：覆盖层焦点意外丢失时 Esc 仍能退出。
            // 注意：实测窗口内 dispatch_action 不会冒泡到这里（动作止步于焦点路径），
            // 覆盖层的 QuitOverlay 处理器才是真正的退出实现；这行只防焦点丢失的极端情况
            cx.on_action(|_: &QuitOverlay, cx| cx.quit());

            // 开窗放进异步任务：displays() 在同步启动阶段恒为空（上游 zed#46378），
            // 事件循环首圈后即可用——在这里给每块捕获匹配 display_id 并开窗
            cx.spawn(async move |cx| {
                let targets = display::await_display_ids(caps, cx).await;
                cx.update(|cx| {
                    for (cap, did) in targets {
                        if did.is_none() {
                            eprintln!(
                                "[shotori] 警告：{} 没匹配到 display，落点交给 compositor",
                                cap.output_name
                            );
                        }
                        cx.open_window(Overlay::window_options(did), |window, cx| {
                            cx.new(|cx| Overlay::new(cap, window, cx))
                        })
                        .expect("打开 layer-shell 窗口失败");
                    }
                });
            })
            .detach();
        });
}
