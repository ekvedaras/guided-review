use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context as _;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::pieces::{self, ChangedFile, PieceId};
use crate::pull_request::Sha;
use crate::review_subject::ReviewSubject;
use crate::tour::{BasicTour, GuidedTour, Tour};

use super::new_commits::{PickCodeSource, RepinnedTour, local_facts_of};
use super::review::{Review, StepId};
use super::review_state::ReviewStateFile;
use super::step_page::{pieces_changed_since, pieces_of};

/// Switch to guided tour as the app runs it: the basic tour's read ticks go to the guided tour's
/// steps, the basic file goes, and a review of the guided tour opens.
pub struct TourSwitcher {
    pub folders: AppFolders,
    pub subject: ReviewSubject,
    pub pick_code_source: PickCodeSource,
}

impl TourSwitcher {
    /// Blocks on the code source and on git; the app runs it off the main thread.
    pub fn run(&self) -> anyhow::Result<RepinnedTour> {
        let guided =
            GuidedTour::load_of(&Tour::path_for(&self.folders, &self.subject), &self.subject)?;
        let source = (self.pick_code_source)(&guided.pin.pinned_tips())?;
        let state_file = ReviewStateFile::for_subject(&self.folders, &self.subject);
        let files = pieces::cut(source.as_ref(), &guided.pin.base, &guided.pin.head)?;
        let reads = state_file.load()?.file_read_heads();
        let heads = reads.values().cloned().chain(self.basic_head()).collect();
        let carrier = ReadCarrier::of(&guided, &files, &heads, source.as_ref())?;
        let carried = carrier.carry(&reads);
        state_file.update(|state| state.carry_file_reads_into_steps(&carried))?;
        let facts = local_facts_of(&self.folders, &self.subject, &guided.pin)?;
        let mut review = Review::open(guided, source.as_ref(), state_file)?;
        if let Some(facts) = facts {
            review.local_facts_are(facts);
        }
        let started_read = review.progress().read;
        review.steps_carried_read_ticks_are(started_read);
        review.takes_over_from_basic_with(carrier);
        self.delete_basic_file()?;
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }

    /// The head the basic review ticks files at while the guided review is built.
    fn basic_head(&self) -> Option<Sha> {
        BasicTour::load_of(
            &Tour::basic_path_for(&self.folders, &self.subject),
            &self.subject,
        )
        .ok()
        .map(|basic| basic.pin.head)
    }

    /// Only once the state is saved: a crash before this carries again instead of losing ticks.
    fn delete_basic_file(&self) -> anyhow::Result<()> {
        let path = Tour::basic_path_for(&self.folders, &self.subject);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("could not delete {}", path.display()))
            }
        }
    }
}

/// What the basic tour's file ticks make of the guided tour's steps.
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
    fn of(
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
