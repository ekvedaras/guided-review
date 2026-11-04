use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, GlobalElementId, InspectorElementId,
    InteractiveElement, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, Pixels, Point, RenderOnce, SharedString, Style, Styled,
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill,
    point, px, relative, size,
};

use super::palette::Colors;
use super::text_input::{Direction, Lines, Step, TextDraft};

actions!(
    text_box,
    [
        Backspace,
        BackspaceWord,
        BackspaceLine,
        Delete,
        DeleteWord,
        Left,
        Right,
        WordLeft,
        WordRight,
        LineStart,
        LineEnd,
        Up,
        Down,
        TextStart,
        TextEnd,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectLineStart,
        SelectLineEnd,
        SelectUp,
        SelectDown,
        SelectAll,
        Newline,
        Submit,
        Cancel,
        Copy,
        Cut,
        Paste,
    ]
);

const KEY_CONTEXT: &str = "TextBox";
const MULTI_LINE: &str = "MultiLine";

pub fn bind_keys(cx: &mut App) {
    let single = Some(KEY_CONTEXT);
    let multi_line = format!("{KEY_CONTEXT} && {MULTI_LINE}");
    let one_line = format!("{KEY_CONTEXT} && !{MULTI_LINE}");
    let multi = Some(multi_line.as_str());
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, single),
        KeyBinding::new("shift-backspace", Backspace, single),
        KeyBinding::new("alt-backspace", BackspaceWord, single),
        KeyBinding::new("cmd-backspace", BackspaceLine, single),
        KeyBinding::new("delete", Delete, single),
        KeyBinding::new("alt-delete", DeleteWord, single),
        KeyBinding::new("left", Left, single),
        KeyBinding::new("right", Right, single),
        KeyBinding::new("alt-left", WordLeft, single),
        KeyBinding::new("alt-right", WordRight, single),
        KeyBinding::new("cmd-left", LineStart, single),
        KeyBinding::new("cmd-right", LineEnd, single),
        KeyBinding::new("home", LineStart, single),
        KeyBinding::new("end", LineEnd, single),
        KeyBinding::new("cmd-up", TextStart, single),
        KeyBinding::new("cmd-down", TextEnd, single),
        KeyBinding::new("shift-left", SelectLeft, single),
        KeyBinding::new("shift-right", SelectRight, single),
        KeyBinding::new("alt-shift-left", SelectWordLeft, single),
        KeyBinding::new("alt-shift-right", SelectWordRight, single),
        KeyBinding::new("cmd-shift-left", SelectLineStart, single),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, single),
        KeyBinding::new("cmd-a", SelectAll, single),
        KeyBinding::new("cmd-c", Copy, single),
        KeyBinding::new("cmd-x", Cut, single),
        KeyBinding::new("cmd-v", Paste, single),
        KeyBinding::new("escape", Cancel, single),
        KeyBinding::new("cmd-enter", Submit, single),
        KeyBinding::new("enter", Submit, Some(one_line.as_str())),
        KeyBinding::new("enter", Newline, multi),
        KeyBinding::new("shift-enter", Newline, multi),
        KeyBinding::new("up", Up, multi),
        KeyBinding::new("down", Down, multi),
        KeyBinding::new("shift-up", SelectUp, multi),
        KeyBinding::new("shift-down", SelectDown, multi),
    ]);
}

/// A key context that is on while a text box has focus, for bindings that must not fire while typing.
pub const TYPING_CONTEXT: &str = KEY_CONTEXT;

pub type OnText = Rc<dyn Fn(String, &mut Window, &mut App)>;
pub type OnKey = Rc<dyn Fn(&mut Window, &mut App)>;

/// An editable box: click to place the caret, keys move it, shift selects, copy and paste,
/// and input methods for accented letters. The owner keeps the text; the box keeps the caret.
#[derive(IntoElement)]
pub struct TextBox {
    id: ElementId,
    focus: FocusHandle,
    text: String,
    lines: Lines,
    on_edit: Option<OnText>,
    on_submit: Option<OnKey>,
    on_cancel: Option<OnKey>,
}

impl TextBox {
    /// `focus` is the owner's, so the owner decides when typing goes here.
    pub fn new(id: impl Into<ElementId>, focus: &FocusHandle, text: &str, lines: Lines) -> Self {
        Self {
            id: id.into(),
            focus: focus.clone(),
            text: text.to_owned(),
            lines,
            on_edit: None,
            on_submit: None,
            on_cancel: None,
        }
    }

