//! # 截图覆盖层：冻结屏幕 + 选区交互的装配层
//!
//! 流程：冻结画面打底（img）→ 拖拽框选（选区"透视"，四周变暗）
//! → Enter 复制到剪贴板 / Ctrl+S 保存 PNG / P 贴图（挂起中）/ Esc 退出。
//!
//! 纯逻辑分别在 [`crate::selection`]（状态机）和 [`crate::export`]（裁剪/编码/落盘），
//! 本文件只做 gpui 装配：窗口、事件 → 状态机调用、状态 → 渲染。

use std::sync::Arc;

use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;

use crate::capture::Capture;
use crate::image_util;
use crate::pin::PinWindow;
use crate::selection::Selection;
use crate::theme::{ACCENT, CHIP_BG, DIM, HINT_TEXT};
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

        // 开发后门仅作用于目标屏（多屏下每个覆盖层都会跑到这里，全开会互相打架）：
        // SACCADE_DEBUG_TARGET=<输出名> 限定；不设 = 全部生效
        let debug_targeted = std::env::var("SACCADE_DEBUG_TARGET")
            .map(|t| t == capture.output_name)
            .unwrap_or(true);

        // 开发后门：SACCADE_DEBUG_ACTION=copy，1.5s 后自动触发复制——
        // 无头 e2e 的唯一入口（虚拟指针在 niri 上不可用，见 ROADMAP）
        if debug_targeted
            && std::env::var("SACCADE_DEBUG_ACTION").ok().as_deref() == Some("copy")
        {
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

        Self {
            focus_handle,
            frozen,
            capture,
            display_id,
            selection: if debug_targeted {
                std::env::var("SACCADE_DEBUG_SELECTION")
                    .ok()
                    .and_then(|s| {
                        let v: Vec<f32> = s
                            .split(',')
                            .filter_map(|n| n.trim().parse().ok())
                            .collect();
                        (v.len() == 4).then(|| Selection::Selected {
                            bounds: Bounds {
                                origin: point(px(v[0]), px(v[1])),
                                size: size(px(v[2]), px(v[3])),
                            },
                        })
                    })
                    .unwrap_or(Selection::Idle)
            } else {
                Selection::Idle
            },
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

    /// 当前选区的物理像素裁剪；无选区 = 整窗（= 整个输出），所有截图工具的默认行为。
    /// 空选区返回 None。
    fn crop_selection_or_full(
        &self,
        window: &mut Window,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let bounds = self
            .selection
            .bounds()
            .unwrap_or_else(|| Bounds {
                origin: Point::default(),
                size: window.bounds().size,
            });
        // 不用 window.scale_factor()：多屏异缩放下 gpui 会报别的输出的 scale
        // （实测：窗口钉在 HDMI 渲染按 1.0，scale_factor() 却报 DP-2 的 1.5）。
        // 用"捕获物理宽 ÷ 窗口逻辑宽"自算——与渲染天然自洽，免疫错报。
        let ws = window.bounds().size;
        let scale = if f32::from(ws.width) > 0. {
            self.capture.width as f32 / f32::from(ws.width)
        } else {
            window.scale_factor()
        };
        crate::export::crop(
            &self.capture.rgba,
            self.capture.width,
            self.capture.height,
            bounds,
            scale,
        )
    }

    /// Enter / Ctrl+C / 工具条[复制]：裁剪 → PNG → 剪贴板（分身驻留）→ 退出。
    /// 日常使用的第一出口。
    fn copy_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((w, h, rgba)) = self.crop_selection_or_full(window) else {
            println!("[saccade] 选区为空，忽略");
            return;
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
        let Some((w, h, out)) = self.crop_selection_or_full(window) else {
            println!("[saccade] 选区为空，忽略");
            return;
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
        let Some((w, h, rgba)) = crate::export::crop(
            &self.capture.rgba,
            self.capture.width,
            self.capture.height,
            bounds,
            window.scale_factor(),
        ) else {
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

/// 底部操作提示
fn hint_bar() -> impl IntoElement {
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
                .child("拖拽框选 · Enter 复制 · Ctrl+S 保存 · Esc 退出"),
        )
}

/// 变暗层：无选区时整屏一块；有选区时挖空选区（四条边带）
fn dim_strips(sel: Option<Bounds<Pixels>>, ws: Size<Pixels>) -> Vec<AnyElement> {
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

/// 选区边框 + 尺寸标签
fn selection_chrome(b: Bounds<Pixels>) -> impl IntoElement {
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
