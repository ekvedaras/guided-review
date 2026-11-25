//! What one review carries into another: a basic review's file reads into the guided tour's steps,
//! and everything of a local review into the PR it moves to.

use std::collections::{BTreeMap, BTreeSet};

use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::pieces::{self, ChangedFile, PieceId};
use crate::pull_request::Sha;
use crate::tour::{GuidedTour, StepId};

use super::local_threads::MovedComment;
use super::piece_diff::{pieces_changed_since, pieces_of};

/// What the basic tour's file ticks make of the guided tour's steps.
#[derive(Default)]
pub(super) struct CarriedReads {
    /// Steps that start read, at the oldest head their files were read at.
    pub(super) ticked: Vec<(StepId, Sha)>,
    /// Steps whose files were read but a piece they show changed since: they show what changed.
    pub(super) read_before: Vec<(StepId, Sha)>,
}

/// What carrying file reads into the guided tour's steps needs from git, found off the main thread
/// for every head a file was or can be read at. The basic review stays on screen while the guided
/// review is built, so the files read meanwhile are carried again at the hand-over, with no git.
pub(super) struct ReadCarrier {
    head: Sha,
    /// The pieces each step shows.
    steps: Vec<(StepId, Vec<PieceId>)>,
    /// By the head a file was read at: the shown pieces that changed since.
    changed_since: BTreeMap<Sha, BTreeSet<PieceId>>,
    /// Pairs of known heads where the first is an ancestor of the second.
    ancestors: BTreeSet<(Sha, Sha)>,
}

impl ReadCarrier {
    pub(super) fn of(
        guided: &GuidedTour,
        files: &[ChangedFile],
        heads: &BTreeSet<Sha>,
        source: &dyn CodeSource,
    ) -> anyhow::Result<Self> {
        let head = guided.pin.head.clone();
        let shown: Vec<(StepId, Vec<_>)> = guided
            .steps()
            .into_iter()
            .map(|step| (step.id.clone(), pieces_of(step, guided, files)))
            .collect();
        let mut changed_since = BTreeMap::new();
        for read_head in heads.iter().filter(|read_head| **read_head != head) {
            let since = pieces::cut(source, read_head, &head)?;
            let changed: BTreeSet<PieceId> = shown
                .iter()
                .flat_map(|(_, pieces)| pieces)
                .filter(|piece| {
                    since
                        .iter()
                        .find(|file| file.path == *piece.id.path())
                        .is_some_and(|file| !pieces_changed_since(piece, file).is_empty())
                })
                .map(|piece| piece.id.clone())
                .collect();
            changed_since.insert(read_head.clone(), changed);
        }
        let mut ancestors = BTreeSet::new();
        for earlier in heads {
            for later in heads.iter().filter(|later| *later != earlier) {
                if source.merge_base(earlier, later)? == *earlier {
                    ancestors.insert((earlier.clone(), later.clone()));
                }
            }
        }
        Ok(Self {
            head,
            steps: shown
                .into_iter()
                .map(|(id, pieces)| (id, pieces.iter().map(|piece| piece.id.clone()).collect()))
                .collect(),
            changed_since,
            ancestors,
        })
    }

    /// A step whose files were all read starts ticked, or shows what changed when a piece it shows
    /// changed since. A step with a file read at a head the carrier does not know stays unread.
    pub(super) fn carry(&self, reads: &BTreeMap<FilePath, Sha>) -> CarriedReads {
        let mut carried = CarriedReads {
            ticked: Vec::new(),
            read_before: Vec::new(),
        };
        for (id, pieces) in &self.steps {
            let Some(read_heads) = pieces
                .iter()
                .map(|piece| Some((piece, reads.get(piece.path())?)))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let Some(changed) = read_heads
                .iter()
                .map(|(piece, read_head)| self.has_changed_since(piece, read_head))
                .collect::<Option<Vec<bool>>>()
            else {
                continue;
            };
            let Some(oldest) = self.oldest_of(read_heads.iter().map(|(_, head)| *head)) else {
                continue;
            };
            if changed.contains(&true) {
                carried.read_before.push((id.clone(), oldest));
            } else {
                carried.ticked.push((id.clone(), oldest));
            }
        }
        carried
    }

    fn has_changed_since(&self, piece: &PieceId, read_head: &Sha) -> Option<bool> {
        if *read_head == self.head {
            return Some(false);
        }
        Some(self.changed_since.get(read_head)?.contains(piece))
    }

    fn oldest_of<'a>(&self, heads: impl Iterator<Item = &'a Sha>) -> Option<Sha> {
        let mut oldest: Option<&Sha> = None;
        for head in heads {
            oldest = Some(match oldest {
                Some(earlier)
                    if earlier == head
                        || self.ancestors.contains(&(earlier.clone(), head.clone())) =>
                {
                    earlier
                }
                _ => head,
            });
        }
        oldest.cloned()
    }
}

/// What a local review hands to the PR it moves to, worked out before anything is saved.
pub(super) struct MovedReview {
    /// The PR's head the move was worked out for, which the drafts' lines are at.
    pub(super) head: Sha,
    pub(super) file_ticks: BTreeMap<FilePath, Sha>,
    pub(super) file_read_before: BTreeMap<FilePath, Sha>,
    pub(super) steps: CarriedReads,
    pub(super) comments: Vec<MovedComment>,
}

impl MovedReview {
    /// The heads the PR's review reads files or steps at that are not its own head: the local
    /// review's snapshots, which nothing keeps once the local review is gone.
    pub(super) fn heads_before_the_pr(&self) -> BTreeSet<Sha> {
        self.file_ticks
            .values()
            .chain(self.file_read_before.values())
            .chain(self.steps.ticked.iter().map(|(_, head)| head))
            .chain(self.steps.read_before.iter().map(|(_, head)| head))
            .filter(|head| **head != self.head)
            .cloned()
            .collect()
    }
}