    pub fn on_edit(mut self, on_edit: impl Fn(String, &mut Window, &mut App) + 'static) -> Self {
        self.on_edit = Some(Rc::new(on_edit));
        self
    }

    pub fn on_submit(mut self, on_submit: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Rc::new(on_submit));
        self
    }

    pub fn on_cancel(mut self, on_cancel: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(on_cancel));
        self
    }
}

impl RenderOnce for TextBox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = self.focus.clone();
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| {
            EditState::new(&self.text, focus.clone())
        });
        state.update(cx, |state, _| {
            state.follow(&self.text, &self.focus);
            state.lines = self.lines;
            state.on_edit = self.on_edit;
            state.on_submit = self.on_submit;
            state.on_cancel = self.on_cancel;
        });
        let key_context = match self.lines {
            Lines::Single => KEY_CONTEXT.to_owned(),
            Lines::Multi => format!("{KEY_CONTEXT} {MULTI_LINE}"),
        };
        div()
            .id(self.id)
            .key_context(key_context.as_str())
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .w_full()
            .on_action(window.listener_for(&state, EditState::backspace))
            .on_action(window.listener_for(&state, EditState::backspace_word))
            .on_action(window.listener_for(&state, EditState::backspace_line))
            .on_action(window.listener_for(&state, EditState::delete))
            .on_action(window.listener_for(&state, EditState::delete_word))
            .on_action(window.listener_for(&state, EditState::left))
            .on_action(window.listener_for(&state, EditState::right))
            .on_action(window.listener_for(&state, EditState::word_left))
            .on_action(window.listener_for(&state, EditState::word_right))
            .on_action(window.listener_for(&state, EditState::line_start))
            .on_action(window.listener_for(&state, EditState::line_end))
            .on_action(window.listener_for(&state, EditState::up))
            .on_action(window.listener_for(&state, EditState::down))
            .on_action(window.listener_for(&state, EditState::text_start))
            .on_action(window.listener_for(&state, EditState::text_end))
            .on_action(window.listener_for(&state, EditState::select_left))
            .on_action(window.listener_for(&state, EditState::select_right))
            .on_action(window.listener_for(&state, EditState::select_word_left))
            .on_action(window.listener_for(&state, EditState::select_word_right))
            .on_action(window.listener_for(&state, EditState::select_line_start))
            .on_action(window.listener_for(&state, EditState::select_line_end))
            .on_action(window.listener_for(&state, EditState::select_up))
            .on_action(window.listener_for(&state, EditState::select_down))
            .on_action(window.listener_for(&state, EditState::select_all))
            .on_action(window.listener_for(&state, EditState::newline))
            .on_action(window.listener_for(&state, EditState::submit))
            .on_action(window.listener_for(&state, EditState::cancel))
            .on_action(window.listener_for(&state, EditState::copy))
            .on_action(window.listener_for(&state, EditState::cut))
            .on_action(window.listener_for(&state, EditState::paste))
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&state, EditState::mouse_down),
            )
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&state, |state, _, _, _| state.is_dragging = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                window.listener_for(&state, |state, _, _, _| state.is_dragging = false),
            )
            .on_mouse_move(window.listener_for(&state, EditState::mouse_move))
            .child(TextBoxElement { state })
    }
}

/// The caret and the layout of the last paint, kept across frames under the box's id.
struct EditState {
    draft: TextDraft,
    focus: FocusHandle,
    lines: Lines,
    layout: Option<Layout>,
    is_dragging: bool,
    /// Up and down keep to this x, so going through a short line does not lose the column.
    goal_x: Option<Pixels>,
    on_edit: Option<OnText>,
    on_submit: Option<OnKey>,
    on_cancel: Option<OnKey>,
}

/// The wrapped lines as painted, one per line of the text, with where each starts.
struct Layout {
    lines: Vec<(usize, WrappedLine)>,
    origin: Point<Pixels>,
    line_height: Pixels,
}

impl Layout {
    fn position_of(&self, offset: usize) -> Point<Pixels> {
        let mut top = Pixels::ZERO;
        for (start, line) in &self.lines {
            if offset <= start + line.len() {
                let at = line
                    .position_for_index(offset - start, self.line_height)
                    .unwrap_or_default();
                return point(at.x, top + at.y);
            }
            top += line.size(self.line_height).height;
        }
        point(Pixels::ZERO, top)
    }

