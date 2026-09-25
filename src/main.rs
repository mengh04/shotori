//! # Saccade 入口：装配、键位、开窗（多屏版）
//!
//! 模块结构（避免 god file）：
//! - `capture`：screencopy 捕获库（独立 wayland 连接，全部输出）
//! - `clipboard`：剪贴板复制（zwlr_data_control + 后台分身驻留）
//! - `overlay`：覆盖层（选区状态机、冻结背景、复制/保存/贴图动作）
//! - `pin`：贴图窗口（挂起中）
//! - `theme`：视觉常量
//!
//! 多屏策略：**每块输出一个覆盖层窗口**，各自钉在自己的屏上（display_id）、
//! 各持自己的冻结画面与选区。Enter/Esc 作用于键盘焦点所在的覆盖层——
//! niri 的 exclusive layer 键盘焦点跟随"你交互的那块屏"。

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

    // ① 冻结全部屏幕（必须在覆盖层出现之前完成）
    let caps = match capture::capture_all_outputs() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[saccade] 捕获失败：{e:#}");
            std::process::exit(1);
        }
    };
    println!(
        "[saccade] 已冻结 {} 块屏：{}",
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
        // 但事件循环首圈后即可用——在这里给每块捕获匹配 display_id
        cx.spawn(async move |cx| {
            let mut targets: Vec<(Capture, Option<DisplayId>)> =
                caps.into_iter().map(|c| (c, None)).collect();
            for _ in 0..20 {
                let all_matched = cx.update(|cx| match_displays(cx, &mut targets));
                if all_matched {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
            }
            cx.update(|cx| {
                for (cap, did) in targets {
                    if did.is_none() {
                        eprintln!(
                            "[saccade] 警告：{} 没匹配到 display，落点交给 compositor",
                            cap.output_name
                        );
                    }
                    cx.open_window(Overlay::window_options(did), |window, cx| {
                        cx.new(|cx| Overlay::new(cap, did, window, cx))
                    })
                    .expect("打开 layer-shell 窗口失败");
                }
            });
        })
        .detach();
    });
}

/// 逐屏匹配 display_id：按 gpui 的坐标系算（display origin = 输出逻辑位置 ÷
/// wl_output 整数 scale，gpui backend 自己除的，对拍实测）。
/// 位置在多屏布局里唯一，尺寸弃用（分数 scale 拿不到真值）。
fn match_displays(cx: &App, targets: &mut [(Capture, Option<DisplayId>)]) -> bool {
    let displays = cx.displays();
    if displays.is_empty() {
        return false;
    }
    let mut diag = Vec::new();
    for (cap, did) in targets.iter_mut() {
        if did.is_none() {
            let ex = cap.logical_pos.0 as f32 / cap.scale;
            let ey = cap.logical_pos.1 as f32 / cap.scale;
            *did = displays
                .iter()
                .find(|d| {
                    let b = d.bounds();
                    (f32::from(b.origin.x) - ex).abs() < 2.0
                        && (f32::from(b.origin.y) - ey).abs() < 2.0
                })
                .map(|d| d.id());
            if did.is_none() {
                diag.push(format!(
                    "{} 在 gpui 坐标应为 ({ex:.0},{ey:.0})，实际 displays={:?}",
                    cap.output_name,
                    displays
                        .iter()
                        .map(|d| {
                            let b = d.bounds();
                            (
                                f32::from(b.origin.x) as i32,
                                f32::from(b.origin.y) as i32,
                            )
                        })
                        .collect::<Vec<_>>()
                ));
            }
        }
    }
    for d in &diag {
        eprintln!("[saccade] 未匹配：{d}");
    }
    targets.iter().all(|(_, d)| d.is_some())
}
