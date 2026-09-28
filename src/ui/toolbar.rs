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
    CopySelection, OcrSelection, PinSelection, QuitOverlay, SaveSelection, ToggleArrow,
    ToggleEllipse, ToggleEraser, ToggleHighlighter, ToggleLine, ToggleMosaic, ToggleNumber,
    TogglePencil, TogglePolyline, ToggleRectangle, ToggleText,
};
use crate::model::placement::{GRIP_W, ROW_H};

// The default GPUI asset bundle does not include every toolbar icon.
gpui_kit::assets::icon_assets!(
    ToolbarAssets,
    [
        Eraser,
        Type,
        MirrorRectangular,
        Highlighter,
        Pencil,
        Square,
        Circle,
        Slash,
        Waypoints,
        ArrowUpRight,
        ListOrdered,
        ScanText,
        Save,
        Pin,
        X,
        Copy
    ]
);

/// The app's own icon set, embedded from `assets/icons` at build time.
/// The Lucide catalog has no true mosaic/pixelate glyph (its grids read
/// as "table"), so shotori maintains its own SVGs — Lucide conventions
/// kept (24×24 canvas, currentColor) so they tint with the toolbar text
/// color like every bundled icon.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
struct OwnIcons;

/// The application asset source: the app's own icons first, then the
/// selected Lucide icons (`icon_assets!` selects from the gpui-kit
/// bundle — see the crate docs for the composition contract). Registered
/// app-wide in `main.rs` via `with_assets`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolbarSource;

impl AssetSource for ToolbarSource {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(file) = OwnIcons::get(path) {
            return Ok(Some(file.data));
        }
        ToolbarAssets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut names: Vec<SharedString> = OwnIcons::iter()
            .filter(|name| name.starts_with(path))
            .map(Into::into)
            .collect();
        names.extend(ToolbarAssets.list(path)?);
        Ok(names)
    }
}

/// One of the app's own icons (see [`OwnIcons`]).
fn own_icon(path: &'static str) -> Svg {
    svg()
        .path(path)
        .size(px(18.))
        .text_color(rgba(theme::c().toolbar_text))
}

