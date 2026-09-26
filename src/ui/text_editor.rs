//! Inline editor placement; text layout and input live in TextInput.
use crate::ui::{text_input::TextInput, theme};
use gpui_kit::*;

pub(crate) struct TextEditor {
    input: Entity<TextInput>,
    bounds: Bounds<Pixels>,
    local: Point<Pixels>,
    height: f32,
    visible_width: f32,
    font_size: f32,
    color: u32,
}
impl TextEditor {
    pub(crate) fn new(
        bounds: Bounds<Pixels>,
        local: Point<Pixels>,
        font_size: f32,
        color: u32,
        _: &mut Window,
        cx: &mut App,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
                font_size,
                cx,
            )
        });
        Self {
            input,
            font_size,
            color,
            bounds,
            local,
            visible_width: 1.,
            height: (font_size * 1.35).min(f32::from(bounds.size.height)),
        }
    }
    pub(crate) fn input(&self) -> &Entity<TextInput> {
        &self.input
    }
    pub(crate) fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }
    pub(crate) fn value(&self, cx: &App) -> String {
        self.input.read(cx).value()
    }
    pub(crate) fn refresh(&mut self, cx: &App) {
        self.height = self.input.read(cx).height();
        self.visible_width = self.input.read(cx).content_width();
    }
    pub(crate) fn sync_style(&mut self, size: f32, color: u32, cx: &mut App) -> bool {
        let changed = self.font_size != size || self.color != color;
        if self.font_size != size {
            self.font_size = size;
            self.input.update(cx, |s, cx| s.set_font_size(size, cx));
            self.refresh(cx);
        }
        self.color = color;
        changed
    }
    pub(crate) fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
    pub(crate) fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |s, cx| s.focus(window, cx));
    }
    pub(crate) fn render(&self) -> impl IntoElement {
        let visible = size(
            px(self.visible_width).min(self.bounds.size.width),
            px(self.height).min(self.bounds.size.height),
        );
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(editor_outline(Bounds::new(self.local, visible)))
            .child(
                div()
                    .id("text-editor")
                    .debug_selector(|| "text-editor".to_owned())
                    .absolute()
                    .left(self.local.x)
                    .top(self.local.y)
                    .w(visible.width)
                    .h(visible.height)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.input.clone()),
            )
    }
}

/// Paint from the overlay's origin, just like selection_backdrop. Outsetting
/// the border or independently rounding a positioned child's origin and size
/// can push its right edge beyond the selection at fractional DPI.
fn editor_outline(bounds: Bounds<Pixels>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |viewport, (), window, _| {
            let mut bounds = bounds;
            bounds.origin += viewport.origin;
            window.paint_quad(outline(
                bounds,
                rgba(theme::c().accent),
                BorderStyle::default(),
            ));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        Bounds, Context, IntoElement, ParentElement, Pixels, Render, Styled, TestAppContext,
        Window, div, point, px, size,
    };

    struct OutlineHarness {
        selection: Bounds<Pixels>,
        editor: Bounds<Pixels>,
    }
    impl Render for OutlineHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(crate::ui::hud::selection_backdrop(Some(self.selection)))
                .child(super::editor_outline(self.editor))
        }
    }
    #[gpui_kit::test]
    fn editor_border_never_outsets_the_selection_at_fractional_dpi(cx: &mut TestAppContext) {
        let initial = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(400.)));
        let (view, cx) = cx.add_window_view(|_, _| OutlineHarness {
            selection: initial,
            editor: initial,
        });
        for scale in [1., 1.25, 1.5, 1.73, 2.] {
            cx.simulate_scale_factor_change(scale);
            for right in [350., 350.2, 350.5, 350.8] {
                for left in [70., 70.2, 70.5, 70.8] {
                    view.update(cx, |s, cx| {
                        s.selection = Bounds::from_corners(
                            point(px(20.), px(20.)),
                            point(px(right), px(360.3)),
                        );
                        s.editor = Bounds::from_corners(
                            point(px(left), px(80.6)),
                            s.selection.bottom_right(),
                        );
                        cx.notify();
                    });
                    let quads = cx.update(|window, cx| {
                        window.draw(cx).clear(cx);
                        window.painted_quads()
                    });
                    let mut borders = quads
                        .iter()
                        .filter(|q| q.border_widths.top.0 > 0.)
                        .map(|q| q.bounds)
                        .collect::<Vec<_>>();
                    // GPUI splits each outline into four masked strips with
                    // identical outer bounds. Compare the two actual outlines.
                    borders.dedup();
                    assert_eq!(borders.len(), 2);
                    assert_eq!(
                        borders[0].right(),
                        borders[1].right(),
                        "right edge at scale {scale}"
                    );
                    assert_eq!(
                        borders[0].bottom(),
                        borders[1].bottom(),
                        "bottom edge at scale {scale}"
                    );
                }
            }
        }
    }
}
