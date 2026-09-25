//! # 截图覆盖层：冻结屏幕 + 选区交互的装配层
//!
//! 流程：冻结画面打底（img）→ 拖拽框选（选区"透视"，四周变暗）
//! → Enter 复制到剪贴板 / Ctrl+S 保存 PNG / P 贴图（挂起中）/ Esc 退出。
//!
//! 分工：纯逻辑在 [`crate::selection`]（状态机）和 [`crate::export`]（裁剪/编码），
//! 视觉在 [`crate::hud`] 和 [`crate::toolbar`]——本文件只做 gpui 装配：
//! 窗口、事件 → 状态机调用、状态 → 渲染。

use std::sync::Arc;

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;

use crate::capture::Capture;
use crate::hud::{dim_strips, hint_bar, selection_chrome};
use crate::image_util;
use crate::pin::PinWindow;
use crate::selection::Selection;
use crate::toolbar::selection_toolbar;

gpui_kit::actions!([QuitOverlay, CopySelection, SaveSelection, PinSelection]);

pub struct Overlay {
    focus_handle: FocusHandle,
    /// 冻结的屏幕画面（给 img 元素显示）
    frozen: Arc<RenderImage>,
    /// 原始像素（裁剪用）
    capture: Capture,
    /// 本覆盖层所在的屏（开贴图窗口时钉同一块屏）
    display_id: Option<DisplayId>,
    selection: Selection,
}

impl Overlay {
    pub fn new(
        capture: Capture,
        display_id: Option<DisplayId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let frozen = image_util::rgba_to_render_image(
            capture.rgba.clone(),
            capture.width,
            capture.height,
        );

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        let debug_targeted = debug_targeted(&capture.output_name);
        if debug_targeted {
            spawn_debug_copy(window, cx);
        }

        Self {
            focus_handle,
            frozen,
            capture,
            display_id,
            selection: debug_selection(debug_targeted),
        }
    }

    /// 覆盖层窗口的 WindowOptions（四边全锚铺满 + Exclusive 键盘）。
    /// display_id：钉在捕获的那块屏上（不给的话 compositor 自己挑——多屏=抽签）
    pub fn window_options(display_id: Option<DisplayId>) -> WindowOptions {
        WindowOptions {
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: true,
            display_id,
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: "saccade-overlay".into(),
                layer: Layer::Overlay,
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                exclusive_zone: Some(px(-1.)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// 本窗口"捕获物理像素 ÷ 逻辑像素"的换算率。
    /// 不用 window.scale_factor()：多屏异缩放下 gpui 会报别的输出的 scale
    /// （实测：窗口钉在 HDMI 渲染按 1.0，scale_factor() 却报 DP-2 的 1.5）。
    /// 自算与渲染天然自洽，免疫错报。
    fn render_scale(&self, window: &Window) -> f32 {
        let ws = window.bounds().size;
        if f32::from(ws.width) > 0. {
            self.capture.width as f32 / f32::from(ws.width)
        } else {
            window.scale_factor()
        }
    }

    /// 逻辑选区 → 物理像素裁剪；空选区返回 None
    fn crop(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> Option<(u32, u32, Vec<u8>)> {
        crate::export::crop(
            &self.capture.rgba,
            self.capture.width,
            self.capture.height,
            bounds,
            self.render_scale(window),
        )
    }

    /// 待处理的选区；无选区 = 整窗（= 整个输出），所有截图工具的默认行为
    fn selection_or_full(&self, window: &mut Window) -> Bounds<Pixels> {
        self.selection.bounds().unwrap_or_else(|| Bounds {
            origin: Point::default(),
            size: window.bounds().size,
        })
    }

    /// Enter / Ctrl+C / 工具条[复制]：裁剪 → PNG → 剪贴板（分身驻留）→ 退出。
    /// 日常使用的第一出口。
    fn copy_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (w, h, rgba) = match self.crop(self.selection_or_full(window), window) {
            Some(x) => x,
            None => {
                println!("[saccade] 选区为空，忽略");
                return;
            }
        };
        let png = match crate::export::encode_png(w, h, &rgba) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[saccade] PNG 编码失败：{e:#}");
                return;
            }
        };
        if let Err(e) = crate::clipboard::copy_image(png) {
            // 失败留在覆盖层：用户还能 Ctrl+S 保存文件
            eprintln!("[saccade] 复制失败：{e:#}");
            return;
        }
        println!(
            "[saccade] 已复制 {w}x{h}（来自 {}）到剪贴板",
            self.capture.output_name
        );
        cx.quit();
    }

    /// Ctrl+S / 工具条[保存]：裁剪 → PNG → 落盘。失败留在覆盖层（可重试/Esc 退出）
    fn save_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (w, h, out) = match self.crop(self.selection_or_full(window), window) {
            Some(x) => x,
            None => {
                println!("[saccade] 选区为空，忽略");
                return;
            }
        };

        let path = match crate::export::next_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[saccade] 保存路径失败：{e:#}");
                return;
            }
        };
        if let Err(e) = crate::export::save_png(&path, w, h, &out) {
            eprintln!("[saccade] 保存失败：{e:#}");
            return;
        }

        println!(
            "[saccade] 已保存 {w}x{h}（来自 {}）→ {}",
            self.capture.output_name,
            path.display()
        );
        cx.quit();
    }

