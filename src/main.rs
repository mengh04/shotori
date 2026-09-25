//! # Saccade · 覆盖层（spike #1 最终形态，base 自绘路线）
//!
//! 架构决策存档（详见 ROADMAP.md）：
//! - 覆盖层 = 裸 `cx.open_window`，绝不包 Root（Root 的 CSD 装饰栈会：
//!   刷主题背景挡桌面 + set_client_inset(20) 膨胀窗口 + padding 内缩内容）
//! - `gpui_kit::base::init`：只初始化 base 层，不注册任何 Root 插件
//! - 四边全锚 + exclusive_zone(-1) + Exclusive 键盘 + 半透明暗幕（已像素级验证）

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;

gpui_kit::actions!([QuitOverlay]);

pub struct Overlay {
    /// 按键分发沿焦点路径走，根节点必须可聚焦
    focus_handle: FocusHandle,
}

impl Overlay {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Render for Overlay {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("saccade-overlay")
            .size_full()
            .track_focus(&self.focus_handle)
            // 变暗层的真相：compositor 没有"变暗"功能，我们自己就是那层半透明黑
            .bg(rgba(0x000000B3))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .px_6()
                    .py_4()
                    .rounded_lg()
                    .bg(rgba(0x16161DE6))
                    .border_1()
                    .border_color(rgba(0xFF6A00FF))
                    .child(
                        div()
                            .text_size(px(22.))
                            .text_color(rgba(0xFFFFFFFF))
                            .child("Saccade · 覆盖层（base 自绘路线）"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(13.))
                            .text_color(rgba(0xAAAAAAFF))
                            .child("暗幕铺满 · 半透明正常 · Esc 退出"),
                    ),
            )
    }
}

fn main() {
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);

    app.run(|cx| {
        // base 自绘路线：不注册 component 的 Root 插件（投毒源拔除）
        gpui_kit::base::init(cx);

        cx.bind_keys([KeyBinding::new("escape", QuitOverlay, None)]);
        cx.on_action(|_: &QuitOverlay, cx| cx.quit());

        let options = WindowOptions {
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: true,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: "saccade-overlay".into(),
                layer: Layer::Overlay,
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                exclusive_zone: Some(px(-1.)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(options, |_, cx| cx.new(Overlay::new))
            .expect("打开 layer-shell 窗口失败");
    });
}
