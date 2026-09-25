//! # Saccade · Spike #1：gpui + layer-shell 覆盖层
//!
//! 这是整个项目风险最高的一块技术赌注的最小验证：
//!
//! - 第 1 讲结论：Wayland 下普通窗口（xdg_toplevel）做不了截图覆盖层，
//!   唯一正道是 layer-shell（`zwlr_layer_shell_v1`）。
//! - 第 2 讲结论：gpui-pre 0.3.6 有 `WindowKind::LayerShell` 第一公民支持，
//!   但 gpui-kit 生态里没有任何人用过——我们是第一个吃螃蟹的。
//!
//! 这个 spike 要回答四个问题：
//!   1. layer-shell 窗口在 niri 上能不能开出来（协议握手）
//!   2. `WindowBackgroundAppearance::Transparent` 能不能真的透出半透明
//!      "变暗层"（取决于 gpui 的 EGL buffer 是否带 alpha 通道）
//!   3. `KeyboardInteractivity::Exclusive` 下 Esc 能不能到我们手里（焦点链路）
//!   4. 四边锚定后 configure 是否把窗口尺寸纠正为整个输出（多屏/缩放）
//!
//! 运行：`cargo run`（在你的图形会话里）
//! 预期：整个屏幕盖上一层半透明黑，中央有提示卡片，按 Esc 退出。

use gpui_kit::*;

// gpui 的键盘处理模型是"键位 → 动作 → 处理器"三段式，
// 不在窗口里裸监听按键。先声明动作：
gpui_kit::actions!([QuitOverlay]);

/// 截图覆盖层视图。
pub struct Overlay {
    /// 焦点句柄。按键分发沿"焦点路径"冒泡，根节点必须可聚焦，
    /// 否则 keymap 匹配到了动作也没人接收。
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
            // 动作处理器：绑定在这个可聚焦元素上，Esc 命中后沿焦点路径到达这里
            .on_action(cx.listener(|_, _: &QuitOverlay, cx| {
                println!("[saccade] 收到 QuitOverlay 动作，退出");
                cx.quit();
            }))
            // ── 变暗层 ──────────────────────────────────────────────
            // 关键认知：compositor 没有"把桌面变暗"的功能。
            // 我们自己就是那层半透明黑——盖在所有窗口上面的 RGBA 表面。
            // 透不透明由 WindowOptions::window_background 决定（见 main）。
            .bg(rgba(0x000000B3)) // 70% 不透明度的黑
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .px_6()
                    .py_4()
                    .rounded(px(10.))
                    .bg(rgba(0x16161DE6))
                    .border_1()
                    .border_color(rgba(0xFF6A00FF))
                    .child(
                        div()
                            .text_size(px(22.))
                            .text_color(rgba(0xFFFFFFFF))
                            .child("Saccade · layer-shell 覆盖层已生效"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(13.))
                            .text_color(rgba(0xAAAAAAFF))
                            .child("这层半透明黑暗幕由 gpui 渲染 · 按 Esc 退出"),
                    ),
            )
    }
}

fn main() {
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);

    app.run(|cx| {
        gpui_kit::init(cx);

        // 键位绑定：escape → QuitOverlay（None 表示不限上下文，任何焦点下都触发）
        cx.bind_keys([KeyBinding::new("escape", QuitOverlay, None)]);
        // 兜底处理器：万一焦点没落在我们身上，App 层也能接住动作
        cx.on_action(|_: &QuitOverlay, cx| cx.quit());

        cx.activate(true);

        // ── 第 2 讲的核心：用 layer-shell 打开覆盖层窗口 ─────────────
        let overlay_options = WindowOptions {
            // 覆盖层没有标题栏
            titlebar: None,
            // 必须透明背景，否则"变暗"会变成"漆黑一片"
            window_background: WindowBackgroundAppearance::Transparent,
            // 初始尺寸随便给：四边锚定后，compositor 的 configure 事件
            // 会用输出的真实尺寸纠正它（多屏/分数缩放也由它兜底）
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1920.), px(1080.)),
            })),
            focus: true,
            kind: WindowKind::LayerShell(LayerShellOptions {
                // niri 的 layer-rule 靠 namespace 匹配我们（比如加特效）
                namespace: "saccade-overlay".into(),
                // 四层模型的最顶层：锁屏、通知、截图覆盖层住的地方
                layer: Layer::Overlay,
                // 四边全锚 = 自动铺满整个输出，尺寸交给 compositor
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                // 哨兵值 -1：不尊重任何保留区——盖过 waybar 等面板
                // （spike 验证点：Option<Pixels> 里塞负值能否正确映射成协议的 -1）
                exclusive_zone: Some(px(-1.)),
                exclusive_edge: None,
                margin: None,
                // 独占键盘：Esc 一定到我们手里，不会先被 compositor 截走
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
            }),
            ..Default::default()
        };

        // compositor 不支持 layer-shell 时这里会报 LayerShellNotSupportedError
        gpui_kit::open_window(overlay_options, cx, |_, cx| cx.new(Overlay::new))
            .expect("打开 layer-shell 窗口失败：compositor 不支持 zwlr_layer_shell_v1？");
    });
}