pub(crate) fn selection_toolbar(
    rect: Bounds<Pixels>,
    output: SharedString,
    annotations: &crate::annotation::Annotations,
    session: Entity<ScreenshotSession>,
    focus: FocusHandle,
) -> impl IntoElement {
    let selected_color = annotations.color().0;
    let filter_tool = matches!(
        annotations.tool(),
        Some(crate::annotation::ShapeKind::Mosaic | crate::annotation::ShapeKind::Blur)
    );
    let eraser_tool = matches!(
        annotations.tool(),
        Some(crate::annotation::ShapeKind::Eraser | crate::annotation::ShapeKind::EraserRect)
    );
    let highlighter_tool = annotations.tool() == Some(crate::annotation::ShapeKind::Highlighter);
    let number_tool = annotations.tool() == Some(crate::annotation::ShapeKind::Number);
    let text_tool = annotations.tool() == Some(crate::annotation::ShapeKind::Text);
    let selected_width = if text_tool {
        annotations.text_size()
    } else if number_tool {
        annotations.number_size()
    } else {
        annotations.width()
    };
    let settings_focus = focus.clone();

    div()
        .id("shotori-toolbar")
        .absolute()
        .left(rect.origin.x)
        .top(rect.origin.y)
        // No fixed width: each row hugs its own natural width (row one
        // ≈ TB_W_ROW1, the settings row is wider). The old fixed TB_W
        // stretched row one, and its flex_1 spacer ballooned into the
        // giant gap between tool cluster and action cluster. The width
        // basis still drives placement/clamping via `toolbar_bounds` —
        // rendering just no longer forces it onto every row.
        .flex()
        .flex_col()
        .items_start()
        .gap(px(6.))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            bar()
                .child(grip("tb-grip-left", output.clone(), session.clone()))
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
                    .child(own_icon("icons/mosaic.svg")),
                )
                .child(
                    icon_button(
                        "tb-eraser",
                        "Eraser · D",
                        IconName::Eraser,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(ToggleEraser), cx);
                        },
                    )
                    .selected(eraser_tool),
                )
                .child(
                    icon_button(
                        "tb-text",
                        "Text · T",
                        IconName::Type,
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(ToggleText), cx);
                        },
                    )
                    .selected(text_tool),
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
                .child(
                    // own icon: the gpui-kit asset whitelist (`icon_assets!`)
                    // does not bundle a pin, and IconName::Pin would render
                    // an empty slot — same situation as the mosaic icon
                    control(
                        "tb-pin".into(),
                        "Pin to screen · Ctrl+P".into(),
                        focus.clone(),
                        |window, cx| {
                            window.dispatch_action(Box::new(PinSelection), cx);
                        },
                    )
                    .child(own_icon("icons/pin.svg")),
                )
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
                ))
                .child(grip("tb-grip-right", output, session.clone())),
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
                                own_icon("icons/mosaic.svg")
                            } else {
                                svg()
                                    .path(IconName::MirrorRectangular.path())
                                    .size(px(18.))
                                    .text_color(rgba(theme::c().toolbar_text))
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

            if eraser_tool {
                options = options.w(px(
                    if annotations.tool() == Some(crate::annotation::ShapeKind::EraserRect) {
                        78.
                    } else {
                        170.
                    },
                ));
                for (id, label, kind, icon) in [
                    (
                        "tb-eraser-brush",
                        "Brush eraser",
                        crate::annotation::ShapeKind::Eraser,
                        IconName::Eraser,
                    ),
                    (
                        "tb-eraser-rect",
                        "Rectangle eraser",
                        crate::annotation::ShapeKind::EraserRect,
                        IconName::Square,
                    ),
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
                            svg()
                                .path(icon.path())
                                .size(px(18.))
                                .text_color(rgba(theme::c().toolbar_text)),
                        ),
                    );
                }
                if annotations.tool() == Some(crate::annotation::ShapeKind::EraserRect) {
                    return options;
                }
                options = options.child(separator());
            }

            let sizes = if eraser_tool {
                [16., 32., 48.]
            } else if text_tool {
                [16., 24., 32.]
            } else if number_tool {
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
                        if text_tool {
                            format!("tb-text-size-{ix}")
                        } else if number_tool {
                            format!("tb-number-size-{ix}")
                        } else {
                            format!("tb-width-{ix}")
                        },
                        if text_tool {
                            format!("Font size: {width} px")
                        } else if number_tool {
                            format!("Marker size: {width} px")
                        } else {
                            format!(
                                "{}: {width} px",
                                if eraser_tool {
                                    "Eraser diameter"
                                } else {
                                    "Line width"
                                }
                            )
                        },
                        settings_focus.clone(),
                        move |_, cx| {
                            session.update(cx, |s, cx| {
                                s.edit_annotation_settings(|a| {
                                    if text_tool {
                                        a.set_text_size(ix)
                                    } else if number_tool {
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
                    .child(if text_tool {
                        div()
                            .text_size(px(12.))
                            .child(format!("{width:.0}"))
                            .into_any_element()
                    } else if number_tool {
                        div()
                            .text_size(px(12.))
                            .child(["S", "M", "L"][ix])
                            .into_any_element()
                    } else {
                        div()
                            .size(px(if highlighter_tool || eraser_tool {
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
            if eraser_tool {
                return options;
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
                                s.edit_annotation_settings(|a| a.set_color(ix));
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

/// A drag strip at the toolbar's edge: press and the whole toolbar
/// follows the pointer anywhere on its layer (session-side clamping
/// keeps it inside the window). Visually a matte "grip texture" — a
/// quiet dot matrix like the textured rubber on physical devices — NOT
/// a button: no pill, no hover background. The open/closed hand cursor
/// is the affordance (see `Overlay::cursor_style`).
fn grip(
    id: &'static str,
    output: SharedString,
    session: Entity<ScreenshotSession>,
) -> impl IntoElement {
    // toolbar_text at low alpha: visible as texture, quiet as texture
    let grain = (theme::c().toolbar_text & 0xFFFFFF00) | 0x4D;
    div()
        .id(id)
        // Full row height, and offset only by the bar's side padding —
        // the element's rect is then EXACTLY `session::toolbar_grips`
        // (row one only, BAR_PAD inset, ROW_H tall): every pixel that
        // shows the hand grabs, every pixel that grabs shows the hand.
        .h(px(ROW_H))
        .w(px(GRIP_W))
        .flex()
        // gap between the dot COLUMNS — do not lose this again: without
        // it the three columns pack solid and the airy matte matrix
        // collapses into tight triple lines (user-noticed regression).
        // No justify_center either: flush-left is the exact look that
        // was approved (pixel-verified against the liked build).
        .gap(px(1.75))
        .items_center()
        .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
            session.update(cx, |s, cx| {
                if s.toolbar_drag_begin(&output, ev.position) {
                    cx.notify();
                }
            });
            cx.stop_propagation(); // the press belongs to the toolbar, not the canvas
        })
        .children((0..3).map(|_| {
            div()
                .flex()
                .flex_col()
                .gap(px(1.75))
                .children((0..7).map(|_| div().size(px(1.5)).rounded_full().bg(rgba(grain))))
        }))
}

fn bar() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .h(px(ROW_H))
        .px(px(crate::model::placement::BAR_PAD))
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
                    .text_color(rgba(theme::c().selected_text()))
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
    use crate::model::placement::{TB_W_ROW1, round_px, toolbar_anchor, toolbar_bounds};
    use gpui_kit::{Bounds, point, px, size};
    #[test]
    fn toolbar_clamps_horizontally() {
        let b = Bounds::new(point(px(1800.), px(100.)), size(px(100.), px(100.)));
        assert_eq!(
            toolbar_anchor(&b, size(px(1920.), px(1080.)), TB_W_ROW1, super::ROW_H).0,
            1920. - TB_W_ROW1 - 8.
        );
        // the composed rect: anchor + the width clamp the render side applies
        let rect = toolbar_bounds(
            &round_px(b),
            size(px(1920.), px(1080.)),
            TB_W_ROW1,
            super::ROW_H,
        );
        assert_eq!(rect.size.width, px(TB_W_ROW1));
        // narrow window: the toolbar shrinks to the window minus breathing room
        let rect = toolbar_bounds(
            &round_px(b),
            size(px(400.), px(400.)),
            TB_W_ROW1,
            super::ROW_H,
        );
        assert_eq!(rect.size.width, px(384.));
    }
    #[test]
    fn toolbar_icons_are_bundled() {
        use gpui_kit::{AssetSource, assets::IconName};
        // the app's composed source: own SVGs first, then selected Lucide
        for icon in [
            IconName::Type,
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
            IconName::Pin,
            IconName::X,
            IconName::Copy,
        ] {
            assert!(
                super::ToolbarSource
                    .load(icon.path().as_ref())
                    .unwrap()
                    .is_some()
            );
        }
        // the self-maintained mosaic icon
        assert!(
            super::ToolbarSource
                .load("icons/mosaic.svg")
                .unwrap()
                .is_some()
        );
    }
}
