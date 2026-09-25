//! # 覆盖层 HUD：选区相关的纯视觉元素
//!
//! 变暗边带 / 选区边框 + 尺寸标签 / 底部提示条。无状态，全函数式；
//! 装配在 [`crate::overlay::Overlay::render`]。

use gpui_kit::*;

use crate::theme::{ACCENT, CHIP_BG, DIM, HINT_TEXT};

/// 变暗层：无选区时整屏一块；有选区时挖空选区（四条边带）
pub(crate) fn dim_strips(sel: Option<Bounds<Pixels>>, ws: Size<Pixels>) -> Vec<AnyElement> {
    let mut els = Vec::new();
    let mut strip = |x: Pixels, y: Pixels, w: Pixels, h: Pixels| {
        if w > px(0.) && h > px(0.) {
            els.push(
                div()
                    .absolute()
                    .left(x)
                    .top(y)
                    .w(w)
                    .h(h)
                    .bg(rgba(DIM))
                    .into_any_element(),
            );
        }
    };

    match sel {
        None => strip(px(0.), px(0.), ws.width, ws.height),
        Some(b) => {
            strip(px(0.), px(0.), ws.width, b.top()); // 上
            strip(px(0.), b.bottom(), ws.width, ws.height - b.bottom()); // 下
            strip(px(0.), b.top(), b.left(), b.size.height); // 左
            strip(b.right(), b.top(), ws.width - b.right(), b.size.height); // 右
        }
    }
    els
}

/// 选区边框 + 尺寸标签（标签放选区上方，空间不够放下方）
pub(crate) fn selection_chrome(b: Bounds<Pixels>) -> impl IntoElement {
    let label_y = if b.top() >= px(34.) {
        b.top() - px(30.)
    } else {
        b.bottom() + px(6.)
    };

    div()
        .absolute()
        .left(b.left())
        .top(b.top())
        .w(b.size.width)
        .h(b.size.height)
        .border_1()
        .border_color(rgba(ACCENT))
        .child(
            div()
                .absolute()
                .left(px(0.))
                .top(label_y - b.top())
                .px_2()
                .py(px(2.))
                .rounded(px(4.))
                .bg(rgba(ACCENT))
                .text_size(px(12.))
                .text_color(rgba(0xFFFFFFFF))
                .child(format!(
                    "{} × {}",
                    f32::from(b.size.width).round() as i32,
                    f32::from(b.size.height).round() as i32
                )),
        )
}

/// 底部操作提示条
pub(crate) fn hint_bar() -> impl IntoElement {
    div()
        .absolute()
        .bottom(px(24.))
        .left_0()
        .w_full()
        .flex()
        .justify_center()
        .child(
            div()
                .px_4()
                .py_1()
                .rounded_lg()
                .bg(rgba(CHIP_BG))
                .text_size(px(13.))
                .text_color(rgba(HINT_TEXT))
                .child("拖拽框选 · Enter 复制 · Ctrl+S 保存 · Ctrl+O OCR · Esc 退出"),
        )
}
