//! Text a person does not see where GitHub or the app shows it, but an AI reading the raw text does:
//! the usual hiding places of a prompt injection.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

/// One hidden part of a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HiddenPart {
    /// What an `<!-- -->` holds.
    HtmlComment(String),
    /// A link reference definition no link uses, as written, such as `[//]: # (text)`.
    UnusedLinkDefinition(String),
    /// Characters that take no room on screen. `spelled` is what Unicode tag characters among them spell.
    InvisibleCharacters { count: usize, spelled: String },
}

/// A Markdown text split into what a person sees and what is hidden from them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenText {
    parts: Vec<HiddenPart>,
    visible: String,
}

/// Fewer zero-width characters than this cannot spell one letter in binary, and a few come with text
/// pasted from chat apps and documents.
const ZERO_WIDTH_WORTH_A_WARNING: usize = 8;

const FLAG_BASE: char = '\u{1F3F4}';
const CANCEL_TAG: char = '\u{E007F}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Invisible {
    /// Hides or turns text around however few there are: tag characters, variation selectors that
    /// can carry bytes, and the controls that change the direction of text.
    Always,
    /// Zero-width spaces, joiners and marks: hiding only in numbers.
    InNumbers,
}

fn invisible(c: char) -> Option<Invisible> {
    match c {
        '\u{E0000}'..='\u{E007F}'
        | '\u{E0100}'..='\u{E01EF}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2066}'..='\u{2069}' => Some(Invisible::Always),
        '\u{200B}'
        | '\u{200C}'
        | '\u{200E}'
        | '\u{200F}'
        | '\u{2060}'..='\u{2064}'
        | '\u{FEFF}'
        | '\u{180E}' => Some(Invisible::InNumbers),
        _ => None,
    }
}

impl HiddenText {
    pub fn of(markdown: &str) -> Self {
        let can_hide = !markdown.is_ascii() || markdown.contains("<!--") || markdown.contains("]:");
        if !can_hide {
            return Self {
                parts: Vec::new(),
                visible: markdown.to_owned(),
            };
        }
        let mut cut: Vec<Range<usize>> = Vec::new();
        let mut parts = Vec::new();
        let (comments, unused_definitions) = markdown_hiding(markdown);
        for range in comments {
            let comment = markdown.get(range.clone()).unwrap_or_default();
            let inside = comment
                .strip_prefix("<!--")
                .map_or_default(|rest| rest.strip_suffix("-->").unwrap_or(rest));
            parts.push(HiddenPart::HtmlComment(inside.to_owned()));
            cut.push(range);
        }
        for range in unused_definitions {
            let definition = markdown.get(range.clone()).unwrap_or_default();
            parts.push(HiddenPart::UnusedLinkDefinition(definition.to_owned()));
            cut.push(range);
        }
        let characters = invisible_characters(markdown);
        if let Some(part) = characters.part() {
            parts.push(part);
        }
        cut.extend(characters.ranges);
        Self {
            parts,
            visible: without(markdown, cut),
        }
    }

    pub fn parts(&self) -> &[HiddenPart] {
        &self.parts
    }

    /// The text without its hidden parts: what the reviewer read.
    pub fn visible(&self) -> &str {
        &self.visible
    }

    /// One line that names what is hidden, `None` when nothing is.
    pub fn summary(&self) -> Option<String> {
        let comments = self
            .parts
            .iter()
            .filter(|part| matches!(part, HiddenPart::HtmlComment(_)))
            .count();
        let definitions = self
            .parts
            .iter()
            .filter(|part| matches!(part, HiddenPart::UnusedLinkDefinition(_)))
            .count();
        let characters: usize = self
            .parts
            .iter()
            .map(|part| match part {
                HiddenPart::InvisibleCharacters { count, .. } => *count,
                HiddenPart::HtmlComment(_) | HiddenPart::UnusedLinkDefinition(_) => 0,
            })
            .sum();
        let named: Vec<String> = [
            (comments, "HTML comment"),
            (definitions, "unused link definition"),
            (characters, "invisible character"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, what)| {
            let plural = if count == 1 { "" } else { "s" };
            format!("{count} {what}{plural}")
        })
        .collect();
        (!named.is_empty()).then(|| format!("Hidden text: {}", named.join(", ")))
    }
}

