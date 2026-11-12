use std::ops::Range;
use std::path::{Path, PathBuf};

use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd, TextMergeStream,
};

use super::highlight::{Span, highlight_snippet};
use crate::image_facts;

pub(super) const OPTIONS: Options = Options::ENABLE_STRIKETHROUGH
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_TABLES);

/// A file of the tour's `folder` that `url` names by a plain relative path.
fn file_in(folder: &Path, url: &str) -> Option<PathBuf> {
    let is_plain = !url.is_empty()
        && !url.contains("://")
        && !url.starts_with(['/', '#'])
        && !url.split('/').any(|part| part == "..");
    let file = folder.join(url);
    (is_plain && file.is_file()).then_some(file)
}

/// A picture or link of a markdown text that names a file of the tour's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalFile {
    /// Where the whole `![…](…)` or `[…](…)` stands in the text.
    pub range: Range<usize>,
    pub path: PathBuf,
    /// The markdown between the brackets: a picture's alt text, a link's text.
    pub text: String,
    pub is_picture: bool,
}

/// The pictures and links of `source` that name files of `folder`, in reading order.
pub(super) fn local_files(source: &str, folder: &Path) -> Vec<LocalFile> {
    let mut found = Vec::new();
    let mut open: Option<(LocalFile, Option<Range<usize>>)> = None;
    let mut depth = 0_usize;
    for (event, range) in Parser::new_ext(source, OPTIONS).into_offset_iter() {
        match (&event, open.as_mut()) {
            (Event::Start(Tag::Image { .. } | Tag::Link { .. }), Some(_)) => depth += 1,
            (Event::Start(Tag::Image { dest_url, .. } | Tag::Link { dest_url, .. }), None) => {
                open = file_in(folder, dest_url).map(|path| {
                    let is_picture = matches!(event, Event::Start(Tag::Image { .. }));
                    let file = LocalFile {
                        range: range.clone(),
                        path,
                        text: String::new(),
                        is_picture,
                    };
                    (file, None)
                });
            }
            (Event::End(TagEnd::Image | TagEnd::Link), Some(_)) if depth > 0 => depth -= 1,
            (Event::End(TagEnd::Image | TagEnd::Link), Some(_)) => {
                if let Some((mut file, inner)) = open.take() {
                    inner
                        .and_then(|inner| source.get(inner))
                        .unwrap_or_default()
                        .clone_into(&mut file.text);
                    found.push(file);
                }
            }
            (_, Some((_, inner))) => {
                *inner = Some(match inner.take() {
                    Some(inner) => inner.start..range.end,
                    None => range,
                });
            }
            (_, None) => {}
        }
    }
    found
}

/// The part of GitHub-flavoured markdown that review comments use, as a tree the views can draw.
/// Everything else (raw HTML other than `<details>`, `<img>`, `<br>`, `<a>`) shows as plain text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Markdown {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Block {
    Paragraph(Vec<Run>),
    Heading {
        level: u8,
        text: Vec<Run>,
    },
    List {
        first_number: Option<u64>,
        items: Vec<Vec<Self>>,
    },
    Code {
        language: Option<String>,
        text: String,
        /// Per line of `text`, coloured when parsed, so drawing the block highlights nothing.
        spans: Vec<Vec<Span>>,
    },
    Quote(Vec<Self>),
    /// A `<details>` fold: shown closed, the reader opens it.
    Details {
        summary: Vec<Run>,
        body: Vec<Self>,
    },
    Rule,
    Table(Table),
    /// A picture from the tour's own folder, of its size in pixels, so its room is known before it loads.
    Picture {
        path: PathBuf,
        alt: String,
        width: u32,
        height: u32,
    },
}

/// A GitHub table: a header row and body rows of cells, each cell inline text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Table {
    /// One per column, from the header's separator line (`:--`, `:-:`, `--:`).
    pub alignments: Vec<ColumnAlignment>,
    pub header: Vec<Vec<Run>>,
    pub rows: Vec<Vec<Vec<Run>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColumnAlignment {
    #[default]
    Left,
    Center,
    Right,
}

impl From<Alignment> for ColumnAlignment {
    fn from(alignment: Alignment) -> Self {
        match alignment {
            Alignment::None | Alignment::Left => Self::Left,
            Alignment::Center => Self::Center,
            Alignment::Right => Self::Right,
        }
    }
}

/// A stretch of text with one style.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
    /// Images become links to the image, so the app never loads them.
    pub link: Option<String>,
}

impl Markdown {
    pub fn parse(source: &str) -> Self {
        Self::parse_in(source, None)
    }

