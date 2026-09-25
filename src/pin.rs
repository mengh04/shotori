//! # 贴图窗口：Layer::Top 的可拖动小窗口
//!
//! - 定位：layer-shell"伪绝对坐标"技巧——锚 TOP|LEFT + margin=(y,0,0,x)
//! - 拖动：动态改 margin（vendor patch：`Window::set_layer_margin`），
//!   窗口本身移动，语义上"窗口=贴图"
//! - 生命周期：Esc 关闭本贴图；所有窗口关闭后 app 退出

use std::sync::Arc;

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;
use image::{Frame, ImageBuffer};
use smallvec::SmallVec;

use crate::theme::{ACCENT, PIN_BORDER};

gpui_kit::actions!([ClosePin]);

pub struct PinWindow {
    focus_handle: FocusHandle,
    image: Arc<RenderImage>,
    /// 贴图当前逻辑位置（输出坐标系；锚 TOP|LEFT + margin 实现定位）
    pos: Point<Pixels>,
    /// 拖动中：(按下时的鼠标位置, 按下时的贴图位置)
    drag: Option<(Point<Pixels>, Point<Pixels>)>,
}

impl PinWindow {
    /// 用裁剪好的物理像素（RGBA）创建贴图，初始逻辑位置 pos
    pub fn new(
        rgba: Vec<u8>,
        w: u32,
        h: u32,
        pos: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // RenderImage 契约：BGRA（gpui Vulkan 后端要求）
        let mut bgra = rgba;
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let buf = ImageBuffer::from_raw(w, h, bgra).expect("像素尺寸不一致");
        let image = Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buf), 1)));

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            focus_handle,
            image,
            pos,
            drag: None,
        }
    }

    /// 开贴图窗口的 WindowOptions（锚左上 + margin 伪绝对定位）
    pub fn window_options(pos: Point<Pixels>, logical_size: Size<Pixels>) -> WindowOptions {
        WindowOptions {
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: true,
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: logical_size,
            })),
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: "saccade-pin".into(),
                layer: Layer::Top, // 普通窗口之上、覆盖层之下
                anchor: Anchor::TOP | Anchor::LEFT,
                // CSS 顺序 top,right,bottom,left → 表面出现在 (pos.x, pos.y)
                margin: Some((pos.y, px(0.), px(0.), pos.x)),
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// 把当前位置翻译成 layer-shell margin（锚 TOP|LEFT → margin 即坐标）
    fn apply_pos(&self, window: &mut Window) {
        window.set_layer_margin(self.pos.y, px(0.), px(0.), self.pos.x);
    }
}

impl Render for PinWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);

        div()
            .id("saccade-pin")
            .key_context("SaccadePin")
            .size_full()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &ClosePin, window, cx| {
                // 关掉这张贴图；若已是最后一个窗口，退出 app（不留僵尸进程）
                window.remove_window();
                if cx.windows().is_empty() {
                    cx.quit();
                }
            }))
            // ── 拖动：改 margin，窗口"自己走" ──────────────────
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, _cx| {
                    this.drag = Some((ev.position, this.pos));
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, cx| {
                if let Some((start, base)) = this.drag {
                    this.pos = base + (ev.position - start);
                    this.apply_pos(window);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _cx| {
                    this.drag = None;
                }),
            )
            // ── 内容 ──────────────────────────────────────────
            .child(img(self.image.clone()).size_full())
            .border_1()
            .border_color(if focused { rgba(ACCENT) } else { rgba(PIN_BORDER) })
    }
}
