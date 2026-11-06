/// How a line of a fenced code block is coloured, the way GitHub colours a `diff` fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceLine {
    Added,
    Removed,
    Plain,
}

/// One entry per line of `text`; every line is plain unless the fence is `diff` or `patch`.
pub fn fence_lines(language: Option<&str>, text: &str) -> Vec<FenceLine> {
    let is_diff = language.is_some_and(|language| {
        language.eq_ignore_ascii_case("diff") || language.eq_ignore_ascii_case("patch")
    });
    text.lines()
        .map(|line| match line {
            _ if !is_diff => FenceLine::Plain,
            _ if line.starts_with("+++") || line.starts_with("---") => FenceLine::Plain,
            _ if line.starts_with('+') => FenceLine::Added,
            _ if line.starts_with('-') => FenceLine::Removed,
            _ => FenceLine::Plain,
        })
        .collect()
}