    /// Like `parse`, where pictures and links may name files of the tour's own `folder`: such a
    /// picture shows, and such a link opens the file. Anything else is left as `parse` leaves it.
    pub fn parse_with_files(source: &str, folder: &Path) -> Self {
        Self::parse_in(source, Some(folder))
    }

    fn parse_in(source: &str, files: Option<&Path>) -> Self {
        let mut builder = Builder {
            files: files.map(Path::to_path_buf),
            ..Builder::default()
        };
        for event in TextMergeStream::new(Parser::new_ext(source, OPTIONS)) {
            builder.event(event);
        }
        builder.finish()
    }

    /// The Mermaid text of every `mermaid` fence, in reading order, folds and lists included.
    pub fn diagrams(&self) -> Vec<&str> {
        let mut diagrams = Vec::new();
        for block in &self.blocks {
            block.collect_diagrams(&mut diagrams);
        }
        diagrams
    }

    /// The pictures from the tour's folder, in reading order, with their alt text.
    pub fn pictures(&self) -> Vec<(&Path, &str)> {
        let mut pictures = Vec::new();
        for block in &self.blocks {
            block.collect_pictures(&mut pictures);
        }
        pictures
    }

    /// For text shown under a title of its own: its headings rank one level lower.
    pub fn nested_one_level(self) -> Self {
        Self {
            blocks: self
                .blocks
                .into_iter()
                .map(Block::nested_one_level)
                .collect(),
        }
    }
}

impl Block {
    /// The Mermaid text, when this is a `mermaid` fence.
    pub fn diagram(&self) -> Option<&str> {
        match self {
            Self::Code {
                language: Some(language),
                text,
                ..
            } if language.eq_ignore_ascii_case("mermaid") => Some(text),
            _ => None,
        }
    }

    fn collect_diagrams<'a>(&'a self, diagrams: &mut Vec<&'a str>) {
        let mut collect = |blocks: &'a [Self]| {
            for block in blocks {
                block.collect_diagrams(diagrams);
            }
        };
        match self {
            Self::List { items, .. } => items.iter().for_each(|item| collect(item)),
            Self::Quote(blocks) | Self::Details { body: blocks, .. } => collect(blocks),
            block => diagrams.extend(block.diagram()),
        }
    }

    fn collect_pictures<'a>(&'a self, pictures: &mut Vec<(&'a Path, &'a str)>) {
        let mut collect = |blocks: &'a [Self]| {
            for block in blocks {
                block.collect_pictures(pictures);
            }
        };
        match self {
            Self::List { items, .. } => items.iter().for_each(|item| collect(item)),
            Self::Quote(blocks) | Self::Details { body: blocks, .. } => collect(blocks),
            Self::Picture { path, alt, .. } => pictures.push((path, alt)),
            _ => {}
        }
    }

    fn nested_one_level(self) -> Self {
        let nested = |blocks: Vec<Self>| blocks.into_iter().map(Self::nested_one_level).collect();
        match self {
            Self::Heading { level, text } => Self::Heading {
                level: (level + 1).min(6),
                text,
            },
            Self::List {
                first_number,
                items,
            } => Self::List {
                first_number,
                items: items.into_iter().map(nested).collect(),
            },
            Self::Quote(blocks) => Self::Quote(nested(blocks)),
            Self::Details { summary, body } => Self::Details {
                summary,
                body: nested(body),
            },
            other => other,
        }
    }
}

/// GitHub links bare `http(s)://` URLs in text; pulldown-cmark only links `<url>`.
fn split_bare_urls(text: &str) -> Vec<(&str, bool)> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = bare_url_start(rest) {
        let (before, candidate) = split_at_or_end(rest, start);
        let length = bare_url_length(candidate);
        if length == 0 {
            let (through_scheme_start, tail) = split_at_or_end(rest, start + 1);
            parts.push((through_scheme_start, false));
            rest = tail;
            continue;
        }
        if !before.is_empty() {
            parts.push((before, false));
        }
        let (url, tail) = split_at_or_end(candidate, length);
        parts.push((url, true));
        rest = tail;
    }
    if !rest.is_empty() {
        parts.push((rest, false));
    }
    parts
}

/// Splits where `at` is a character boundary, else keeps all the text before the split.
fn split_at_or_end(text: &str, at: usize) -> (&str, &str) {
    text.split_at_checked(at).unwrap_or((text, ""))
}

fn bare_url_start(text: &str) -> Option<usize> {
    [text.find("https://"), text.find("http://")]
        .into_iter()
        .flatten()
        .min()
}