    /// P：把选区裁出来钉在屏幕上（贴图）——功能挂起中，保持原样可用
    fn pin_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.selection.bounds() else {
            return;
        };
        let Some((w, h, rgba)) = self.crop(bounds, window) else {
            println!("[saccade] 选区为空，忽略");
            return;
        };

        let pos = point(bounds.left(), bounds.top());
        let logical_size = size(bounds.size.width, bounds.size.height);

        cx.open_window(
            PinWindow::window_options(pos, logical_size, self.display_id),
            |window, cx| cx.new(|cx| PinWindow::new(rgba, w, h, pos, window, cx)),
        )
        .expect("打开贴图窗口失败");

        println!(
            "[saccade] 已贴图 {w}x{h} @ ({}, {})——按住拖动，Esc 关闭",
            f32::from(pos.x).round() as i32,
            f32::from(pos.y).round() as i32
        );

        // 关闭覆盖层窗口（app 继续活着伺候贴图）
        window.remove_window();
    }
}

impl Render for Overlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sel = self.selection.bounds();
        let ws = window.bounds().size; // 窗口逻辑尺寸（= 输出逻辑尺寸）

        div()
            .id("saccade-overlay")
            .key_context("SaccadeOverlay")
            .size_full()
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &CopySelection, window, cx| {
                this.copy_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveSelection, window, cx| {
                this.save_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &PinSelection, window, cx| {
                this.pin_selection(window, cx);
            }))
            // 两段 Esc：拖拽中 = 只放弃本次拖拽（吞掉动作不冒泡）；
            // 松手后（Idle/Selected）= 不处理，冒泡到 main.rs 的全局兜底 → 退出
            .on_action(cx.listener(|this, _: &QuitOverlay, _, cx| {
                if this.selection.is_dragging() {
                    this.selection.cancel_drag();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            // ── 选区交互（事件 → 状态机）─────────────────────
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    this.selection.begin(ev.position);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.selection.drag_to(ev.position) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    this.selection.end(ev.position);
                    cx.notify();
                }),
            )
            // ── 图层堆栈（从底到顶）───────────────────────────
            // ① 冻结的屏幕画面（不透明，铺满）
            .child(img(self.frozen.clone()).size_full())
            // ② 变暗层：无选区=全屏；有选区=四条边带（选区"透视"）
            .children(dim_strips(sel, ws))
            // ③ 选区边框 + 尺寸标签（拖拽中实时显示）
            .children(sel.map(selection_chrome))
            // ④ 工具条：只在松手定型后出现（拖拽中不闪）
            .children(
                if let Selection::Selected { bounds } = self.selection {
                    Some(selection_toolbar(bounds, ws))
                } else {
                    None
                },
            )
            // ⑤ 底部提示条
            .child(hint_bar())
    }
}

// ── 开发后门（自动化 e2e 的入口，正常启动不受影响）───────────────────

/// 后门是否作用于本覆盖层：SACCADE_DEBUG_TARGET=<输出名> 限定
/// （多屏下每个覆盖层都会跑到这里，全开会互相打架）；不设 = 全部生效
fn debug_targeted(output_name: &str) -> bool {
    std::env::var("SACCADE_DEBUG_TARGET")
        .map(|t| t == output_name)
        .unwrap_or(true)
}

/// SACCADE_DEBUG_SELECTION=x,y,w,h：注入现成选区
fn debug_selection(targeted: bool) -> Selection {
    if !targeted {
        return Selection::Idle;
    }
    std::env::var("SACCADE_DEBUG_SELECTION")
        .ok()
        .and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|n| n.trim().parse().ok()).collect();
            (v.len() == 4).then(|| Selection::Selected {
                bounds: Bounds {
                    origin: point(px(v[0]), px(v[1])),
                    size: size(px(v[2]), px(v[3])),
                },
            })
        })
        .unwrap_or(Selection::Idle)
}

/// SACCADE_DEBUG_ACTION=copy：1.5s 后自动触发复制——无头 e2e 的唯一入口
/// （虚拟指针在 niri 上不可用，见 ROADMAP）
fn spawn_debug_copy(window: &mut Window, cx: &mut Context<Overlay>) {
    if std::env::var("SACCADE_DEBUG_ACTION").ok().as_deref() != Some("copy") {
        return;
    }
    let win = window.window_handle();
    cx.spawn(async move |this, cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(1500))
            .await;
        let _ = win.update(cx, |_, window, cx| {
            let _ = this.update(cx, |overlay, cx| overlay.copy_selection(window, cx));
        });
    })
    .detach();
}
