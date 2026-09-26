//! # Selection toolbar: actions + annotation tool controls
//!
//! Buttons dispatch the exact same actions as the keyboard through
//! `dispatch_action` — one action, two triggers, one pipeline. Row two
//! (annotation tools, colors, widths) only appears while a tool is
//! active. Geometry lives in [`crate::model::placement`].
use gpui_kit::{assets::IconName, base::Button, *};

use crate::model::session::ScreenshotSession;
use crate::ui::theme;

use crate::actions::{
    CopySelection, OcrSelection, QuitOverlay, SaveSelection, ToggleArrow, ToggleEllipse,
    ToggleHighlighter, ToggleLine, ToggleMosaic, ToggleNumber, TogglePencil, TogglePolyline,
    ToggleRectangle,
};
use crate::model::placement::{ROW_H, TB_H, TB_W, toolbar_anchor};

// The default GPUI asset bundle does not include every toolbar icon.
gpui_kit::assets::icon_assets!(pub ToolbarAssets, [MirrorRectangular, Highlighter, Pencil, Square, Circle, Slash, Waypoints, ArrowUpRight, ListOrdered, ScanText, Save, X, Copy]);

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
    let filter_tool = matches!(
        annotations.tool(),
        Some(crate::annotation::ShapeKind::Mosaic | crate::annotation::ShapeKind::Blur)
    );
    let highlighter_tool = annotations.tool() == Some(crate::annotation::ShapeKind::Highlighter);
    let number_tool = annotations.tool() == Some(crate::annotation::ShapeKind::Number);
    let selected_width = if number_tool {
        annotations.number_size()
    } else {
        annotations.width()
    };
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
                    .selected(annotations.tool() == Some(crate::annotation::ShapeKind::Rectangle)),
                )
                .child(
                    icon_button(
                        "tb-ellipse",
                        "Ellipse · E",
                        IconName::Circle,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(ToggleEllipse), cx);
                        },
                    )
                    .selected(annotations.tool() == Some(crate::annotation::ShapeKind::Ellipse)),
                )
                .child(
                    icon_button(
                        "tb-line",
                        "Line · L",
                        IconName::Slash,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(ToggleLine), cx);
                        },
                    )
                    .selected(annotations.tool() == Some(crate::annotation::ShapeKind::Line)),
                )
                .child(
                    icon_button(
                        "tb-polyline",
                        "Polyline · P",
                        IconName::Waypoints,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(TogglePolyline), cx);
                        },
                    )
                    .selected(annotations.tool() == Some(crate::annotation::ShapeKind::Polyline)),
                )
                .child(
                    icon_button(
                        "tb-arrow",
                        "Arrow · A",
                        IconName::ArrowUpRight,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(ToggleArrow), cx);
                        },
                    )
                    .selected(annotations.tool() == Some(crate::annotation::ShapeKind::Arrow)),
                )
                .child(
                    icon_button(
                        "tb-number",
                        "Sequence number · N",
                        IconName::ListOrdered,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(ToggleNumber), cx);
                        },
                    )
                    .selected(number_tool),
                )
                .child(
                    icon_button(
                        "tb-pencil",
                        "Pencil · B",
                        IconName::Pencil,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(TogglePencil), cx);
                        },
                    )
                    .selected(annotations.tool() == Some(crate::annotation::ShapeKind::Pencil)),
                )
                .child(
                    icon_button(
                        "tb-highlighter",
                        "Highlighter · H",
                        IconName::Highlighter,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(ToggleHighlighter), cx);
                        },
                    )
                    .selected(
                        annotations.tool() == Some(crate::annotation::ShapeKind::Highlighter),
                    ),
                )
                .child(
                    control(
                        "tb-mosaic".into(),
                        "Mosaic / Blur · M".into(),
                        focus.clone(),
                        |window, cx| window.dispatch_action(Box::new(ToggleMosaic), cx),
                    )
                    .selected(filter_tool)
                    .child(mosaic_icon()),
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
            if filter_tool {
                options = options.w(px(178.));
                for (id, label, kind) in [
                    (
                        "tb-pixelate",
                        "Mosaic",
                        crate::annotation::ShapeKind::Mosaic,
                    ),
                    ("tb-blur", "Blur", crate::annotation::ShapeKind::Blur),
                ] {
                    let session = session.clone();
                    options = options.child(
                        control(
                            id.into(),
                            label.into(),
                            settings_focus.clone(),
                            move |_, cx| {
                                session.update(cx, |s, cx| {
                                    s.edit_annotations(|a| {
                                        if a.tool() != Some(kind) {
                                            a.toggle(kind);
                                        }
                                    });
                                    cx.notify();
                                });
                            },
                        )
                        .selected(annotations.tool() == Some(kind))
                        .child(
                            if kind == crate::annotation::ShapeKind::Mosaic {
                                mosaic_icon().into_any_element()
                            } else {
                                svg()
                                    .path(IconName::MirrorRectangular.path())
                                    .size(px(18.))
                                    .text_color(rgba(theme::c().toolbar_text))
                                    .into_any_element()
                            },
                        ),
                    );
                }
                options = options.child(separator());
                for (ix, label) in ["Low", "Medium", "High"].into_iter().enumerate() {
                    let session = session.clone();
                    options = options.child(
                        control(
                            format!("tb-strength-{ix}"),
                            format!("Effect strength: {label}"),
                            settings_focus.clone(),
                            move |_, cx| {
                                session.update(cx, |s, cx| {
                                    s.edit_annotations(|a| a.set_width(ix));
                                    cx.notify();
                                });
                            },
                        )
                        .w(px(26.))
                        .selected(annotations.width() == [8., 16., 24.][ix])
                        .child(
                            div()
                                .size(px([4., 7., 10.][ix]))
                                .rounded(px(1.))
                                .bg(rgba(theme::c().toolbar_text)),
                        ),
                    );
                }
                return options;
            }

            let sizes = if number_tool {
                [24., 32., 40.]
            } else if highlighter_tool {
                [12., 20., 32.]
            } else {
                [1., 3., 5.]
            };
            for (ix, width) in sizes.into_iter().enumerate() {
                let session = session.clone();
                options = options.child(
                    control(
                        if number_tool {
                            format!("tb-number-size-{ix}")
                        } else {
                            format!("tb-width-{ix}")
                        },
                        if number_tool {
                            format!("Marker size: {width} px")
                        } else {
                            format!("Line width: {width} px")
                        },
                        settings_focus.clone(),
                        move |_, cx| {
                            session.update(cx, |s, cx| {
                                s.edit_annotations(|a| {
                                    if number_tool {
                                        a.set_number_size(ix)
                                    } else {
                                        a.set_width(ix)
                                    }
                                });
                                cx.notify();
                            })
                        },
                    )
                    .w(px(26.))
                    .selected(selected_width == width)
                    .child(if number_tool {
                        div()
                            .text_size(px(12.))
                            .child(["S", "M", "L"][ix])
                            .into_any_element()
                    } else {
                        div()
                            .size(px(if highlighter_tool {
                                4. + ix as f32 * 3.
                            } else {
                                width + 2.
                            }))
                            .rounded_full()
                            .bg(rgba(theme::c().toolbar_text))
                            .into_any_element()
                    }),
                );
            }
            options = options.child(separator()).child(div().flex_1());
            for (ix, color) in theme::c().annotation_colors.into_iter().enumerate() {
                let name = theme::PALETTE_NAMES[ix];
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
                            .border_color(rgba(theme::c().swatch_border)),
                    ),
                );
            }
            options
        }))
}

