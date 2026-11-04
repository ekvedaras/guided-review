use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// Whether enter ends the draft (one line) or adds a line to it, with cmd-enter ending it (many lines).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lines {
    Single,
    Multi,
}

/// How far a caret move goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Character,
    Word,
    /// To the start or end of the line the caret is on.
    Line,
}

/// Whether a caret move drops the selection or stretches it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Caret {
    Move,
    Extend,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Back,
    Forward,
}

/// The text of a box and where its caret and selection are. Offsets are utf-8 byte offsets
/// that always sit on a grapheme boundary.
#[derive(Default, Debug, Clone)]
pub struct TextDraft {
    text: String,
    selection: Range<usize>,
    /// The caret is at the start of the selection, not the end.
    is_reversed: bool,
    /// Text an input method is still composing, such as a letter with an accent.
    marked: Option<Range<usize>>,
}

impl TextDraft {
    /// The caret goes to the end.
    pub fn with_text(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            selection: text.len()..text.len(),
            is_reversed: false,
            marked: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    pub fn caret(&self) -> usize {
        if self.is_reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    pub fn selected_text(&self) -> &str {
        self.text_in(&self.selection)
    }

    pub fn move_to(&mut self, offset: usize) {
        let offset = self.clamp(offset);
        self.selection = offset..offset;
        self.is_reversed = false;
    }

    /// Moves the caret and keeps the other end of the selection where it is.
    pub fn select_to(&mut self, offset: usize) {
        let offset = self.clamp(offset);
        if self.is_reversed {
            self.selection.start = offset;
        } else {
            self.selection.end = offset;
        }
        if self.selection.end < self.selection.start {
            self.is_reversed = !self.is_reversed;
            self.selection = self.selection.end..self.selection.start;
        }
    }

    /// The caret is at `head`, and the selection runs to `anchor`, which stays put when it extends.
    pub fn select(&mut self, anchor: usize, head: usize) {
        self.selection = self.range_in_text(&(anchor..head));
        self.is_reversed = self.clamp(head) < self.clamp(anchor);
    }

    pub fn put_caret_at(&mut self, offset: usize, caret: Caret) {
        match caret {
            Caret::Move => self.move_to(offset),
            Caret::Extend => self.select_to(offset),
        }
    }

    pub fn select_all(&mut self) {
        self.selection = 0..self.text.len();
        self.is_reversed = false;
    }

    /// A plain move with a selection only collapses it to the side it moves to.
    pub fn go(&mut self, step: Step, direction: Direction, caret: Caret) {
        if caret == Caret::Move && step == Step::Character && !self.selection.is_empty() {
            let end = match direction {
                Direction::Back => self.selection.start,
                Direction::Forward => self.selection.end,
            };
            self.move_to(end);
        } else {
            self.put_caret_at(self.offset_from_caret(step, direction), caret);
        }
    }

    /// Deletes the selection, or from the caret by `step` when nothing is selected.
    pub fn delete(&mut self, step: Step, direction: Direction) {
        if self.selection.is_empty() {
            self.select_to(self.offset_from_caret(step, direction));
        }
        self.insert("");
    }

    /// Replaces the selection, or the marked text while an input method composes.
    pub fn insert(&mut self, text: &str) {
        let range = self
            .marked
            .clone()
            .unwrap_or_else(|| self.selection.clone());
        self.replace(range, text);
    }

    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let range = self.range_in_text(&range);
        self.text.replace_range(range.clone(), text);
        self.move_to(range.start + text.len());
        self.marked = None;
    }

    /// Puts in text an input method has not finished; `selected` is inside the new text.
    pub fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) {
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let range = self.range_in_text(&range);
        self.replace(range.clone(), text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        if let Some(selected) = selected {
            let selected = selected.start.min(selected.end)..selected.start.max(selected.end);
            let start = range.start + selected.start.min(text.len());
            let end = range.start + selected.end.min(text.len());
            self.selection = start..end;
        }
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }

    pub fn to_utf16(&self, offset: usize) -> usize {
        self.text_in(&(0..offset)).encode_utf16().count()
    }

    pub fn offset_at_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        for (index, character) in self.text.char_indices() {
            if utf16 >= offset {
                return index;
            }
            utf16 += character.len_utf16();
        }
        self.text.len()
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.to_utf16(range.start)..self.to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_at_utf16(range.start)..self.offset_at_utf16(range.end)
    }

    fn offset_from_caret(&self, step: Step, direction: Direction) -> usize {
        let caret = self.caret();
        match (step, direction) {
            (Step::Character, Direction::Back) => self.previous_grapheme(caret),
            (Step::Character, Direction::Forward) => self.next_grapheme(caret),
            (Step::Word, Direction::Back) => self.previous_word_start(caret),
            (Step::Word, Direction::Forward) => self.next_word_end(caret),
            (Step::Line, Direction::Back) => {
                self.text_in(&(0..caret)).rfind('\n').map_or(0, |at| at + 1)
            }
            (Step::Line, Direction::Forward) => self
                .text_in(&(caret..self.text.len()))
                .find('\n')
                .map_or(self.text.len(), |at| caret + at),
        }
    }

    fn previous_grapheme(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_grapheme(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.text.len())
    }

