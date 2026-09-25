//! # 截图覆盖层：冻结屏幕 + 选区交互
//!
//! 流程：冻结画面打底（img）→ 拖拽框选（选区"透视"，四周变暗）
//! → Enter 保存 PNG / P 贴图 / Esc 退出。

use std::sync::Arc;

use gpui_kit::base::Button;
use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui_kit::*;
use image::{Frame, ImageBuffer};
use smallvec::SmallVec;

use crate::capture::Capture;
use crate::pin::PinWindow;
use crate::theme::{ACCENT, CHIP_BG, DIM};

gpui_kit::actions!([QuitOverlay, ConfirmSelection, PinSelection]);

/// 选区状态机
enum Selection {
    Idle,
    Dragging {
        start: Point<Pixels>,
        current: Point<Pixels>,
    },
    Selected {
        bounds: Bounds<Pixels>,
    },
}

pub struct Overlay {
    focus_handle: FocusHandle,
    /// 冻结的屏幕画面（给 img 元素显示）
    frozen: Arc<RenderImage>,
    /// 原始像素（裁剪用）
    capture: Capture,
    selection: Selection,
}

impl Overlay {
    pub fn new(capture: Capture, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // RenderImage 的契约是 BGRA 字节（gpui 的 img.rs 解码路径同样做了
        // RGBA→BGRA 转换；我们跳过解码直接喂内存，必须自己交换 R/B）
        let mut bgra = capture.rgba.clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let buf =
            ImageBuffer::from_raw(capture.width, capture.height, bgra).expect("像素尺寸不一致");
        let frozen = Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buf), 1)));

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        Self {
            focus_handle,
            frozen,
            capture,
            // 开发后门：SACCADE_DEBUG_SELECTION=x,y,w,h 注入现成选区
            // （自动化验证工具条/选区渲染用，正常启动不受影响）
            selection: std::env::var("SACCADE_DEBUG_SELECTION")
                .ok()
                .and_then(|s| {
                    let v: Vec<f32> = s
                        .split(',')
                        .filter_map(|n| n.trim().parse().ok())
                        .collect();
                    if v.len() == 4 {
                        Some(Selection::Selected {
                            bounds: Bounds {
                                origin: point(px(v[0]), px(v[1])),
                                size: size(px(v[2]), px(v[3])),
                            },
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or(Selection::Idle),
        }
    }

    /// 覆盖层窗口的 WindowOptions（四边全锚铺满 + Exclusive 键盘）
    pub fn window_options() -> WindowOptions {
        WindowOptions {
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
        }
    }

    /// 当前选区（拖拽中也算，实时显示）
    fn selection_bounds(&self) -> Option<Bounds<Pixels>> {
        match &self.selection {
            Selection::Idle => None,
            Selection::Dragging { start, current } => Some(Bounds::from_corners(*start, *current)),
            Selection::Selected { bounds } => Some(*bounds),
        }
    }

    /// 从冻结像素裁出选区（物理像素），返回 (w, h, rgba)
    fn crop(&self, bounds: Bounds<Pixels>, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        let cap = &self.capture;
        let clamp = |v: f32, max: u32| v.round().clamp(0., max as f32) as u32;
        let x0 = clamp(f32::from(bounds.left()) * scale, cap.width);
        let y0 = clamp(f32::from(bounds.top()) * scale, cap.height);
        let x1 = clamp(f32::from(bounds.right()) * scale, cap.width);
        let y1 = clamp(f32::from(bounds.bottom()) * scale, cap.height);
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if w == 0 || h == 0 {
            return None;
        }
        let mut out = vec![0u8; (w * h * 4) as usize];
        let cw4 = (cap.width as usize) * 4;
        for row in 0..h as usize {
            let src = (y0 as usize + row) * cw4 + x0 as usize * 4;
            let dst = row * (w as usize * 4);
            out[dst..dst + w as usize * 4]
                .copy_from_slice(&cap.rgba[src..src + w as usize * 4]);
        }
        Some((w, h, out))
    }

    /// Enter：从冻结像素里裁出选区，保存 PNG
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.selection_bounds() else {
            return;
        };
        let Some((w, h, out)) = self.crop(bounds, window.scale_factor()) else {
            println!("[saccade] 选区为空，忽略");
            return;
        };

        // 保存到 ~/Pictures/Saccade/
        let dir = std::path::PathBuf::from(
            std::env::var("HOME").expect("没有 HOME 环境变量"),
        )
        .join("Pictures/Saccade");
        std::fs::create_dir_all(&dir).ok();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let path = dir.join(format!("saccade_{ts}.png"));

        let file = std::fs::File::create(&path).expect("创建文件失败");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .expect("png header")
            .write_image_data(&out)
            .expect("png 数据");

        println!(
            "[saccade] 已保存 {w}x{h}（来自 {}）→ {}",
            self.capture.output_name,
            path.display()
        );
        cx.quit();
    }

    /// P：把选区裁出来钉在屏幕上（贴图）
    fn pin_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.selection_bounds() else {
            return;
        };
        let Some((w, h, rgba)) = self.crop(bounds, window.scale_factor()) else {
            println!("[saccade] 选区为空，忽略");
            return;
        };

        let pos = point(bounds.left(), bounds.top());
        let logical_size = size(bounds.size.width, bounds.size.height);

        cx.open_window(PinWindow::window_options(pos, logical_size), |window, cx| {
            cx.new(|cx| PinWindow::new(rgba, w, h, pos, window, cx))
        })
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
        let sel = self.selection_bounds();
        let ws = window.bounds().size; // 窗口逻辑尺寸（= 输出逻辑尺寸）

        div()
            .id("saccade-overlay")
            .key_context("SaccadeOverlay")
            .size_full()
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ConfirmSelection, window, cx| {
                this.confirm(window, cx);
            }))
            .on_action(cx.listener(|this, _: &PinSelection, window, cx| {
                this.pin_selection(window, cx);
            }))
            // ── 选区交互 ─────────────────────────────────────
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    // 按下即开始新选区（Selected 状态下重新框选也是它）
                    this.selection = Selection::Dragging {
                        start: ev.position,
                        current: ev.position,
                    };
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Selection::Dragging { current, .. } = &mut this.selection {
                    if *current != ev.position {
                        *current = ev.position;
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    if let Selection::Dragging { start, .. } = this.selection {
                        this.selection = Selection::Selected {
                            bounds: Bounds::from_corners(start, ev.position),
                        };
                        cx.notify();
                    }
                }),
            )
            // ── 图层堆栈（从底到顶）───────────────────────────
            // ① 冻结的屏幕画面（不透明，铺满）
            .child(img(self.frozen.clone()).size_full())
            // ② 变暗层：无选区=全屏；有选区=四条边带（选区"透视"）
            .children(dim_strips(sel, ws))
            // ③ 选区边框 + 尺寸标签
            .children(sel.map(selection_chrome))
            // ③.5 工具条：选区确定后浮现（保存/贴图/取消），键盘按钮同管线
            .children(sel.map(|b| selection_toolbar(b, ws)))
            // ④ 底部提示条
            .child(
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
                            .text_color(rgba(0xAAAAAAFF))
                            .child("拖拽框选 · Enter 保存 · P 贴图 · Esc 退出"),
                    ),
            )
    }
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

/// 选区工具条：保存 / 贴图 / 取消。
/// 按钮点击通过 dispatch_action 走和键盘完全相同的动作管线——
/// "按钮、快捷键、菜单三位一体"不是口号，是同一行代码的三种触发方式。
fn selection_toolbar(b: Bounds<Pixels>, ws: Size<Pixels>) -> impl IntoElement {
    // 估算工具条尺寸（3 按钮 + 间距 + padding），够 v1 用
    const TB_W: f32 = 240.;
    const TB_H: f32 = 40.;
    // 首选：选区左下角下方 8px；空间不够放上方；水平方向夹在屏幕内
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
        .bg(rgba(crate::theme::CHIP_BG))
        .border_1()
        .border_color(rgba(crate::theme::ACCENT))
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
/// 点击回调里 dispatch_action —— 和键盘 Enter/P/Esc 走同一条动作管线，
/// "按钮、快捷键三位一体"不是口号，是同一个动作的三种触发方式。
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
        .text_color(rgba(0xEEEEEEFF))
        .hover(|s| s.bg(rgba(0xFFFFFF26)))
        .child(label)
}

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