/// Soft gray checkerboard with rounded cells.
fn mosaic_icon() -> Div {
    div()
        .size(px(18.))
        .flex()
        .flex_col()
        .children((0..3).map(|row| {
            div().flex().children((0..3).map(move |col| {
                div().size(px(6.)).rounded(px(2.)).flex_shrink_0().bg(rgba(
                    if (row + col) % 2 == 0 {
                        theme::c().mosaic_dark
                    } else {
                        theme::c().mosaic_light
                    },
                ))
            }))
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
        .bg(rgba(theme::c().toolbar_bg))
        .border_1()
        .border_color(rgba(theme::c().toolbar_border))
        .shadow_md()
}

fn separator() -> Div {
    div()
        .w(px(1.))
        .h(px(18.))
        .mx(px(4.))
        .bg(rgba(theme::c().toolbar_border))
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
            .text_color(rgba(theme::c().toolbar_text)),
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
        .text_color(rgba(theme::c().toolbar_text))
        .hover(|s| s.bg(rgba(theme::c().toolbar_hover)))
        .focus_visible(|s| s.border_1().border_color(rgba(theme::c().accent)))
        .styles(|s| {
            s.selected(|s| {
                s.bg(rgba(theme::c().toolbar_selected))
                    .text_color(rgba(theme::c().accent))
                    .border_1()
                    .border_color(rgba(theme::c().accent))
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
            .bg(rgba(theme::c().toolbar_bg))
            .border_1()
            .border_color(rgba(theme::c().toolbar_border))
            .text_size(px(12.))
            .text_color(rgba(theme::c().toolbar_text))
            .child(self.0.clone())
    }
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
            IconName::MirrorRectangular,
            IconName::Highlighter,
            IconName::Pencil,
            IconName::Square,
            IconName::Circle,
            IconName::Slash,
            IconName::Waypoints,
            IconName::ArrowUpRight,
            IconName::ListOrdered,
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