    /// `position` is inside the box: the top left of the text is 0, 0.
    fn offset_at(&self, position: Point<Pixels>) -> usize {
        let mut top = Pixels::ZERO;
        for (index, (start, line)) in self.lines.iter().enumerate() {
            let height = line.size(self.line_height).height;
            let is_last = index + 1 == self.lines.len();
            if position.y < top + height || is_last {
                let inside = point(
                    position.x.max(Pixels::ZERO),
                    (position.y - top).max(Pixels::ZERO),
                );
                let index = match line.closest_index_for_position(inside, self.line_height) {
                    Ok(index) | Err(index) => index,
                };
                return start + index;
            }
            top += height;
        }
        0
    }
}

impl EditState {
    fn new(text: &str, focus: FocusHandle) -> Self {
        Self {
            draft: TextDraft::with_text(text),
            focus,
            lines: Lines::Single,
            layout: None,
            is_dragging: false,
            goal_x: None,
            on_edit: None,
            on_submit: None,
            on_cancel: None,
        }
    }

    /// The owner changed the text (cleared it, added a suggestion block): the caret goes to the end.
    fn follow(&mut self, text: &str, focus: &FocusHandle) {
        if self.draft.text() != text {
            self.draft = TextDraft::with_text(text);
        }
        self.focus = focus.clone();
    }