    fn previous_word_start(&self, offset: usize) -> usize {
        let before = self.text_in(&(0..offset));
        let trimmed = before.trim_end_matches(|c: char| !c.is_alphanumeric());
        trimmed
            .rfind(|c: char| !c.is_alphanumeric())
            .map_or(0, |at| {
                let width = trimmed
                    .get(at..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(1, char::len_utf8);
                at + width
            })
    }

    fn next_word_end(&self, offset: usize) -> usize {
        let after = self.text_in(&(offset..self.text.len()));
        let skipped = after.len()
            - after
                .trim_start_matches(|c: char| !c.is_alphanumeric())
                .len();
        after
            .get(skipped..)
            .unwrap_or_default()
            .find(|c: char| !c.is_alphanumeric())
            .map_or(self.text.len(), |at| offset + skipped + at)
    }

    /// The text between two offsets, each moved back onto a character boundary.
    pub fn text_in(&self, range: &Range<usize>) -> &str {
        self.text.get(self.range_in_text(range)).unwrap_or_default()
    }

    /// The input method may hand over a range end first; it means the same text start first.
    fn range_in_text(&self, range: &Range<usize>) -> Range<usize> {
        let (a, b) = (self.clamp(range.start), self.clamp(range.end));
        a.min(b)..a.max(b)
    }

    fn clamp(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_at(text: &str, caret: usize) -> TextDraft {
        let mut draft = TextDraft::with_text(text);
        draft.move_to(caret);
        draft
    }

    #[test]
    #[expect(
        clippy::reversed_empty_ranges,
        reason = "the input method can hand over ranges end first, which is the case under test"
    )]
    fn a_reversed_range_from_the_input_method_reads_as_its_start_to_its_end() {
        let mut draft = draft_at("hello", 0);

        draft.select(4, 1);
        assert_eq!(draft.selected_text(), "ell");

        draft.replace(4..1, "a");
        assert_eq!(draft.text(), "hao");

        draft.replace_and_mark(Some(2..1), "ü", Some("ü".len()..0));
        assert_eq!(draft.text(), "hüo");
        assert_eq!(draft.selected_text(), "ü");
    }

    #[test]
    fn a_selection_made_from_its_end_to_its_start_has_the_caret_at_the_start() {
        let mut draft = TextDraft::with_text("abcd");
        draft.select(3, 1);
        assert_eq!(draft.selection(), 1..3);
        assert_eq!(draft.caret(), 1);
        draft.select(1, 3);
        assert_eq!(draft.caret(), 3);
    }

    #[test]
    fn typing_goes_in_at_the_caret_not_at_the_end() {
        let mut draft = draft_at("helo", 3);
        draft.insert("l");
        assert_eq!(draft.text(), "hello");
        assert_eq!(draft.caret(), 4);
    }

    #[test]
    fn backspace_and_delete_remove_one_grapheme_beside_the_caret() {
        let mut draft = draft_at("ąčę", "ą".len());
        draft.delete(Step::Character, Direction::Back);
        assert_eq!(draft.text(), "čę");
        draft.delete(Step::Character, Direction::Forward);
        assert_eq!(draft.text(), "ę");
    }

    #[test]
    fn alt_moves_by_word_and_cmd_by_line() {
        let mut draft = draft_at("one two\nthree four", 12);
        draft.go(Step::Word, Direction::Back, Caret::Move);
        assert_eq!(draft.caret(), 8);
        draft.go(Step::Word, Direction::Back, Caret::Move);
        assert_eq!(draft.caret(), 4);
        draft.go(Step::Word, Direction::Forward, Caret::Move);
        assert_eq!(draft.caret(), 7);
        draft.go(Step::Line, Direction::Forward, Caret::Move);
        assert_eq!(draft.caret(), 7);
        draft.move_to(10);
        draft.go(Step::Line, Direction::Back, Caret::Move);
        assert_eq!(draft.caret(), 8);
    }

    #[test]
    fn shift_moves_select_and_typing_replaces_the_selection() {
        let mut draft = draft_at("hello world", 5);
        draft.go(Step::Word, Direction::Forward, Caret::Extend);
        assert_eq!(draft.selected_text(), " world");
        draft.insert("!");
        assert_eq!(draft.text(), "hello!");
    }

    #[test]
    fn selecting_back_past_the_anchor_flips_the_selection() {
        let mut draft = draft_at("abcd", 2);
        draft.go(Step::Character, Direction::Forward, Caret::Extend);
        draft.go(Step::Character, Direction::Back, Caret::Extend);
        draft.go(Step::Character, Direction::Back, Caret::Extend);
        assert_eq!(draft.selection(), 1..2);
        assert_eq!(draft.caret(), 1);
    }

    #[test]
    fn a_plain_move_collapses_the_selection_to_its_side() {
        let mut draft = TextDraft::with_text("abcd");
        draft.select(1, 3);
        draft.go(Step::Character, Direction::Back, Caret::Move);
        assert_eq!(draft.selection(), 1..1);
    }

    #[test]
    fn an_input_method_replaces_its_marked_text_until_it_is_done() {
        let mut draft = TextDraft::with_text("a");
        draft.replace_and_mark(None, "˛", None);
        assert_eq!(draft.marked(), Some(1..1 + "˛".len()));
        draft.insert("ą");
        assert_eq!(draft.text(), "aą");
        assert_eq!(draft.marked(), None);
    }

    #[test]
    fn utf16_offsets_map_to_bytes_and_back() {
        let draft = TextDraft::with_text("ą😀b");
        let b = "ą😀".len();
        assert_eq!(draft.to_utf16(b), 3);
        assert_eq!(draft.offset_at_utf16(3), b);
    }
}
