//! # display 匹配：把每块捕获钉回"它截的那块屏"
//!
//! **为什么需要匹配**：layer surface 不指定 output 时 compositor 自选落点
//! （多屏 = 抽签，即"240Hz 隐形"的真凶）；gpui 的 `WindowOptions.display_id`
//! 可以钉屏，但 id 在 gpui 的连接里，与捕获连接的 wl_output 无直接对应——
//! 只能靠几何信息对号入座。
//!
//! **匹配算法（只有一条规则，务必别加复杂度）**：
//! gpui 的 display bounds origin = 输出逻辑位置 ÷ wl_output 整数 scale
//! （backend 自己除的，对拍实测：eDP 1920,0→960,0；DP-2 -720,-100→-360,-50）。
//! 位置在多屏布局里唯一 → 只比位置，尺寸弃用（分数 scale 拿不到真值，见
//! [`crate::capture::Capture::scale`]）。
//!
//! **时机**：displays() 在同步启动阶段恒为空（上游 zed#46378），事件循环
//! 首圈后可用 → [`await_display_ids`] 在异步任务里轮询（上限 1s）。

use gpui_kit::*;

use crate::capture::Capture;

/// 等 displays() 可用并逐屏匹配。超时（1s）的屏返回 None——
/// 沿用 compositor 自选落点的旧行为，至少能开窗。
pub async fn await_display_ids(
    caps: Vec<Capture>,
    cx: &AsyncApp,
) -> Vec<(Capture, Option<DisplayId>)> {
    let mut targets: Vec<(Capture, Option<DisplayId>)> =
        caps.into_iter().map(|c| (c, None)).collect();
    for attempt in 0..20 {
        let all_matched = cx.update(|cx| match_all(cx, &mut targets, attempt == 19));
        if all_matched {
            break;
        }
        cx.background_executor()
            .timer(std::time::Duration::from_millis(50))
            .await;
    }
    targets
}

/// 一轮匹配；`final_round` 时才打印诊断（避免 50ms 一条刷屏）
fn match_all(cx: &App, targets: &mut [(Capture, Option<DisplayId>)], final_round: bool) -> bool {
    let displays = cx.displays();
    if displays.is_empty() {
        return false;
    }
    let mut diag = Vec::new();
    for (cap, did) in targets.iter_mut() {
        if did.is_none() {
            *did = displays
                .iter()
                .find(|d| display_matches(&d.bounds(), cap))
                .map(|d| d.id());
            if did.is_none() {
                diag.push(format!(
                    "{} 在 gpui 坐标应为 {:?}，实际 displays={:?}",
                    cap.output_name,
                    expected_origin(cap),
                    displays
                        .iter()
                        .map(|d| {
                            let b = d.bounds();
                            (f32::from(b.origin.x) as i32, f32::from(b.origin.y) as i32)
                        })
                        .collect::<Vec<_>>()
                ));
            }
        }
    }
    if final_round {
        for d in &diag {
            eprintln!("[shotori] 未匹配：{d}");
        }
    }
    targets.iter().all(|(_, d)| d.is_some())
}

/// 捕获的输出在 gpui 坐标系里应有的 origin（逻辑位置 ÷ 整数 scale）
fn expected_origin(cap: &Capture) -> (f32, f32) {
    (
        cap.logical_pos.0 as f32 / cap.scale,
        cap.logical_pos.1 as f32 / cap.scale,
    )
}

/// 匹配谓词：origin 对上（±2px 容差防浮点毛刺）即认定同一块屏
fn display_matches(bounds: &Bounds<Pixels>, cap: &Capture) -> bool {
    let (ex, ey) = expected_origin(cap);
    (f32::from(bounds.origin.x) - ex).abs() < 2.0
        && (f32::from(bounds.origin.y) - ey).abs() < 2.0
}

// tests 模块不用 `use super::*`：父模块顶部的 `use gpui_kit::*` 会把 gpui 的
// test 宏带进来遮蔽内建 #[test]（宏展开爆递归，详见 selection.rs 注释）
#[cfg(test)]
mod tests {
    use super::{display_matches, expected_origin};
    use crate::capture::Capture;
    use gpui_kit::{point, px, size, Bounds, Pixels};

    fn cap(pos: (i32, i32), scale: f32) -> Capture {
        Capture::for_test(pos, scale)
    }

    fn bounds(x: f32, y: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(1920.), px(1080.)),
        }
    }

    #[test]
    fn 匹配_三屏实测定坐标() {
        // 用户三屏：HDMI (0,0)@1x；eDP (1920,0)@2x→gpui(960,0)；DP-2 (-720,-100)@2.0(报整数)→(-360,-50)
        let hdmi = cap((0, 0), 1.);
        let edp = cap((1920, 0), 2.);
        let dp2 = cap((-720, -100), 2.);

        assert!(display_matches(&bounds(0., 0.), &hdmi));
        assert!(display_matches(&bounds(960., 0.), &edp));
        assert!(display_matches(&bounds(-360., -50.), &dp2));

        // 互相不能匹配错
        assert!(!display_matches(&bounds(960., 0.), &hdmi));
        assert!(!display_matches(&bounds(-360., -50.), &edp));
    }

    #[test]
    fn 匹配_容差2px() {
        let hdmi = cap((0, 0), 1.);
        assert!(display_matches(&bounds(1.5, -1.5), &hdmi)); // ±1.5px 内
        assert!(!display_matches(&bounds(2.5, 0.), &hdmi)); // 超界
    }

    #[test]
    fn 匹配_负坐标除法() {
        // -720/2 = -360；-100/2 = -50（负数整除方向不影响 f32 除法）
        let dp2 = cap((-720, -100), 2.);
        assert_eq!(expected_origin(&dp2), (-360., -50.));
    }
}
