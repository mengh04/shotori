//! Compact screenshot toolbar and rectangle appearance controls.
use gpui_kit::{assets::IconName, base::Button, *};

use crate::{
    overlay::{CopySelection, OcrSelection, QuitOverlay, SaveSelection, ToggleRectangle},
    session::ScreenshotSession,
    theme,
};

// The default GPUI asset bundle does not include every toolbar icon.
gpui_kit::assets::icon_assets!(pub ToolbarAssets, [Square, ScanText, Save, X, Copy]);

const TB_W: f32 = 332.;
const ROW_H: f32 = 38.;
pub(crate) const TB_H: f32 = ROW_H * 2. + 6.;
const EDGE_B: f32 = 12.;

pub(crate) fn selection_toolbar(
    b: Bounds<Pixels>,
    ws: Size<Pixels>,
    annotations: &crate::annotation::Annotations,
    session: Entity<ScreenshotSession>,
    focus: FocusHandle,
) -> impl IntoElement {
    let height = if annotations.enabled() { TB_H } else { ROW_H };
    let (x, y) = toolbar_anchor(&b, ws, height);
    let selected_color = annotations.color().0;
    let selected_width = annotations.width();
    let settings_focus = focus.clone();

    div()
        .id("shotori-toolbar")
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(TB_W.min((f32::from(ws.width) - 16.).max(1.))))
        .flex()
        .flex_col()
        .gap(px(6.))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            bar()
                .child(
                    icon_button(
                        "tb-rectangle",
                        "Rectangle · R",
                        IconName::Square,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(ToggleRectangle), cx);
                        },
                    )
                    .selected(annotations.enabled()),
                )
                .child(div().flex_1())
                .child(icon_button(
                    "tb-ocr",
                    "Recognize text · Ctrl+O",
                    IconName::ScanText,
                    focus.clone(),
                    |window, cx| {
                        window.dispatch_action(Box::new(OcrSelection), cx);
                    },
                ))
                .child(icon_button(
                    "tb-save",
                    "Save · Ctrl+S",
                    IconName::Save,
                    focus.clone(),
                    |window, cx| {
                        window.dispatch_action(Box::new(SaveSelection), cx);
                    },
                ))
                .child(separator())
                .child(icon_button(
                    "tb-cancel",
                    "Cancel · Esc",
                    IconName::X,
                    focus.clone(),
                    |window, cx| {
                        window.dispatch_action(Box::new(QuitOverlay), cx);
                    },
                ))
                .child(icon_button(
                    "tb-copy",
                    "Copy · Enter / Ctrl+C",
                    IconName::Copy,
                    focus,
                    |window, cx| {
                        window.dispatch_action(Box::new(CopySelection), cx);
                    },
                )),
        )
        .children(annotations.enabled().then(|| {
            let mut options = bar();
            for (ix, width) in [1., 3., 5.].into_iter().enumerate() {
                let session = session.clone();
                options = options.child(
                    control(
                        format!("tb-width-{ix}"),
                        format!("Line width: {width} px"),
                        settings_focus.clone(),
                        move |_, cx| {
                            session.update(cx, |s, cx| {
                                s.edit_annotations(|a| a.set_width(ix));
                                cx.notify();
                            })
                        },
                    )
                    .w(px(26.))
                    .selected(selected_width == width)
                    .child(
                        div()
                            .size(px(width + 2.))
                            .rounded_full()
                            .bg(rgba(theme::TOOLBAR_TEXT)),
                    ),
                );
            }
            options = options.child(separator()).child(div().flex_1());
            for (ix, (color, name)) in theme::ANNOTATION_COLORS.into_iter().enumerate() {
                let session = session.clone();
                options = options.child(
                    control(
                        format!("tb-color-{ix}"),
                        format!("Color: {name}"),
                        settings_focus.clone(),
                        move |_, cx| {
                            session.update(cx, |s, cx| {
                                s.edit_annotations(|a| a.set_color(ix));
                                cx.notify();
                            })
                        },
                    )
                    .w(px(28.))
                    .selected(selected_color == color)
                    .child(
                        div()
                            .size(px(20.))
                            .rounded(px(4.))
                            .bg(rgba(color))
                            .border_1()
                            .border_color(rgba(theme::TOOLBAR_BORDER)),
                    ),
                );
            }
            options
        }))
}

