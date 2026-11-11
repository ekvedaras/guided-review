//! Which words of a rewritten line changed, so a reader sees the edit inside a long line.

use std::ops::Range;

pub struct ChangedWords {
    pub old: Vec<Range<usize>>,
    pub new: Vec<Range<usize>>,
}

/// Below this share of their text in common, two lines are a rewrite, and marking words in them
/// would mark nearly everything.
const LEAST_SHARED: f32 = 0.4;
/// The word matching takes time in the product of the two lines' word counts.
const MOST_PAIRS: usize = 250_000;

/// `None` when the two lines share too little for marked words to help: all of it changed.
pub fn changed_words(old: &str, new: &str) -> Option<ChangedWords> {
    let old_tokens = tokens(old);
    let new_tokens = tokens(new);
    if old_tokens.len().saturating_mul(new_tokens.len()) > MOST_PAIRS {
        return None;
    }
    let old_words: Vec<&str> = old_tokens
        .iter()
        .filter_map(|t| old.get(t.clone()))
        .collect();
    let new_words: Vec<&str> = new_tokens
        .iter()
        .filter_map(|t| new.get(t.clone()))
        .collect();
    let (old_kept, new_kept) = kept_tokens(&old_words, &new_words);
    let shared: usize = old_words
        .iter()
        .zip(&old_kept)
        .filter(|(_, kept)| **kept)
        .map(|(word, _)| non_blank_len(word))
        .sum();
    let longer = non_blank_len(old).max(non_blank_len(new));
    if longer > 0 && (shared as f32) < LEAST_SHARED * longer as f32 {
        return None;
    }
    Some(ChangedWords {
        old: runs(old, &old_tokens, &old_kept),
        new: runs(new, &new_tokens, &new_kept),
    })
}

fn non_blank_len(text: &str) -> usize {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(char::len_utf8)
        .sum()
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum TokenKind {
    Word,
    Blank,
    Mark,
}

fn kind_of(c: char) -> TokenKind {
    if c.is_alphanumeric() || c == '_' {
        TokenKind::Word
    } else if c.is_whitespace() {
        TokenKind::Blank
    } else {
        TokenKind::Mark
    }
}

/// Words and runs of blanks whole, any other character alone.
fn tokens(text: &str) -> Vec<Range<usize>> {
    let mut tokens: Vec<Range<usize>> = Vec::new();
    let mut last_kind = None;
    for (start, c) in text.char_indices() {
        let kind = kind_of(c);
        let end = start + c.len_utf8();
        match tokens.last_mut() {
            Some(last) if kind != TokenKind::Mark && last_kind == Some(kind) => last.end = end,
            _ => tokens.push(start..end),
        }
        last_kind = Some(kind);
    }
    tokens
}

/// Which tokens of each side the longest common run of tokens keeps, taking the earliest match.
fn kept_tokens(old: &[&str], new: &[&str]) -> (Vec<bool>, Vec<bool>) {
    let width = new.len() + 1;
    let mut longest = vec![0_u32; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            let value = if old.get(i) == new.get(j) {
                longest.get((i + 1) * width + j + 1).copied().unwrap_or(0) + 1
            } else {
                let below = longest.get((i + 1) * width + j).copied().unwrap_or(0);
                let right = longest.get(i * width + j + 1).copied().unwrap_or(0);
                below.max(right)
            };
            if let Some(cell) = longest.get_mut(i * width + j) {
                *cell = value;
            }
        }
    }
    let at = |i: usize, j: usize| longest.get(i * width + j).copied().unwrap_or(0);
    let (mut old_kept, mut new_kept) = (vec![false; old.len()], vec![false; new.len()]);
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old.get(i) == new.get(j) {
            if let Some(kept) = old_kept.get_mut(i) {
                *kept = true;
            }
            if let Some(kept) = new_kept.get_mut(j) {
                *kept = true;
            }
            i += 1;
            j += 1;
        } else if at(i + 1, j) >= at(i, j + 1) {
            i += 1;
        } else {
            j += 1;
        }
    }
    (old_kept, new_kept)
}

/// Changed tokens with only blanks between them read as one run; a run never starts or ends on a blank.
fn runs(text: &str, tokens: &[Range<usize>], kept: &[bool]) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    let mut open: Option<Range<usize>> = None;
    for (token, &is_kept) in tokens.iter().zip(kept) {
        let is_blank = text
            .get(token.clone())
            .is_some_and(|word| word.chars().all(char::is_whitespace));
        if is_blank {
            continue;
        }
        match (is_kept, open.as_mut()) {
            (false, Some(run)) => run.end = token.end,
            (false, None) => open = Some(token.clone()),
            (true, _) => runs.extend(open.take()),
        }
    }
    runs.extend(open);
    runs
}