/// The HTML comments outside code, and the link definitions no link or picture uses, as byte ranges.
fn markdown_hiding(markdown: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let parser = Parser::new_ext(markdown, super::markdown::OPTIONS);
    #[expect(
        clippy::needless_collect,
        reason = "the definitions borrow the parser, which the loop below consumes"
    )]
    let definitions: Vec<(String, Range<usize>)> = parser
        .reference_definitions()
        .iter()
        .map(|(label, definition)| (label_key(label), definition.span.clone()))
        .collect();
    let mut html = Vec::new();
    let mut used = Vec::new();
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::HtmlBlock) | Event::InlineHtml(_) => html.push(range),
            Event::Start(Tag::Link { id, .. } | Tag::Image { id, .. }) if !id.is_empty() => {
                used.push(label_key(&id));
            }
            _ => {}
        }
    }
    let comments = html
        .into_iter()
        .flat_map(|range| comments_in(markdown, range))
        .collect();
    let unused = definitions
        .into_iter()
        .filter(|(label, _)| !used.contains(label))
        .map(|(_, span)| trimmed(markdown, span))
        .collect();
    (comments, unused)
}

fn label_key(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The span without the line break and blanks after the definition, which stay in the visible text.
fn trimmed(markdown: &str, span: Range<usize>) -> Range<usize> {
    let text = markdown.get(span.clone()).unwrap_or_default();
    span.start..span.start + text.trim_end().len()
}

fn comments_in(markdown: &str, html: Range<usize>) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut at = html.start;
    while let Some(start) = markdown
        .get(at..html.end)
        .and_then(|rest| rest.find("<!--"))
        .map(|offset| at + offset)
    {
        let after_open = start + "<!--".len();
        let end = markdown
            .get(after_open..html.end)
            .and_then(|rest| rest.find("-->"))
            .map_or(html.end, |offset| after_open + offset + "-->".len());
        found.push(start..end);
        at = end;
    }
    found
}

#[derive(Default)]
struct InvisibleCharacters {
    ranges: Vec<Range<usize>>,
    always: usize,
    in_numbers: usize,
    spelled: String,
}

impl InvisibleCharacters {
    fn part(&self) -> Option<HiddenPart> {
        let count = if self.in_numbers >= ZERO_WIDTH_WORTH_A_WARNING || self.always > 0 {
            self.always + self.in_numbers
        } else {
            0
        };
        (count > 0).then(|| HiddenPart::InvisibleCharacters {
            count,
            spelled: self.spelled.clone(),
        })
    }
}

/// Every invisible character, except the tag characters of a flag emoji such as Scotland's, which
/// follow a black flag and end with the cancel tag.
fn invisible_characters(text: &str) -> InvisibleCharacters {
    let mut found = InvisibleCharacters::default();
    let mut in_flag = false;
    for (at, c) in text.char_indices() {
        let is_tag = ('\u{E0000}'..=CANCEL_TAG).contains(&c);
        if in_flag && is_tag {
            in_flag = c != CANCEL_TAG;
            continue;
        }
        in_flag = c == FLAG_BASE;
        let Some(kind) = invisible(c) else {
            continue;
        };
        found.ranges.push(at..at + c.len_utf8());
        match kind {
            Invisible::Always => found.always += 1,
            Invisible::InNumbers => found.in_numbers += 1,
        }
        if let Some(letter) = (is_tag && c != CANCEL_TAG)
            .then(|| char::from_u32(u32::from(c) - 0xE0000))
            .flatten()
            .filter(|letter| letter.is_ascii_graphic() || *letter == ' ')
        {
            found.spelled.push(letter);
        }
    }
    found
}

fn without(text: &str, mut cut: Vec<Range<usize>>) -> String {
    cut.sort_by_key(|range| range.start);
    let mut kept = String::with_capacity(text.len());
    let mut at = 0;
    for range in cut {
        if range.start > at {
            kept.push_str(text.get(at..range.start).unwrap_or_default());
        }
        at = at.max(range.end);
    }
    kept.push_str(text.get(at..).unwrap_or_default());
    kept
}