/// Up to the first space or `<`, less trailing punctuation and any `)` it did not open; 0 when
/// nothing follows the scheme.
fn bare_url_length(candidate: &str) -> usize {
    let scheme = if candidate.starts_with("https://") {
        8
    } else {
        7
    };
    let mut end = candidate
        .find(|c: char| c.is_whitespace() || c == '<')
        .unwrap_or(candidate.len());
    loop {
        let url = candidate.get(..end).unwrap_or_default();
        let opened = url.matches('(').count();
        let closed = url.matches(')').count();
        match url.chars().last() {
            Some(c) if ".,;:!?'\"*_~".contains(c) => end -= c.len_utf8(),
            Some(')') if closed > opened => end -= 1,
            _ => break,
        }
    }
    if end > scheme { end } else { 0 }
}

enum Frame {
    Root,
    Quote,
    Item,
    Details {
        summary: Vec<Run>,
    },
    List {
        first_number: Option<u64>,
        items: Vec<Vec<Block>>,
    },
}

struct Open {
    frame: Frame,
    blocks: Vec<Block>,
}

#[derive(Default)]
struct Style {
    bold: usize,
    italic: usize,
    strikethrough: usize,
    links: Vec<String>,
}

struct Builder {
    stack: Vec<Open>,
    runs: Vec<Run>,
    style: Style,
    code: Option<(Option<String>, String)>,
    heading: Option<u8>,
    table: Option<Table>,
    row: Vec<Vec<Run>>,
    image: Option<(String, String)>,
    /// The tour's own folder, whose files pictures and links may name.
    files: Option<PathBuf>,
    /// `</details>` seen inside a list item or quote: the fold closes once those close.
    details_to_close: usize,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            stack: vec![Open {
                frame: Frame::Root,
                blocks: Vec::new(),
            }],
            runs: Vec::new(),
            style: Style::default(),
            code: None,
            heading: None,
            table: None,
            row: Vec::new(),
            image: None,
            files: None,
            details_to_close: 0,
        }
    }
}

