use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use anyhow::Context as _;

use crate::app_folders::AppFolders;
use crate::code_source::CodeSource;
use crate::file_path::FilePath;
use crate::pieces::{self, ChangedFile, Piece};
use crate::pull_request::{PullRequestUrl, Sha};
use crate::tour::{GuidedTour, Tour};

use super::new_commits::{PickCodeSource, RepinnedTour};
use super::review::{Review, StepId, read_head_of_file};
use super::review_state::{ReviewState, ReviewStateFile};
use super::step_page::{pieces_changed_since, pieces_of};

/// Switch to guided tour as the app runs it: the basic tour's read ticks go to the guided tour's
/// steps, the basic file goes, and a review of the guided tour opens.
pub struct TourSwitcher {
    pub folders: AppFolders,
    pub pr: PullRequestUrl,
    pub pick_code_source: PickCodeSource,
}

impl TourSwitcher {
    /// Blocks on the code source and on git; the app runs it off the main thread.
    pub fn run(&self) -> anyhow::Result<RepinnedTour> {
        let guided = GuidedTour::load(&Tour::path_for(&self.folders, &self.pr))?;
        let source = (self.pick_code_source)(&guided.pin.pinned_tips())?;
        let state_file = ReviewStateFile::for_pr(&self.folders, &self.pr);
        let files = pieces::cut(source.as_ref(), &guided.pin.base, &guided.pin.head)?;
        let carried = CarriedReads::of(&state_file.load()?, &guided, &files, source.as_ref())?;
        let started_read = carried.ticked.len();
        state_file.update(|state| state.carry_file_reads_into_steps(&carried))?;
        let mut review = Review::open(guided, source.as_ref(), state_file)?;
        review.steps_start_read(started_read);
        self.delete_basic_file()?;
        Ok(RepinnedTour {
            review,
            source: source.into(),
        })
    }

    /// Only once the state is saved: a crash before this carries again instead of losing ticks.
    fn delete_basic_file(&self) -> anyhow::Result<()> {
        let path = Tour::basic_path_for(&self.folders, &self.pr);
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

impl CarriedReads {
    fn of(
        state: &ReviewState,
        guided: &GuidedTour,
        files: &[ChangedFile],
        source: &dyn CodeSource,
    ) -> anyhow::Result<Self> {
        let head = &guided.pin.head;
        let mut changes_since: BTreeMap<Sha, Vec<ChangedFile>> = BTreeMap::new();
        let mut carried = Self {
            ticked: Vec::new(),
            read_before: Vec::new(),
        };
        for step in guided.steps() {
            let shown = pieces_of(step, guided, files);
            let Some(read_heads) = read_heads_of_files(state, &shown) else {
                continue;
            };
            let oldest = oldest_of(read_heads.values().copied(), source)?;
            let mut changed = false;
            for piece in &shown {
                let Some(&read_head) = read_heads.get(piece.id.path()) else {
                    continue;
                };
                if read_head == head {
                    continue;
                }
                let since = match changes_since.entry(read_head.clone()) {
                    Entry::Occupied(known) => known.into_mut(),
                    Entry::Vacant(room) => room.insert(pieces::cut(source, read_head, head)?),
                };
                changed |= since
                    .iter()
                    .find(|file| file.path == *piece.id.path())
                    .is_some_and(|file| !pieces_changed_since(piece, file).is_empty());
            }
            let id: StepId = step.id.clone();
            if changed {
                carried.read_before.push((id, oldest));
            } else {
                carried.ticked.push((id, oldest));
            }
        }
        Ok(carried)
    }
}

/// The head each file the pieces are of was read at; `None` when a file was never read or the
/// step shows no pieces.
fn read_heads_of_files<'a>(
    state: &'a ReviewState,
    shown: &[&'a Piece],
) -> Option<BTreeMap<&'a FilePath, &'a Sha>> {
    let heads: BTreeMap<&FilePath, &Sha> = shown
        .iter()
        .map(|piece| piece.id.path())
        .map(|path| Some((path, read_head_of_file(state, path)?)))
        .collect::<Option<_>>()?;
    (!heads.is_empty()).then_some(heads)
}

fn oldest_of<'a>(
    heads: impl Iterator<Item = &'a Sha>,
    source: &dyn CodeSource,
) -> anyhow::Result<Sha> {
    let mut oldest: Option<&Sha> = None;
    for head in heads {
        oldest = Some(match oldest {
            Some(earlier) if source.merge_base(earlier, head)? == *earlier => earlier,
            _ => head,
        });
    }
    oldest.cloned().context("a step with files has a read head")
}
