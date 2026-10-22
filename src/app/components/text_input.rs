use gpui::Keystroke;

/// Whether enter ends the draft (one line) or adds a line to it, with cmd-enter ending it (many lines).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lines {
    Single,
    Multi,
}

/// What a key did to a draft, for the owner to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftKey {
    Submit,
    Cancel,
    Edited,
    Ignored,
}

/// The text of a one-line box that has focus. The owner decides what enter and escape mean.
#[derive(Default)]
pub struct TextDraft {
    text: String,
}

impl TextDraft {
    pub fn with_text(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }

    fn newline(&mut self) {
        self.text.push('\n');
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// The text with a caret at the end, as the box shows it.
    pub fn shown(&self) -> String {
        format!("{}\u{258f}", self.text)
    }

    pub fn on_key(&mut self, keystroke: &Keystroke, lines: Lines) -> DraftKey {
        match (keystroke.key.as_str(), lines) {
            ("escape", _) => DraftKey::Cancel,
            ("enter", Lines::Single) => DraftKey::Submit,
            ("enter", Lines::Multi) if keystroke.modifiers.platform => DraftKey::Submit,
            ("enter", Lines::Multi) => {
                self.newline();
                DraftKey::Edited
            }
            _ if self.edit(keystroke) => DraftKey::Edited,
            _ => DraftKey::Ignored,
        }
    }

    /// Typing and backspace. True when the text changed.
    fn edit(&mut self, keystroke: &Keystroke) -> bool {
        match keystroke.key.as_str() {
            "backspace" => self.text.pop().is_some(),
            _ if !keystroke.modifiers.control && !keystroke.modifiers.platform => {
                match &keystroke.key_char {
                    Some(typed) => {
                        self.text.push_str(typed);
                        true
                    }
                    None => false,
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(keys: &str) -> Keystroke {
        let mut keystroke = Keystroke::parse(keys).unwrap();
        if keystroke.key.chars().count() == 1 {
            keystroke.key_char = Some(keystroke.key.clone());
        }
        keystroke
    }

    #[test]
    fn enter_submits_a_single_line_and_escape_cancels() {
        let mut draft = TextDraft::with_text("hi");
        assert_eq!(draft.on_key(&key("enter"), Lines::Single), DraftKey::Submit);
        assert_eq!(
            draft.on_key(&key("escape"), Lines::Single),
            DraftKey::Cancel
        );
        assert_eq!(draft.text(), "hi");
    }

    #[test]
    fn enter_adds_a_line_and_cmd_enter_submits_many_lines() {
        let mut draft = TextDraft::with_text("hi");
        assert_eq!(draft.on_key(&key("enter"), Lines::Multi), DraftKey::Edited);
        assert_eq!(draft.text(), "hi\n");
        assert_eq!(
            draft.on_key(&key("cmd-enter"), Lines::Multi),
            DraftKey::Submit
        );
    }

    #[test]
    fn typing_and_backspace_edit_and_other_keys_are_ignored() {
        let mut draft = TextDraft::default();
        assert_eq!(draft.on_key(&key("a"), Lines::Single), DraftKey::Edited);
        assert_eq!(
            draft.on_key(&key("cmd-a"), Lines::Single),
            DraftKey::Ignored
        );
        assert_eq!(
            draft.on_key(&key("backspace"), Lines::Single),
            DraftKey::Edited
        );
        assert_eq!(
            draft.on_key(&key("backspace"), Lines::Single),
            DraftKey::Ignored
        );
    }
}