impl Builder {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match (&mut self.code, &mut self.image) {
                (Some((_, code)), _) => code.push_str(&text),
                (None, Some((_, alt))) => alt.push_str(&text),
                (None, None) => self.push_text(&text, false),
            },
            Event::Code(text) => self.push_text(&text, true),
            Event::SoftBreak => self.push_text(" ", false),
            Event::HardBreak => self.push_text("\n", false),
            Event::Rule => {
                self.flush_runs();
                self.push_block(Block::Rule);
            }
            Event::Html(html) | Event::InlineHtml(html) => self.html(&html),
            Event::TaskListMarker(checked) => {
                self.push_text(if checked { "[x] " } else { "[ ] " }, false);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.flush_runs(),
            Tag::Heading { level, .. } => {
                self.flush_runs();
                self.heading = Some(heading_number(level));
            }
            Tag::BlockQuote(_) => self.open(Frame::Quote),
            Tag::CodeBlock(kind) => {
                self.flush_runs();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .map(str::to_owned)
                        .filter(|language| !language.is_empty()),
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((language, String::new()));
            }
            Tag::List(first_number) => {
                self.flush_runs();
                self.open(Frame::List {
                    first_number,
                    items: Vec::new(),
                });
            }
            Tag::Item => self.open(Frame::Item),
            Tag::Table(alignments) => {
                self.flush_runs();
                self.table = Some(Table {
                    alignments: alignments.into_iter().map(ColumnAlignment::from).collect(),
                    ..Table::default()
                });
            }
            Tag::TableHead | Tag::TableRow => self.row.clear(),
            Tag::Emphasis => self.style.italic += 1,
            Tag::Strong => self.style.bold += 1,
            Tag::Strikethrough => self.style.strikethrough += 1,
            Tag::Link { dest_url, .. } => {
                let link = match self.file_named(&dest_url) {
                    Some(file) => format!("file://{}", file.display()),
                    None => dest_url.to_string(),
                };
                self.style.links.push(link);
            }
            Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                let runs = std::mem::take(&mut self.runs);
                if !runs.is_empty() {
                    self.push_block(Block::Paragraph(runs));
                }
            }
            TagEnd::Heading(_) => {
                let text = std::mem::take(&mut self.runs);
                let level = self.heading.take().unwrap_or(1);
                self.push_block(Block::Heading { level, text });
            }
            TagEnd::BlockQuote(_) => {
                self.flush_runs();
                if let Some(open) = self.close_frame(|frame| matches!(frame, Frame::Quote)) {
                    self.push_block(Block::Quote(open.blocks));
                }
                self.close_waiting_details();
            }
            TagEnd::CodeBlock => {
                if let Some((language, text)) = self.code.take() {
                    let spans = highlight_snippet(language.as_deref(), &text);
                    self.push_block(Block::Code {
                        language,
                        text,
                        spans,
                    });
                }
            }
            TagEnd::List(_) => {
                self.flush_runs();
                if let Some(Open {
                    frame:
                        Frame::List {
                            first_number,
                            items,
                        },
                    ..
                }) = self.close_frame(|frame| matches!(frame, Frame::List { .. }))
                {
                    self.push_block(Block::List {
                        first_number,
                        items,
                    });
                }
                self.close_waiting_details();
            }
            TagEnd::Item => {
                self.flush_runs();
                if let Some(open) = self.close_frame(|frame| matches!(frame, Frame::Item))
                    && let Some(Open {
                        frame: Frame::List { items, .. },
                        ..
                    }) = self.stack.last_mut()
                {
                    items.push(open.blocks);
                }
                self.close_waiting_details();
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.runs);
                self.row.push(cell);
            }
            TagEnd::TableHead => {
                let header = std::mem::take(&mut self.row);
                if let Some(table) = &mut self.table {
                    table.header = header;
                }
            }
            TagEnd::TableRow => {
                let row = std::mem::take(&mut self.row);
                if let Some(table) = &mut self.table {
                    table.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.push_block(Block::Table(table));
                }
            }
            TagEnd::Emphasis => self.style.italic = self.style.italic.saturating_sub(1),
            TagEnd::Strong => self.style.bold = self.style.bold.saturating_sub(1),
            TagEnd::Strikethrough => {
                self.style.strikethrough = self.style.strikethrough.saturating_sub(1);
            }
            TagEnd::Link => {
                self.style.links.pop();
            }
            TagEnd::Image => {
                if let Some((url, alt)) = self.image.take() {
                    let link = match self.file_named(&url).map(|file| Self::picture(file, &alt)) {
                        Some(Ok(picture)) => {
                            self.flush_runs();
                            self.push_block(picture);
                            return;
                        }
                        Some(Err(unreadable)) => format!("file://{}", unreadable.display()),
                        None => url,
                    };
                    let text = if alt.is_empty() {
                        "image".to_owned()
                    } else {
                        alt
                    };
                    self.runs.push(Run {
                        text,
                        link: Some(link),
                        ..self.style_run()
                    });
                }
            }
            _ => {}
        }
    }

    fn html(&mut self, html: &str) {
        let mut rest = html;
        while let Some((text, after_open)) = rest.split_once('<') {
            self.push_html_text(text);
            if let Some(after_comment) = after_open.strip_prefix("!--") {
                rest = after_comment
                    .split_once("-->")
                    .map_or("", |(_, after)| after);
                continue;
            }
            let Some((tag, after_tag)) = after_open.split_once('>') else {
                self.push_html_text(rest.get(text.len()..).unwrap_or_default());
                return;
            };
            self.tag(tag);
            rest = after_tag;
        }
        self.push_html_text(rest);
    }

    fn tag(&mut self, tag: &str) {
        let tag = tag.trim().trim_end_matches('/');
        let (closing, tag) = tag
            .strip_prefix('/')
            .map_or((false, tag), |tag| (true, tag));
        let name = tag
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match (name.as_str(), closing) {
            ("details", false) => self.open(Frame::Details {
                summary: Vec::new(),
            }),
            ("details", true) => self.close_details(),
            ("summary", false) => self.flush_runs(),
            ("summary", true) => {
                let runs = std::mem::take(&mut self.runs);
                if let Some(Open {
                    frame: Frame::Details { summary },
                    ..
                }) = self.stack.last_mut()
                {
                    *summary = runs;
                }
            }
            ("br", _) => self.push_text("\n", false),
            ("img", false) => {
                if let Some(src) = attribute(tag, "src") {
                    let text = attribute(tag, "alt").filter(|alt| !alt.is_empty());
                    self.runs.push(Run {
                        text: text.unwrap_or_else(|| "image".to_owned()),
                        link: Some(src),
                        ..self.style_run()
                    });
                }
            }
            ("a", false) => {
                if let Some(href) = attribute(tag, "href") {
                    self.style.links.push(href);
                }
            }
            ("a", true) => {
                self.style.links.pop();
            }
            ("b" | "strong", closing) => bump(&mut self.style.bold, closing),
            ("i" | "em", closing) => bump(&mut self.style.italic, closing),
            _ => {}
        }
    }

    fn push_html_text(&mut self, text: &str) {
        let text = text
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        let after_space = self
            .runs
            .last()
            .is_none_or(|run| run.text.ends_with(char::is_whitespace));
        let text = if after_space {
            text.trim_start()
        } else {
            &text
        };
        if !text.is_empty() {
            self.push_text(text, false);
        }
    }

    /// A `</details>` inside a list item or quote the fold holds waits until those close.
    fn close_details(&mut self) {
        self.flush_runs();
        let is_open = self
            .stack
            .iter()
            .any(|open| matches!(open.frame, Frame::Details { .. }));
        if !is_open {
            return;
        }
        self.details_to_close += 1;
        self.close_waiting_details();
    }

    fn close_waiting_details(&mut self) {
        while self.details_to_close > 0 && self.top_is_details() {
            self.details_to_close -= 1;
            self.close_top();
        }
    }

    fn top_is_details(&self) -> bool {
        self.stack
            .last()
            .is_some_and(|open| matches!(open.frame, Frame::Details { .. }))
    }

    /// Closes the innermost frame of a kind, and any fold still open inside it. `None` when no
    /// such frame is open, so a stray end tag never closes a frame it did not open.
    fn close_frame(&mut self, is: impl Fn(&Frame) -> bool) -> Option<Open> {
        let index = self.stack.iter().rposition(|open| is(&open.frame))?;
        while self.stack.len() > index + 1 {
            self.close_top();
        }
        self.stack.pop()
    }

    /// Puts the innermost frame into its parent: a fold as a fold, anything else as a quote.
    /// The root stays.
    fn close_top(&mut self) {
        self.flush_runs();
        if self.stack.len() < 2 {
            return;
        }
        let Some(open) = self.stack.pop() else {
            return;
        };
        let block = match open.frame {
            Frame::Details { summary } => Block::Details {
                summary,
                body: open.blocks,
            },
            Frame::Root | Frame::Quote | Frame::Item | Frame::List { .. } => {
                Block::Quote(open.blocks)
            }
        };
        self.push_block(block);
    }

    fn open(&mut self, frame: Frame) {
        self.flush_runs();
        self.stack.push(Open {
            frame,
            blocks: Vec::new(),
        });
    }

    fn file_named(&self, url: &str) -> Option<PathBuf> {
        file_in(self.files.as_ref()?, url)
    }

    fn picture(path: PathBuf, alt: &str) -> Result<Block, PathBuf> {
        let Some(size) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| image_facts::measure(&path.to_string_lossy(), &bytes))
        else {
            return Err(path);
        };
        Ok(Block::Picture {
            path,
            alt: alt.to_owned(),
            width: size.width,
            height: size.height,
        })
    }

    fn push_block(&mut self, block: Block) {
        if let Some(open) = self.stack.last_mut() {
            open.blocks.push(block);
        }
    }

    fn flush_runs(&mut self) {
        let runs = std::mem::take(&mut self.runs);
        if !runs.is_empty() {
            self.push_block(Block::Paragraph(runs));
        }
    }

    fn style_run(&self) -> Run {
        Run {
            text: String::new(),
            bold: self.style.bold > 0,
            italic: self.style.italic > 0,
            strikethrough: self.style.strikethrough > 0,
            code: false,
            link: self.style.links.last().cloned(),
        }
    }

    fn push_text(&mut self, text: &str, code: bool) {
        let after_space = self.runs.last().is_some_and(|run| run.text.ends_with(' '));
        let text = if after_space && !code {
            text.trim_start_matches(' ')
        } else {
            text
        };
        if text.is_empty() {
            return;
        }
        let style = self.style_run();
        if code || style.link.is_some() {
            self.runs.push(Run {
                text: text.to_owned(),
                code,
                ..style
            });
            return;
        }
        for (part, is_url) in split_bare_urls(text) {
            self.runs.push(Run {
                text: part.to_owned(),
                link: is_url.then(|| part.to_owned()),
                ..style.clone()
            });
        }
    }

    fn finish(mut self) -> Markdown {
        self.flush_runs();
        while self.stack.len() > 1 {
            self.close_top();
        }
        Markdown {
            blocks: self.stack.pop().map_or_default(|open| open.blocks),
        }
    }
}

fn bump(counter: &mut usize, closing: bool) {
    *counter = if closing {
        counter.saturating_sub(1)
    } else {
        *counter + 1
    };
}

fn heading_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// `name="value"` or `name='value'` inside a tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let start = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = tag.get(start..)?;
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let (value, _) = rest.strip_prefix(quote)?.split_once(quote)?;
    Some(value.to_owned())
}
