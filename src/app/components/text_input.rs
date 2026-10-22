use gpui::Keystroke;

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

    pub fn newline(&mut self) {
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

    /// Typing and backspace. True when the text changed.
    pub fn edit(&mut self, keystroke: &Keystroke) -> bool {
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