    fn edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        window.notify_text_input_changed(&self.focus);
        if let Some(on_edit) = self.on_edit.clone() {
            let text = self.draft.text().to_owned();
            cx.defer_in(window, move |_, window, cx| on_edit(text, window, cx));
        }
        cx.notify();
    }

    fn moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.caret_moved(window, cx);
    }

    fn caret_moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn change(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut TextDraft),
    ) {
        change(&mut self.draft);
        self.edited(window, cx);
    }

    fn walk(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        walk: impl FnOnce(&mut TextDraft),
    ) {
        walk(&mut self.draft);
        self.moved(window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.change(window, cx, |draft| {
            draft.delete(Step::Character, Direction::Back);
        });
    }

    fn backspace_word(&mut self, _: &BackspaceWord, window: &mut Window, cx: &mut Context<Self>) {
        self.change(window, cx, |draft| {
            draft.delete(Step::Word, Direction::Back);
        });
    }

    fn backspace_line(&mut self, _: &BackspaceLine, window: &mut Window, cx: &mut Context<Self>) {
        self.change(window, cx, |draft| {
            draft.delete(Step::Line, Direction::Back);
        });
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.change(window, cx, |draft| {
            draft.delete(Step::Character, Direction::Forward);
        });
    }

    fn delete_word(&mut self, _: &DeleteWord, window: &mut Window, cx: &mut Context<Self>) {
        self.change(window, cx, |draft| {
            draft.delete(Step::Word, Direction::Forward);
        });
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| {
            draft.go(Step::Character, Direction::Back);
        });
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| {
            draft.go(Step::Character, Direction::Forward);
        });
    }

    fn word_left(&mut self, _: &WordLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| draft.go(Step::Word, Direction::Back));
    }

    fn word_right(&mut self, _: &WordRight, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| draft.go(Step::Word, Direction::Forward));
    }

    fn line_start(&mut self, _: &LineStart, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| draft.go(Step::Line, Direction::Back));
    }

    fn line_end(&mut self, _: &LineEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| draft.go(Step::Line, Direction::Forward));
    }

    fn text_start(&mut self, _: &TextStart, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| draft.move_to(0));
    }

    fn text_end(&mut self, _: &TextEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| draft.move_to(usize::MAX));
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| {
            draft.extend(Step::Character, Direction::Back);
        });
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| {
            draft.extend(Step::Character, Direction::Forward);
        });
    }

    fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.walk(window, cx, |draft| {
            draft.extend(Step::Word, Direction::Back);
        });
    }

    fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.walk(window, cx, |draft| {
            draft.extend(Step::Word, Direction::Forward);
        });
    }

    fn select_line_start(
        &mut self,
        _: &SelectLineStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.walk(window, cx, |draft| {
            draft.extend(Step::Line, Direction::Back);
        });
    }

    fn select_line_end(&mut self, _: &SelectLineEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, |draft| {
            draft.extend(Step::Line, Direction::Forward);
        });
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.walk(window, cx, TextDraft::select_all);
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.go_vertically(Direction::Back, false, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.go_vertically(Direction::Forward, false, window, cx);
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.go_vertically(Direction::Back, true, window, cx);
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.go_vertically(Direction::Forward, true, window, cx);
    }

    /// One painted row up or down, wrapped rows included. Past the first or last row it goes to
    /// the start or end of the text.
    fn go_vertically(
        &mut self,
        direction: Direction,
        is_selecting: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = &self.layout else {
            return;
        };
        let at = layout.position_of(self.draft.caret());
        let goal_x = *self.goal_x.get_or_insert(at.x);
        let row_y = match direction {
            Direction::Back => at.y - layout.line_height,
            Direction::Forward => at.y + layout.line_height,
        };
        let total_height: Pixels = layout
            .lines
            .iter()
            .map(|(_, line)| line.size(layout.line_height).height)
            .sum();
        let target = if row_y < Pixels::ZERO {
            0
        } else if row_y >= total_height {
            self.draft.text().len()
        } else {
            layout.offset_at(point(goal_x, row_y + layout.line_height / 2.))
        };
        if is_selecting {
            self.draft.select_to(target);
        } else {
            self.draft.move_to(target);
        }
        self.caret_moved(window, cx);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        self.change(window, cx, |draft| draft.insert("\n"));
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(on_submit) = self.on_submit.clone() {
            cx.defer_in(window, move |_, window, cx| on_submit(window, cx));
        }
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        match self.on_cancel.clone() {
            Some(on_cancel) => cx.defer_in(window, move |_, window, cx| on_cancel(window, cx)),
            None => cx.propagate(),
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.draft.selection().is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.draft.selected_text().to_owned(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.draft.selection().is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.draft.selected_text().to_owned(),
            ));
            self.change(window, cx, |draft| draft.insert(""));
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let text = match self.lines {
            Lines::Single => text.replace('\n', " "),
            Lines::Multi => text,
        };
        self.change(window, cx, |draft| draft.insert(&text));
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(offset) = self.offset_at_mouse(event.position) else {
            return;
        };
        match event.click_count {
            2 => {
                self.draft.move_to(offset);
                self.draft.go(Step::Word, Direction::Back);
                self.draft.extend(Step::Word, Direction::Forward);
            }
            3 => {
                self.draft.move_to(offset);
                self.draft.go(Step::Line, Direction::Back);
                self.draft.extend(Step::Line, Direction::Forward);
            }
            _ if event.modifiers.shift => self.draft.select_to(offset),
            _ => {
                self.draft.move_to(offset);
                self.is_dragging = true;
            }
        }
        self.moved(window, cx);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_dragging || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some(offset) = self.offset_at_mouse(event.position) {
            self.draft.select_to(offset);
            self.moved(window, cx);
        }
    }

    fn offset_at_mouse(&self, position: Point<Pixels>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        Some(layout.offset_at(position - layout.origin))
    }

    fn bounds_of(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let start = layout.position_of(range.start);
        let end = layout.position_of(range.end);
        Some(Bounds::from_corners(
            layout.origin + start,
            layout.origin + point(end.x.max(start.x + px(1.)), end.y + layout.line_height),
        ))
    }
}

