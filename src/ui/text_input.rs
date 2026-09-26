//! Canvas text input. Cosmic owns glyphs, wrapping, hit testing and editing;
//! GPUI's input-handler protocol supplies native IME and clipboard integration.
use crate::{annotation::text, ui::theme};
use cosmic_text::{Action as EditAction, Cursor, Edit, Editor, Metrics, Motion, Scroll, Selection};
use gpui_kit::{base::input::InputEvent, *};
use std::ops::Range;

pub(crate) struct TextInput {
    editor: Editor<'static>,
    focus: FocusHandle,
    marked: Option<Range<usize>>,
    composition_start: Option<Editor<'static>>,
    undo: Vec<Editor<'static>>,
    redo: Vec<Editor<'static>>,
    width: f32,
    limit: f32,
    font_size: f32,
    bounds: Bounds<Pixels>,
    dragging: bool,
}
impl EventEmitter<InputEvent> for TextInput {}
impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl TextInput {
    pub(crate) fn new(width: f32, limit: f32, font_size: f32, cx: &mut Context<Self>) -> Self {
        Self {
            editor: Editor::new(text::buffer("", width, limit, font_size)),
            focus: cx.focus_handle(),
            marked: None,
            composition_start: None,
            undo: Vec::new(),
            redo: Vec::new(),
            width,
            limit,
            font_size,
            bounds: Bounds::default(),
            dragging: false,
        }
    }
    pub(crate) fn value(&self) -> String {
        self.editor.with_buffer(|b| {
            b.lines
                .iter()
                .map(|l| l.text())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
    pub(crate) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }
    pub(crate) fn content_width(&self) -> f32 {
        self.editor.with_buffer(|b| {
            (b.layout_runs().map(|r| r.line_w).fold(0., f32::max) + 1.).min(self.width)
        })
    }
    pub(crate) fn height(&self) -> f32 {
        self.editor.with_buffer(|b| {
            b.layout_runs()
                .map(|r| r.line_top + r.line_height)
                .fold(self.font_size * 1.35, f32::max)
                .min(self.limit)
        })
    }
    pub(crate) fn set_font_size(&mut self, font_size: f32, cx: &mut Context<Self>) {
        self.font_size = font_size;
        self.editor
            .with_buffer_mut(|b| b.set_metrics(Metrics::new(font_size, font_size * 1.35)));
        self.changed(cx);
    }
    fn shape(&mut self) {
        text::with_fonts(|fonts| {
            self.editor.with_buffer_mut(|b| {
                b.set_metrics(Metrics::new(self.font_size, self.font_size * 1.35));
                b.set_size(Some(self.width), None);
                b.set_scroll(Scroll::default());
                b.shape_until_scroll(fonts, false);
            })
        });
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.shape();
        cx.emit(InputEvent::Change);
        cx.notify();
    }
    fn remember(&mut self) {
        self.undo.push(
            self.composition_start
                .take()
                .unwrap_or_else(|| self.editor.clone()),
        );
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    fn byte(&self, c: Cursor) -> usize {
        self.editor.with_buffer(|b| {
            b.lines
                .iter()
                .take(c.line)
                .map(|l| l.text().len() + 1)
                .sum::<usize>()
                + c.index
        })
    }
    fn cursor_for(&self, offset: usize) -> Cursor {
        let value = self.value();
        let offset = offset.min(value.len());
        let prefix = &value[..offset];
        Cursor::new(
            prefix.bytes().filter(|&b| b == b'\n').count(),
            prefix.rsplit('\n').next().unwrap_or("").len(),
        )
    }
    fn selection(&self) -> Range<usize> {
        let (start, end) = self
            .editor
            .selection_bounds()
            .unwrap_or((self.editor.cursor(), self.editor.cursor()));
        self.byte(start)..self.byte(end)
    }
    fn byte_range_for_utf16(&self, r: Range<usize>) -> Range<usize> {
        let text = self.value();
        utf16_byte(&text, r.start)..utf16_byte(&text, r.end)
    }
    fn to_utf16(&self, r: Range<usize>) -> Range<usize> {
        let text = self.value();
        text[..r.start].encode_utf16().count()..text[..r.end].encode_utf16().count()
    }
    fn replace(&mut self, range: Range<usize>, value: &str) {
        let start = self.cursor_for(range.start);
        let end = self.cursor_for(range.end);
        self.editor.set_cursor(end);
        self.editor.set_selection(Selection::Normal(start));
        self.editor.insert_string(value, None);
    }
    fn caret(&self, cursor: Cursor) -> Bounds<Pixels> {
        self.editor.with_buffer(|b| {
            b.layout_runs()
                .find_map(|r| {
                    r.cursor_position(&cursor).map(|x| {
                        Bounds::new(
                            point(px(x), px(r.line_top)),
                            size(px(1.), px(r.line_height)),
                        )
                    })
                })
                .unwrap_or(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1.), px(self.font_size * 1.35)),
                ))
        })
    }
    fn highlights(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        if range.is_empty() {
            return Vec::new();
        }
        let start = self.cursor_for(range.start);
        let end = self.cursor_for(range.end);
        self.editor.with_buffer(|b| {
            b.layout_runs()
                // Cosmic's editor also checks paragraph bounds before calling
                // highlight; runs outside both endpoints otherwise select every glyph.
                .filter(|r| r.line_i >= start.line && r.line_i <= end.line)
                .flat_map(|r| {
                    r.highlight(start, end).map(move |(x, w)| {
                        Bounds::new(point(px(x), px(r.line_top)), size(px(w), px(r.line_height)))
                    })
                })
                .collect()
        })
    }
    fn action(&mut self, a: EditAction) {
        text::with_fonts(|fonts| self.editor.action(fonts, a));
        self.shape();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_some() {
            return;
        } // the IME owns keys while composing
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        let command = mods.control || mods.platform;
        let motion = match key {
            "left" => Some(if command {
                Motion::LeftWord
            } else {
                Motion::Left
            }),
            "right" => Some(if command {
                Motion::RightWord
            } else {
                Motion::Right
            }),
            "up" => Some(Motion::Up),
            "down" => Some(Motion::Down),
            "home" => Some(if command {
                Motion::BufferStart
            } else {
                Motion::Home
            }),
            "end" => Some(if command {
                Motion::BufferEnd
            } else {
                Motion::End
            }),
            _ => None,
        };
        if let Some(motion) = motion {
            if mods.shift {
                if self.editor.selection() == Selection::None {
                    self.editor
                        .set_selection(Selection::Normal(self.editor.cursor()));
                }
            } else {
                self.editor.set_selection(Selection::None);
            }
            self.action(EditAction::Motion(motion));
        } else {
            match key {
                "enter" if !mods.shift => {
                    cx.emit(InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    });
                }
                "enter" => {
                    self.remember();
                    self.action(EditAction::Enter);
                }
                "escape" => {
                    window.dispatch_action(Box::new(crate::actions::CancelText), cx);
                }
                "backspace" | "delete" => {
                    self.remember();
                    if command && self.editor.selection_bounds().is_none() {
                        self.editor
                            .set_selection(Selection::Normal(self.editor.cursor()));
                        self.action(EditAction::Motion(if key == "backspace" {
                            Motion::PreviousWord
                        } else {
                            Motion::NextWord
                        }));
                    }
                    self.action(if key == "backspace" {
                        EditAction::Backspace
                    } else {
                        EditAction::Delete
                    });
                }
                "a" if command => {
                    self.editor
                        .set_selection(Selection::Normal(Cursor::new(0, 0)));
                    self.editor.set_cursor(self.cursor_for(self.value().len()));
                }
                "c" | "x" if command => {
                    if let Some(value) = self.editor.copy_selection() {
                        cx.write_to_clipboard(ClipboardItem::new_string(value));
                    }
                    if key == "x" {
                        self.remember();
                        self.editor.delete_selection();
                    }
                }
                "v" if command => {
                    if let Some(value) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        self.remember();
                        self.editor.insert_string(&value, None);
                    }
                }
                "z" | "y" if command => {
                    let redo = key == "y" || mods.shift;
                    let next = if redo {
                        self.redo.pop()
                    } else {
                        self.undo.pop()
                    };
                    if let Some(next) = next {
                        let old = std::mem::replace(&mut self.editor, next);
                        if redo {
                            self.undo.push(old);
                        } else {
                            self.redo.push(old);
                        }
                    }
                }
                _ => return,
            }
        }
        self.changed(cx);
        cx.stop_propagation();
        window.prevent_default();
    }
}
fn utf16_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}
impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.byte_range_for_utf16(range);
        *actual = Some(self.to_utf16(range.clone()));
        Some(self.value()[range].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.to_utf16(self.selection()),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|r| self.to_utf16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked.take().is_some() {
            self.remember();
            self.changed(cx);
        }
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        value: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| self.byte_range_for_utf16(r))
            .or(self.marked.clone())
            .unwrap_or_else(|| self.selection());
        self.remember();
        self.replace(range, value);
        self.marked = None;
        self.changed(cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        value: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| self.byte_range_for_utf16(r))
            .or(self.marked.clone())
            .unwrap_or_else(|| self.selection());
        if self.composition_start.is_none() {
            self.composition_start = Some(self.editor.clone());
        }
        self.replace(range.clone(), value);
        self.marked = (!value.is_empty()).then_some(range.start..range.start + value.len());
        if let Some(selection) = selection {
            let start = self.cursor_for(range.start + utf16_byte(value, selection.start));
            let end = self.cursor_for(range.start + utf16_byte(value, selection.end));
            self.editor.set_selection(Selection::Normal(start));
            self.editor.set_cursor(end);
        }
        self.changed(cx);
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.byte_range_for_utf16(range);
        let mut caret = self.caret(self.cursor_for(range.end));
        caret.origin += bounds.origin;
        Some(caret)
    }
    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let local = p - self.bounds.origin;
        let c = self
            .editor
            .with_buffer(|b| b.hit(f32::from(local.x), f32::from(local.y)))?;
        Some(self.to_utf16(self.byte(c)..self.byte(c)).start)
    }
}
impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let focus = self.focus.clone();
        let caret = self.caret(self.editor.cursor());
        let selection = self.highlights(self.selection());
        let marked = self
            .marked
            .clone()
            .map(|r| self.highlights(r))
            .unwrap_or_default();
        div()
            .size_full()
            .key_context("ShotoriTextInput")
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus(window, cx);
                    this.dragging = true;
                    let p = event.position - this.bounds.origin;
                    let (x, y) = (f32::from(p.x) as i32, f32::from(p.y) as i32);
                    this.action(match event.click_count {
                        2 => EditAction::DoubleClick { x, y },
                        3 => EditAction::TripleClick { x, y },
                        _ => EditAction::Click { x, y },
                    });
                    this.changed(cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.dragging {
                    let p = event.position - this.bounds.origin;
                    this.action(EditAction::Drag {
                        x: f32::from(p.x) as i32,
                        y: f32::from(p.y) as i32,
                    });
                    this.changed(cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        entity.update(cx, |s, _| s.bounds = bounds);
                        entity.clone()
                    },
                    move |bounds, entity, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            for mut r in selection {
                                r.origin += bounds.origin;
                                window.paint_quad(fill(r, rgba(0x4080ff44)));
                            }
                            for mut r in marked {
                                r.origin += bounds.origin;
                                r.origin.y += r.size.height - px(2.);
                                r.size.height = px(1.);
                                window.paint_quad(fill(r, rgba(theme::c().accent)));
                            }
                            if focus.is_focused(window) {
                                let mut r = caret;
                                r.origin += bounds.origin;
                                window.paint_quad(fill(r, rgba(theme::c().accent)));
                            }
                        });
                    },
                )
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::TextInput;
    use cosmic_text::Edit;
    use gpui_kit::{Bounds, EntityInputHandler, TestAppContext, point, px, size};

    #[gpui_kit::test]
    fn ime_preedit_replacement_and_utf16_ranges(cx: &mut TestAppContext) {
        let (input, cx) = cx.add_window_view(|_, cx| TextInput::new(160., 400., 24., cx));
        cx.update(|window, cx| {
            input.update(cx, |s, cx| {
                s.replace_text_in_range(None, "🙂", window, cx);
                s.replace_and_mark_text_in_range(None, "nihao", Some(5..5), window, cx);
                assert_eq!(s.value(), "🙂nihao");
                assert_eq!(s.marked_text_range(window, cx), Some(2..7));
                assert!(!s.highlights(s.marked.clone().unwrap()).is_empty());
                assert_eq!(
                    s.selected_text_range(false, window, cx).unwrap().range,
                    7..7
                );
                s.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                assert_eq!(s.value(), "🙂ni");
                s.replace_text_in_range(None, "你好", window, cx);
                assert_eq!(s.value(), "🙂你好");
                assert!(s.marked.is_none());
                assert_eq!(
                    s.selected_text_range(false, window, cx).unwrap().range,
                    4..4
                );
            })
        });
    }

    #[gpui_kit::test]
    fn text_grows_until_the_selection_right_edge(cx: &mut TestAppContext) {
        crate::annotation::text::with_test_font(|| {
            let (input, cx) = cx.add_window_view(|_, cx| TextInput::new(500., 600., 24., cx));
            cx.update(|window, cx| {
                input.update(cx, |s, cx| {
                    assert_eq!(s.content_width(), 1.);
                    s.replace_text_in_range(None, "short", window, cx);
                    let short_width = s.content_width();
                    assert!(short_width > 1. && short_width < 100.);
                    // These glyphs are covered by the bundled font. CJK
                    // fallback availability must not change layout assertions.
                    s.replace_text_in_range(None, &"W".repeat(12), window, cx);
                    assert!(s.content_width() > 320.);
                    assert_eq!(s.height(), 24. * 1.35);
                    s.replace_text_in_range(None, &"W".repeat(12), window, cx);
                    assert!(s.height() > 24. * 1.35);
                    assert!(s.content_width() <= 500.);
                })
            });
        });
    }

    #[gpui_kit::test]
    fn paragraph_changes_do_not_paint_unselected_text(cx: &mut TestAppContext) {
        let (input, cx) = cx.add_window_view(|_, cx| TextInput::new(160., 600., 24., cx));
        cx.update(|window, cx| {
            input.update(cx, |s, cx| {
                for paragraph in ["第一段文字自动换行", "second paragraph wraps", "第三段"]
                {
                    s.replace_text_in_range(None, paragraph, window, cx);
                    assert!(
                        s.highlights(s.selection()).is_empty(),
                        "a caret is not a selection"
                    );
                    s.action(cosmic_text::Action::Enter); // Shift+Enter uses this action.
                    assert!(
                        s.highlights(s.selection()).is_empty(),
                        "newline must not highlight prior paragraphs"
                    );
                }
                // Moving the caret into any paragraph must not tint the others.
                for line in 0..3 {
                    s.editor.set_cursor(cosmic_text::Cursor::new(line, 0));
                    s.editor.set_selection(cosmic_text::Selection::None);
                    assert!(s.highlights(s.selection()).is_empty());
                }
                // A real selection (and an IME underline) stays within its paragraphs.
                let start = s.byte(cosmic_text::Cursor::new(1, 0));
                let end = s.byte(cosmic_text::Cursor::new(1, 6));
                let spans = s.highlights(start..end);
                assert!(!spans.is_empty());
                let (top, bottom) = s.editor.with_buffer(|b| {
                    let runs = b
                        .layout_runs()
                        .filter(|r| r.line_i == 1)
                        .collect::<Vec<_>>();
                    (
                        runs.first().unwrap().line_top,
                        runs.last().unwrap().line_top + runs.last().unwrap().line_height,
                    )
                });
                assert!(
                    spans
                        .iter()
                        .all(|r| r.top() >= px(top) && r.bottom() <= px(bottom))
                );
            });
        });
    }

    #[gpui_kit::test]
    fn caret_stays_on_last_glyph_after_many_soft_wraps(cx: &mut TestAppContext) {
        let (input, cx) = cx.add_window_view(|_, cx| TextInput::new(160., 2000., 24., cx));
        cx.update(|window, cx| {
            input.update(cx, |s, cx| {
                for font_size in [16., 24., 32.] {
                    s.set_font_size(font_size, cx);
                    for length in [1, 20, 60, 120] {
                        s.replace_text_in_range(
                            Some(0..s.value().encode_utf16().count()),
                            &"abcdefgh".repeat(length),
                            window,
                            cx,
                        );
                        let caret = s.caret(s.editor.cursor());
                        s.editor.with_buffer(|b| {
                            let run = b.layout_runs().last().unwrap();
                            let glyph = run.glyphs.last().unwrap();
                            assert_eq!(caret.origin.y, px(run.line_top));
                            assert!((f32::from(caret.origin.x) - glyph.x - glyph.w).abs() < 0.01);
                        });
                        let ime = s
                            .bounds_for_range(
                                s.to_utf16(s.selection()),
                                Bounds::new(point(px(20.), px(30.)), size(px(160.), px(2000.))),
                                window,
                                cx,
                            )
                            .unwrap();
                        assert_eq!(ime.origin, caret.origin + point(px(20.), px(30.)));
                    }
                }
            })
        });
    }
}