fn bar() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .h(px(ROW_H))
        .px(px(5.))
        .rounded(px(8.))
        .bg(rgba(theme::TOOLBAR_BG))
        .border_1()
        .border_color(rgba(theme::TOOLBAR_BORDER))
        .shadow_md()
}

fn separator() -> Div {
    div()
        .w(px(1.))
        .h(px(18.))
        .mx(px(4.))
        .bg(rgba(theme::TOOLBAR_BORDER))
}

fn icon_button(
    id: &'static str,
    label: &'static str,
    icon: IconName,
    focus: FocusHandle,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    control(id.to_owned(), label.to_owned(), focus, on_click).child(
        svg()
            .path(icon.path())
            .size(px(18.))
            .text_color(rgba(theme::TOOLBAR_TEXT)),
    )
}

/// Restore the canvas focus BEFORE changing toolbar state. A focused control
/// may disappear on redraw (e.g. leaving rectangle mode), otherwise scoped
/// shortcut dispatch no longer has the screenshot root in its focus path.
fn control(
    id: String,
    label: String,
    focus: FocusHandle,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    let selector = id.clone();
    let tooltip: SharedString = label.clone().into();
    Button::new(SharedString::from(id))
        .debug_selector(move || selector)
        .accessibility_label(label)
        .tooltip(move |_, cx| cx.new(|_| ToolbarTooltip(tooltip.clone())).into())
        .on_click(move |_, window, cx| {
            window.focus(&focus, cx);
            on_click(window, cx);
        })
        .size(px(30.))
        .flex_shrink_0()
        .rounded(px(5.))
        .text_color(rgba(theme::TOOLBAR_TEXT))
        .hover(|s| s.bg(rgba(theme::TOOLBAR_HOVER)))
        .focus_visible(|s| s.border_1().border_color(rgba(theme::ACCENT)))
        .styles(|s| {
            s.selected(|s| {
                s.bg(rgba(theme::TOOLBAR_SELECTED))
                    .text_color(rgba(theme::ACCENT))
                    .border_1()
                    .border_color(rgba(theme::ACCENT))
            })
        })
}

struct ToolbarTooltip(SharedString);
impl Render for ToolbarTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(5.))
            .bg(rgba(theme::TOOLBAR_BG))
            .border_1()
            .border_color(rgba(theme::TOOLBAR_BORDER))
            .text_size(px(12.))
            .text_color(rgba(theme::TOOLBAR_TEXT))
            .child(self.0.clone())
    }
}

pub(crate) fn toolbar_anchor(b: &Bounds<Pixels>, ws: Size<Pixels>, height: f32) -> (f32, f32) {
    let inside = f32::from(b.bottom()) + height + 8. + EDGE_B > f32::from(ws.height);
    let x = (f32::from(b.left()) + if inside { 12. } else { 0. })
        .clamp(8., (f32::from(ws.width) - TB_W - 8.).max(8.));
    let y = if inside {
        f32::from(b.bottom()) - height - 8.
    } else {
        f32::from(b.bottom()) + 8.
    };
    (x, y.clamp(8., (f32::from(ws.height) - height - 8.).max(8.)))
}

#[cfg(test)]
mod tests {
    use super::{TB_H, TB_W, toolbar_anchor};
    use gpui_kit::{Bounds, point, px, size};
    #[test]
    fn toolbar_clamps_horizontally() {
        let b = Bounds::new(point(px(1800.), px(100.)), size(px(100.), px(100.)));
        assert_eq!(
            toolbar_anchor(&b, size(px(1920.), px(1080.)), TB_H).0,
            1920. - TB_W - 8.
        );
    }
    #[test]
    fn toolbar_icons_are_bundled() {
        use gpui_kit::{AssetSource, assets::IconName};
        for icon in [
            IconName::Square,
            IconName::ScanText,
            IconName::Save,
            IconName::X,
            IconName::Copy,
        ] {
            assert!(
                super::ToolbarAssets
                    .load(icon.path().as_ref())
                    .unwrap()
                    .is_some()
            );
        }
    }
}
