//! # 选区工具条：保存 / 贴图 / 取消
//!
//! 按钮点击通过 `dispatch_action` 走和键盘完全相同的动作管线——
//! "按钮、快捷键三位一体"不是口号，是同一个动作的三种触发方式。
//! 出现时机由 overlay 决定：只在选区松手定型后（[`crate::selection::Selection::is_selected`]）。

use gpui_kit::base::Button;
use gpui_kit::*;

use crate::overlay::{ConfirmSelection, PinSelection, QuitOverlay};
use crate::theme;

/// 工具条：摆放在选区左下角下方 8px（空间不够放上方），水平夹在屏幕内
pub fn selection_toolbar(b: Bounds<Pixels>, ws: Size<Pixels>) -> impl IntoElement {
    // 估算工具条尺寸（3 按钮 + 间距 + padding），够 v1 用
    const TB_W: f32 = 240.;
    const TB_H: f32 = 40.;
    let y = if f32::from(b.bottom()) + TB_H + 8. <= f32::from(ws.height) {
        b.bottom() + px(8.)
    } else {
        b.top() - px(TB_H) - px(8.)
    };
    let x = f32::from(b.left()).clamp(8., f32::from(ws.width) - TB_W - 8.);

    div()
        .id("saccade-toolbar")
        .absolute()
        .left(px(x))
        .top(y)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_lg()
        .bg(rgba(theme::CHIP_BG))
        .border_1()
        .border_color(rgba(theme::ACCENT))
        // 关键：工具条区域点击不冒泡到根节点——否则点按钮会触发"开始新选区"
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(toolbar_button("tb-save", "保存", |window, cx| {
            window.dispatch_action(Box::new(ConfirmSelection), cx);
        }))
        .child(toolbar_button("tb-pin", "贴图", |window, cx| {
            window.dispatch_action(Box::new(PinSelection), cx);
        }))
        .child(toolbar_button("tb-cancel", "取消", |window, cx| {
            window.dispatch_action(Box::new(QuitOverlay), cx);
        }))
}

/// 自绘工具条按钮：base 版 Button 出行为（点击/焦点/hover 状态机/无障碍），
/// 我们只画皮——gpui-base 自绘路线的标准姿势。
fn toolbar_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Button::new(id)
        .on_click(move |_, window, cx| on_click(window, cx))
        .px_3()
        .py_1()
        .rounded(px(6.))
        .text_size(px(13.))
        .text_color(rgba(theme::BTN_TEXT))
        .hover(|s| s.bg(rgba(theme::BTN_HOVER_BG)))
        .child(label)
}