impl EntityInputHandler for EditState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.draft.range_from_utf16(&range_utf16);
        actual_range.replace(self.draft.range_to_utf16(&range));
        Some(self.draft.text_in(&range).to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.draft.selection();
        Some(UTF16Selection {
            range: self.draft.range_to_utf16(&selection),
            reversed: self.draft.caret() == selection.start && !selection.is_empty(),
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.draft.range_from_utf16(&range_utf16);
        self.draft.select(range, false);
        self.moved(window, cx);
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.draft.to_utf16(self.draft.text().len()))
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.draft
            .marked()
            .map(|range| self.draft.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.unmark();
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|range| self.draft.range_from_utf16(&range));
        let text = match self.lines {
            Lines::Single => new_text.replace('\n', " "),
            Lines::Multi => new_text.to_owned(),
        };
        self.change(window, cx, |draft| match range {
            Some(range) => draft.replace(range, &text),
            None => draft.insert(&text),
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|range| self.draft.range_from_utf16(&range));
        let selected = new_selected_range_utf16.map(|selected| {
            let inserted = TextDraft::with_text(new_text);
            inserted.range_from_utf16(&selected)
        });
        self.change(window, cx, |draft| {
            draft.replace_and_mark(range, new_text, selected);
        });
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.bounds_of(self.draft.range_from_utf16(&range_utf16))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.offset_at_mouse(point)?;
        Some(self.draft.to_utf16(offset))
    }
}

struct TextBoxElement {
    state: Entity<EditState>,
}

impl IntoElement for TextBoxElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct Painted {
    lines: Vec<(usize, WrappedLine)>,
    selection: Vec<Bounds<Pixels>>,
    caret: Option<Bounds<Pixels>>,
    line_height: Pixels,
}

impl Element for TextBoxElement {
    type RequestLayoutState = ();
    type PrepaintState = Painted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let text = SharedString::from(self.state.read(cx).draft.text().to_owned());
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let runs = [text_style.to_run(text.len())];
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let wrap_width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let height = window
                .text_system()
                .shape_text(text.clone(), font_size, &runs, wrap_width, None)
                .map_or(line_height, |lines| {
                    lines
                        .iter()
                        .map(|line| line.size(line_height).height)
                        .sum::<Pixels>()
                });
            size(wrap_width.unwrap_or(Pixels::ZERO), height.max(line_height))
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = self.state.read(cx);
        let draft = &state.draft;
        let text = draft.text().to_owned();
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let run = text_style.to_run(text.len());
        let runs = match draft.marked() {
            Some(marked) => vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.len(),
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: text.len() - marked.end,
                    ..run
                },
            ],
            None => vec![run],
        };
        let shaped = window
            .text_system()
            .shape_text(text.into(), font_size, &runs, Some(bounds.size.width), None)
            .unwrap_or_default();
        let mut start = 0;
        let lines: Vec<(usize, WrappedLine)> = shaped
            .into_iter()
            .map(|line| {
                let line_start = start;
                start += line.len() + 1;
                (line_start, line)
            })
            .collect();
        let layout = Layout {
            lines,
            origin: bounds.origin,
            line_height,
        };
        let selection = draft.selection();
        let selection_bounds = if selection.is_empty() {
            Vec::new()
        } else {
            selection_rows(&layout, selection, bounds.size.width)
        };
        let caret = (state.focus.is_focused(window) && draft.selection().is_empty()).then(|| {
            let at = layout.position_of(draft.caret());
            Bounds::new(bounds.origin + at, size(px(1.5), line_height))
        });
        Painted {
            lines: layout.lines,
            selection: selection_bounds,
            caret,
            line_height,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        painted: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let colors = Colors::active(cx);
        let focus = self.state.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.state.clone()),
            cx,
        );
        for row in &painted.selection {
            window.paint_quad(fill(*row, colors.current));
        }
        let mut top = bounds.top();
        for (_, line) in &painted.lines {
            line.paint(
                point(bounds.left(), top),
                painted.line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
            top += line.size(painted.line_height).height;
        }
        if let Some(caret) = painted.caret {
            window.paint_quad(fill(caret, colors.text));
        }
        let lines = std::mem::take(&mut painted.lines);
        let line_height = painted.line_height;
        self.state.update(cx, |state, _| {
            state.layout = Some(Layout {
                lines,
                origin: bounds.origin,
                line_height,
            });
        });
    }
}

/// One rectangle per painted row the selection covers.
fn selection_rows(layout: &Layout, selection: Range<usize>, width: Pixels) -> Vec<Bounds<Pixels>> {
    let start = layout.position_of(selection.start);
    let end = layout.position_of(selection.end);
    let height = layout.line_height;
    let row = |left: Pixels, top: Pixels, right: Pixels| {
        Bounds::from_corners(
            layout.origin + point(left, top),
            layout.origin + point(right.max(left + px(4.)), top + height),
        )
    };
    if start.y == end.y {
        return vec![row(start.x, start.y, end.x)];
    }
    let mut rows = vec![row(start.x, start.y, width)];
    let mut top = start.y + height;
    while top < end.y {
        rows.push(row(Pixels::ZERO, top, width));
        top += height;
    }
    rows.push(row(Pixels::ZERO, end.y, end.x));
    rows
}
