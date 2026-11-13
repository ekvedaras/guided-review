use std::ops::Range;

use super::Capture;

/// No Blade grammar exists for tree-sitter 0.27, so a Blade file is read in two layers: its Blade
/// parts are found here by scanning, and what is left over is plain HTML for the HTML grammar.
#[derive(Debug, Default)]
pub(super) struct BladeScan {
    /// Parts coloured by the scan itself: directive names, delimiters, comments.
    pub marks: Vec<(Range<usize>, Capture)>,
    /// PHP written inside Blade, to be coloured with the PHP grammar.
    pub php: Vec<PhpCode>,
    /// Everything Blade owns, hidden from the HTML grammar.
    blade: Vec<Range<usize>>,
}

/// A stretch of PHP and the opening that makes it parse as a statement on its own.
#[derive(Debug)]
pub(super) struct PhpCode {
    pub range: Range<usize>,
    pub opening: String,
    pub closing: &'static str,
}

impl BladeScan {
    pub fn of(source: &str) -> Self {
        let mut scan = Self::default();
        let bytes = source.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            at = scan.part_at(source, at).unwrap_or(at + 1);
        }
        scan
    }

    /// The source with Blade parts blanked out, byte for byte, so HTML spans keep their offsets.
    pub fn html_only(&self, source: &str) -> String {
        let mut bytes = source.as_bytes().to_vec();
        for range in &self.blade {
            for byte in bytes.get_mut(range.clone()).into_iter().flatten() {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
        }
        String::from_utf8(bytes).unwrap_or_default()
    }

    /// Where the scan goes on when a Blade part starts at `at`; None when none does.
    fn part_at(&mut self, source: &str, at: usize) -> Option<usize> {
        let rest = source.get(at..)?;
        if rest.starts_with("{{--") {
            let end = rest.find("--}}").map_or(rest.len(), |close| close + 4);
            return Some(self.comment(at..at + end));
        }
        if rest.starts_with("{{") && !escaped(source, at) {
            return Some(self.echo(source, at, "{{", "}}"));
        }
        if rest.starts_with("{!!") && !escaped(source, at) {
            return Some(self.echo(source, at, "{!!", "!!}"));
        }
        if rest.starts_with("@@") {
            return Some(at + 2);
        }
        if rest.starts_with('@') && !follows_word(source, at) {
            return self.directive(source, at);
        }
        None
    }

    fn comment(&mut self, range: Range<usize>) -> usize {
        let end = range.end;
        self.blade.push(range.clone());
        self.marks.push((range, Capture::Comment));
        end
    }

    fn echo(&mut self, source: &str, at: usize, open: &str, close: &str) -> usize {
        let inner_start = at + open.len();
        let Some(length) = from(source, inner_start).find(close) else {
            return inner_start;
        };
        let inner_end = inner_start + length;
        let end = inner_end + close.len();
        self.delimiter(at..inner_start);
        self.delimiter(inner_end..end);
        self.php(inner_start..inner_end, "<?php ", ";");
        end
    }

    fn directive(&mut self, source: &str, at: usize) -> Option<usize> {
        let name_start = at + 1;
        let name: String = from(source, name_start)
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect();
        if !is_directive(&name) {
            return None;
        }
        let name_end = name_start + name.len();
        self.blade.push(at..name_end);
        self.marks.push((at..name_end, Capture::Keyword));
        if takes_no_arguments(&name) {
            return Some(name_end);
        }
        let after_spaces = name_end
            + from(source, name_end)
                .bytes()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count();
        if from(source, after_spaces).starts_with('(') {
            let close = closing_parenthesis(source, after_spaces)?;
            self.blade.push(after_spaces..close + 1);
            self.marks
                .push((after_spaces..after_spaces + 1, Capture::PunctuationBracket));
            self.marks
                .push((close..close + 1, Capture::PunctuationBracket));
            self.php(
                after_spaces + 1..close,
                &format!("<?php {}(", php_stand_in(&name)),
                ");",
            );
            return Some(close + 1);
        }
        if name == "php" {
            let end = from(source, name_end)
                .find("@endphp")
                .map_or(source.len(), |close| name_end + close);
            self.php(name_end..end, "<?php ", "");
            return Some(end);
        }
        Some(name_end)
    }

    fn delimiter(&mut self, range: Range<usize>) {
        self.blade.push(range.clone());
        self.marks.push((range, Capture::PunctuationSpecial));
    }

    fn php(&mut self, range: Range<usize>, opening: &str, closing: &'static str) {
        self.blade.push(range.clone());
        self.php.push(PhpCode {
            range,
            opening: opening.to_owned(),
            closing,
        });
    }
}

fn from(source: &str, at: usize) -> &str {
    source.get(at..).unwrap_or_default()
}

fn before(source: &str, at: usize) -> &str {
    source.get(..at).unwrap_or_default()
}

/// `@{{ name }}` prints the braces as they are.
fn escaped(source: &str, at: usize) -> bool {
    before(source, at).ends_with('@')
}

/// `me@example.com` is an address, not a directive.
fn follows_word(source: &str, at: usize) -> bool {
    before(source, at)
        .chars()
        .next_back()
        .is_some_and(|before| before.is_alphanumeric() || before == '_')
}

/// The `)` that closes the `(` at `open`, stepping over strings.
fn closing_parenthesis(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut skip_next = false;
    for (offset, character) in from(source, open).char_indices() {
        if skip_next {
            skip_next = false;
            continue;
        }
        match (quote, character) {
            (Some(_), '\\') => skip_next = true,
            (Some(open_quote), character) if character == open_quote => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// The PHP construct whose syntax a directive's arguments imitate, so `$users as $user` parses.
fn php_stand_in(directive: &str) -> &str {
    match directive {
        "forelse" => "foreach",
        "elseif" | "unless" | "elseunless" => "if",
        other => other,
    }
}

fn takes_no_arguments(directive: &str) -> bool {
    directive.starts_with("end")
        || matches!(
            directive,
            "else" | "csrf" | "once" | "verbatim" | "parent" | "show" | "stop" | "append"
        )
}

/// Laravel's own directives. Others (`@media`, `@click`) are CSS or Alpine, not Blade.
fn is_directive(name: &str) -> bool {
    const DIRECTIVES: &[&str] = &[
        "append",
        "auth",
        "break",
        "can",
        "cannot",
        "case",
        "checked",
        "class",
        "component",
        "concat",
        "continue",
        "csrf",
        "default",
        "disabled",
        "dd",
        "dump",
        "else",
        "elseauth",
        "elsecan",
        "elseguest",
        "elseif",
        "empty",
        "env",
        "error",
        "extends",
        "for",
        "foreach",
        "forelse",
        "guest",
        "hasSection",
        "if",
        "include",
        "includeFirst",
        "includeIf",
        "includeUnless",
        "includeWhen",
        "inject",
        "isset",
        "json",
        "lang",
        "livewire",
        "livewireScripts",
        "livewireStyles",
        "method",
        "once",
        "overwrite",
        "parent",
        "persist",
        "php",
        "prepend",
        "production",
        "props",
        "push",
        "pushIf",
        "pushOnce",
        "readonly",
        "required",
        "section",
        "selected",
        "session",
        "show",
        "slot",
        "sectionMissing",
        "stack",
        "stop",
        "style",
        "switch",
        "unless",
        "use",
        "verbatim",
        "vite",
        "viteReactRefresh",
        "while",
        "yield",
    ];
    let name = name.trim_prefix("end");
    DIRECTIVES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name))
}
