use super::markdown::{Block, Markdown};

/// A PR template can run for screens, so a long description shows only its start until the reader unfolds it.
const BLOCKS_SHOWN_FOLDED: usize = 4;

/// The PR's body as GitHub has it, shown under the tour's summary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrDescription {
    pub shown_blocks: Vec<Block>,
    /// How many blocks the fold hides; 0 when it shows all of them.
    pub folded_blocks: usize,
    /// It is long enough to fold, and the reader unfolded it.
    pub can_fold: bool,
}

impl PrDescription {
    /// `None` for a PR with no description.
    pub(super) fn of(body: &str, is_unfolded: bool) -> Option<Self> {
        if body.trim().is_empty() {
            return None;
        }
        let mut blocks = Markdown::parse(body).blocks;
        let is_long = blocks.len() > BLOCKS_SHOWN_FOLDED;
        let folded_blocks = if is_long && !is_unfolded {
            blocks.split_off(BLOCKS_SHOWN_FOLDED).len()
        } else {
            0
        };
        Some(Self {
            shown_blocks: blocks,
            folded_blocks,
            can_fold: is_long && is_unfolded,
        })
    }
}
