use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::app_folders::{AppFolders, remove_if_there};
use crate::file_path::FilePath;
use crate::findings::Finding;
use crate::pieces::PieceId;
use crate::pull_request::{PullRequestUrl, RepoName, Sha};
use crate::repin::{Repin, RepinStatus};
use crate::review_subject::{LOCAL_FILES_FOLDER, ReviewSubject};
use crate::timestamp::UtcTimestamp;
use crate::tour::Tour;

use super::ask::{Ask, AskStore};
use super::comments::{CommentDraft, CommentPlace, QueuedComment};
use super::finding_decisions::{Fate, FindingDecisions};
use super::hand_offs::{AgentReplied, AgentReplyRefused, HandOffId, HandOffs};
use super::local_threads::LocalThreads;
use super::move_to_pr::MovedReview;
use super::ports::Viewed;
use super::review::{ReadSubject, StepId};
use super::switch_to_guided::CarriedReads;
use super::threads::ThreadId;

/// PR links in the state file stay the strings an earlier build wrote.
mod pull_request_links {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::pull_request::PullRequestUrl;

    pub fn serialize<S: Serializer>(prs: &[PullRequestUrl], to: S) -> Result<S::Ok, S::Error> {
        prs.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .serialize(to)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        from: D,
    ) -> Result<Vec<PullRequestUrl>, D::Error> {
        Vec::<String>::deserialize(from)?
            .iter()
            .map(|link| link.parse().map_err(serde::de::Error::custom))
            .collect()
    }
}

/// What the viewer remembers about one PR, apart from the tour: everything that is the reviewer's,
/// not the tour writer's. Every section defaults to empty, so a file written before a section
/// existed still loads.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReviewState {
    #[serde(flatten)]
    viewer: ViewerSections,
    #[serde(flatten)]
    typed: TypedText,
    #[serde(default)]
    asks: Vec<Ask>,
    /// The key of the local review the file belongs to. Its path holds only a short hash of the folder, so
    /// the key is what tells a file of another review that landed on the same path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
    #[serde(default)]
    last_repin: Option<RepinRecord>,
    /// The folder the PR was last opened from, so a switch back to it finds the same clone.
    #[serde(default)]
    opened_from: Option<PathBuf>,
    /// The PRs whose offer to move this local review to them was declined, written as their links.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "pull_request_links"
    )]
    declined_moves: Vec<PullRequestUrl>,
    /// A local review's threads, written only through `LocalThreadStore`, never with the viewer's sections.
    #[serde(default, skip_serializing_if = "LocalThreads::is_empty")]
    local_threads: LocalThreads,
    /// The clone whose refs keep the snapshots a moved local review was read at, while this review
    /// still reads a file or step at one of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    read_heads_kept_in: Option<PathBuf>,
    /// Sections this build does not know, written by a newer one. Kept so saving does not drop them.
    #[serde(flatten)]
    unknown_sections: serde_json::Map<String, serde_json::Value>,
}

/// The sections the viewer owns and writes back whole, with `typed` beside it (serde cannot
/// flatten one flattened struct into another); the Asks and the re-pin record are written by
/// others, so a save of these never touches them.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ViewerSections {
    #[serde(default)]
    read_ticks: BTreeMap<StepId, ReadTick>,
    /// Per file, whether GitHub said viewed at the last sync. A pull reacts to changes against it.
    #[serde(default)]
    viewed_on_github: BTreeMap<FilePath, Viewed>,
    /// Viewed changes GitHub has not taken yet, newest wish per file.
    #[serde(default)]
    unsent_viewed: BTreeMap<FilePath, Viewed>,
    /// Ticks a re-pin took away; the step was read at that head, so it can show what changed since.
    #[serde(default)]
    read_before: BTreeMap<StepId, ReadTick>,
    /// A basic tour's read ticks, per file, apart from the steps' so one never reads as the other.
    #[serde(default)]
    file_read_ticks: BTreeMap<FilePath, ReadTick>,
    /// What a re-pin took from the file ticks, as `read_before` does for steps.
    #[serde(default)]
    file_read_before: BTreeMap<FilePath, ReadTick>,
    /// After a re-pin GitHub's viewed state is taken as it is, so its "not viewed" for files
    /// the new commits touched is not read as the reviewer unticking.
    #[serde(default)]
    github_baseline_due: bool,
    /// What the reviewer did with the findings of an AI review.
    #[serde(default, skip_serializing_if = "FindingDecisions::is_empty")]
    finding_decisions: FindingDecisions,
    /// Comments GitHub did not take, oldest first.
    #[serde(default)]
    comment_queue: Vec<QueuedComment>,
    /// The PR's body at the last pull, so the summary shows it before GitHub answers.
    #[serde(default)]
    pr_description: String,
    /// The comments sent with Ask agent and where each is with the tour agent.
    #[serde(default)]
    hand_offs: HandOffs,
    /// The head the reviewer marked the review done at, with no new review sent. New commits move
    /// the head on, so the mark then says nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    done_at: Option<ReadTick>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadTick {
    /// The head the step was read at; after new commits it tells what changed since.
    pub head: Sha,
}

/// What the last re-pin (or re-pins, until a Refresh) did, from the head the tour was read at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepinRecord {
    pub from_base: Sha,
    pub from_head: Sha,
    pub to_base: Sha,
    pub to_head: Sha,
    /// Pieces at `to_head`, by id.
    #[serde(deserialize_with = "statuses_of_readable_ids")]
    pub pieces: BTreeMap<PieceId, RepinStatus>,
    /// By old id: a gone hunk's ordinal may belong to another hunk now.
    #[serde(deserialize_with = "readable_gone_ids")]
    pub gone_by_step: BTreeMap<StepId, BTreeSet<PieceId>>,
}

impl RepinRecord {
    pub fn new(
        from_base: &Sha,
        from_head: &Sha,
        to_base: &Sha,
        to_head: &Sha,
        repin: &Repin,
    ) -> Self {
        Self {
            from_base: from_base.clone(),
            from_head: from_head.clone(),
            to_base: to_base.clone(),
            to_head: to_head.clone(),
            pieces: repin
                .pieces
                .iter()
                .filter(|piece| piece.status != RepinStatus::Gone)
                .map(|piece| (piece.id.clone(), piece.status))
                .collect(),
            gone_by_step: repin
                .steps
                .iter()
                .map(|step| {
                    let gone = step
                        .pieces
                        .iter()
                        .filter(|piece| piece.status == RepinStatus::Gone)
                        .map(|piece| piece.id.clone())
                        .collect();
                    (step.step.clone(), gone)
                })
                .filter(|(_, gone): &(StepId, BTreeSet<PieceId>)| !gone.is_empty())
                .collect(),
        }
    }

    pub fn status_of(&self, piece: &PieceId) -> Option<RepinStatus> {
        self.pieces
            .get(piece)
            .or_else(|| {
                self.pieces
                    .iter()
                    .find_map(|(id, status)| id.may_be(piece).then_some(status))
            })
            .copied()
    }

    /// What the re-pin did to a file as a whole: `New` when all its pieces are new, `Changed` when
    /// some are changed or new, nothing when it stayed as it was.
    pub fn status_of_file(&self, path: &FilePath) -> Option<RepinStatus> {
        let statuses: Vec<RepinStatus> = self
            .pieces
            .iter()
            .filter(|(id, _)| id.path() == path)
            .map(|(_, status)| *status)
            .collect();
        if !statuses.is_empty() && statuses.iter().all(|status| *status == RepinStatus::New) {
            Some(RepinStatus::New)
        } else if statuses
            .iter()
            .any(|status| matches!(status, RepinStatus::Changed | RepinStatus::New))
        {
            Some(RepinStatus::Changed)
        } else {
            None
        }
    }

    pub fn gone_count(&self, step: &StepId) -> usize {
        self.gone_by_step.get(step).map_or(0, BTreeSet::len)
    }

    /// Two re-pins with no Refresh between read as one, from where the first one started:
    /// what changed in the first stays changed even when the second leaves it alone.
    fn following(self, earlier: &Self) -> Self {
        let was_new_then = |id: &PieceId| earlier.pieces.get(id) == Some(&RepinStatus::New);
        let pieces = self
            .pieces
            .into_iter()
            .map(|(id, now)| {
                let status = match (earlier.pieces.get(&id), now) {
                    (Some(RepinStatus::New), _) => RepinStatus::New,
                    (Some(RepinStatus::Changed), RepinStatus::Same) => RepinStatus::Changed,
                    _ => now,
                };
                (id, status)
            })
            .collect();
        let mut gone_by_step = earlier.gone_by_step.clone();
        for (step, gone) in self.gone_by_step {
            gone_by_step
                .entry(step)
                .or_default()
                .extend(gone.into_iter().filter(|id| !was_new_then(id)));
        }
        gone_by_step.retain(|_, gone| !gone.is_empty());
        Self {
            from_base: earlier.from_base.clone(),
            from_head: earlier.from_head.clone(),
            pieces,
            gone_by_step,
            ..self
        }
    }
}

/// An id no build wrote (empty, or an absolute path) is dropped, so one bad key never makes the
/// whole state file unreadable.
fn statuses_of_readable_ids<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<PieceId, RepinStatus>, D::Error> {
    let by_text = BTreeMap::<String, RepinStatus>::deserialize(deserializer)?;
    Ok(by_text
        .into_iter()
        .filter_map(|(text, status)| Some((text.parse().ok()?, status)))
        .collect())
}

fn readable_gone_ids<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<StepId, BTreeSet<PieceId>>, D::Error> {
    let by_step = BTreeMap::<StepId, Vec<String>>::deserialize(deserializer)?;
    Ok(by_step
        .into_iter()
        .map(|(step, texts)| {
            let ids = texts.iter().filter_map(|text| text.parse().ok()).collect();
            (step, ids)
        })
        .collect())
}

impl ReviewState {
    /// Steps whose pieces changed or gained new ones lose their tick; it is kept as `read_before`.
    pub fn record_repin(&mut self, record: RepinRecord, repin: &Repin) {
        for step in repin
            .steps
            .iter()
            .filter(|step| step.has_changed_or_new_piece())
        {
            let id = step.step.clone();
            if let Some(tick) = self.viewer.read_ticks.remove(&id) {
                self.viewer.read_before.insert(id, tick);
            }
        }
        let touched_files: BTreeSet<&FilePath> = repin
            .pieces
            .iter()
            .filter(|piece| matches!(piece.status, RepinStatus::Changed | RepinStatus::New))
            .map(|piece| piece.id.path())
            .collect();
        for path in touched_files {
            if let Some(tick) = self.viewer.file_read_ticks.remove(path) {
                self.viewer.file_read_before.insert(path.clone(), tick);
            }
        }
        self.last_repin = Some(match self.last_repin.take() {
            Some(earlier) if earlier.to_head == record.from_head => record.following(&earlier),
            _ => record,
        });
        self.viewer.github_baseline_due = true;
    }

    /// The head each file of a basic tour was last read at: its tick, or the tick a re-pin took away.
    pub(super) fn file_read_heads(&self) -> BTreeMap<FilePath, Sha> {
        self.viewer
            .file_read_before
            .iter()
            .chain(&self.viewer.file_read_ticks)
            .map(|(path, tick)| (path.clone(), tick.head.clone()))
            .collect()
    }

    /// The hand-over of a switch to a guided tour. The basic review stayed on screen while the guided
    /// one was built, so what it holds is the reviewer's latest: ticks, viewed marks, queued
    /// comments, hand-offs, typed text and declined moves are all taken from it, and its file reads go
    /// into the steps.
    pub(super) fn take_over_from_basic(&mut self, basic: &Self, carried: &CarriedReads) {
        self.take_viewer_sections(basic);
        for pr in &basic.declined_moves {
            self.decline_move_to(pr);
        }
        self.carry_file_reads_into_steps(carried);
    }

    /// Switching to a guided tour: its steps take the reads the files had, and the file sections and
    /// the re-pin record, which only a basic tour holds, are cleared, as after a Refresh.
    pub(super) fn carry_file_reads_into_steps(&mut self, carried: &CarriedReads) {
        for (step, head) in &carried.ticked {
            self.tick(step, head);
        }
        for (step, head) in &carried.read_before {
            self.viewer
                .read_before
                .insert(step.clone(), ReadTick { head: head.clone() });
        }
        self.viewer.file_read_ticks.clear();
        self.viewer.file_read_before.clear();
        self.last_repin = None;
        self.viewer.github_baseline_due = true;
    }

    /// A Refresh answered what the re-pin left open, so its marks go.
    pub fn finish_refresh(&mut self) {
        self.last_repin = None;
    }

    pub fn last_repin(&self) -> Option<&RepinRecord> {
        self.last_repin.as_ref()
    }

    pub fn read_before(&self, step: &StepId) -> Option<&ReadTick> {
        self.viewer.read_before.get(step)
    }

    pub fn github_baseline_due(&self) -> bool {
        self.viewer.github_baseline_due
    }

    /// GitHub's state replaces what was seen before; only wishes it does not match yet stay queued.
    pub(super) fn take_github_baseline(
        &mut self,
        on_github: &BTreeMap<FilePath, Viewed>,
        wishes: &BTreeMap<FilePath, Viewed>,
    ) {
        self.viewer.viewed_on_github = on_github.clone();
        self.viewer.unsent_viewed = wishes
            .iter()
            .filter(|(path, viewed)| self.was_viewed_on_github(path) != **viewed)
            .map(|(path, viewed)| (path.clone(), *viewed))
            .collect();
        self.viewer.github_baseline_due = false;
    }

    pub fn tick(&mut self, step: &StepId, head: &Sha) {
        self.viewer.read_before.remove(step);
        self.viewer
            .read_ticks
            .insert(step.clone(), ReadTick { head: head.clone() });
    }

    pub fn untick(&mut self, step: &StepId) {
        self.viewer.read_ticks.remove(step);
    }

    pub fn mark_done(&mut self, head: &Sha) {
        self.viewer.done_at = Some(ReadTick { head: head.clone() });
    }

    pub fn is_done_at(&self, head: &Sha) -> bool {
        self.viewer
            .done_at
            .as_ref()
            .is_some_and(|done| done.head == *head)
    }

    pub fn file_read_tick(&self, path: &FilePath) -> Option<&ReadTick> {
        self.viewer.file_read_ticks.get(path)
    }

    pub fn file_read_before(&self, path: &FilePath) -> Option<&ReadTick> {
        self.viewer.file_read_before.get(path)
    }

    pub fn tick_file(&mut self, path: &FilePath, head: &Sha) {
        self.viewer.file_read_before.remove(path);
        self.viewer
            .file_read_ticks
            .insert(path.clone(), ReadTick { head: head.clone() });
    }

    pub fn untick_file(&mut self, path: &FilePath) {
        self.viewer.file_read_ticks.remove(path);
    }

    pub(super) fn is_read(&self, subject: &ReadSubject) -> bool {
        match subject {
            ReadSubject::Step(step) => self.read_tick(step).is_some(),
            ReadSubject::File(path) => self.file_read_tick(path).is_some(),
        }
    }

    pub(super) fn tick_subject(&mut self, subject: &ReadSubject, head: &Sha) {
        match subject {
            ReadSubject::Step(step) => self.tick(step, head),
            ReadSubject::File(path) => self.tick_file(path, head),
        }
    }

    pub(super) fn untick_subject(&mut self, subject: &ReadSubject) {
        match subject {
            ReadSubject::Step(step) => self.untick(step),
            ReadSubject::File(path) => self.untick_file(path),
        }
    }

    pub(super) fn was_viewed_on_github(&self, path: &FilePath) -> Viewed {
        self.viewer
            .viewed_on_github
            .get(path)
            .copied()
            .unwrap_or(Viewed::No)
    }

    pub(super) fn saw_on_github(&mut self, path: &FilePath, viewed: Viewed) {
        self.viewer.viewed_on_github.insert(path.clone(), viewed);
    }

    /// A wish that equals what GitHub already says needs no push, so it cancels the queued one.
    pub(super) fn want_viewed(&mut self, path: &FilePath, viewed: Viewed) {
        if self.was_viewed_on_github(path) == viewed {
            self.viewer.unsent_viewed.remove(path);
        } else {
            self.viewer.unsent_viewed.insert(path.clone(), viewed);
        }
    }

    pub(super) fn unsent_viewed(&self) -> &BTreeMap<FilePath, Viewed> {
        &self.viewer.unsent_viewed
    }

    pub(super) fn sent_viewed(&mut self, path: &FilePath, viewed: Viewed) {
        self.saw_on_github(path, viewed);
        if self.viewer.unsent_viewed.get(path) == Some(&viewed) {
            self.viewer.unsent_viewed.remove(path);
        }
    }

    pub const fn finding_decisions(&self) -> &FindingDecisions {
        &self.viewer.finding_decisions
    }

    pub(super) fn decide_finding(&mut self, finding: Finding, fate: Fate) {
        self.viewer.finding_decisions.record(finding, fate);
    }

    pub fn hand_offs(&self) -> &HandOffs {
        &self.viewer.hand_offs
    }

    pub fn hand_off_replied(
        &mut self,
        id: HandOffId,
        replied: &AgentReplied,
    ) -> Result<(), AgentReplyRefused> {
        self.viewer.hand_offs.replied(id, replied)
    }

    pub(super) fn hand_offs_mut(&mut self) -> &mut HandOffs {
        &mut self.viewer.hand_offs
    }

    /// The file ticks after a re-pin: those that stayed, and those a re-pin took away, each with the head
    /// the file was read at.
    pub(super) fn file_reads_after_repin(
        &self,
    ) -> (BTreeMap<FilePath, Sha>, BTreeMap<FilePath, Sha>) {
        let heads = |ticks: &BTreeMap<FilePath, ReadTick>| {
            ticks
                .iter()
                .map(|(path, tick)| (path.clone(), tick.head.clone()))
                .collect()
        };
        (
            heads(&self.viewer.file_read_ticks),
            heads(&self.viewer.file_read_before),
        )
    }

    /// A guided tour's step reads after a re-pin, as file reads, so they carry by the rule file ticks
    /// carry by. A file is read only when every step showing a piece of it was read; it stays ticked
    /// when none of its pieces changed, and shows what changed since the step's read otherwise.
    pub(super) fn step_reads_as_file_reads_after_repin(
        &self,
        repin: &Repin,
    ) -> (BTreeMap<FilePath, Sha>, BTreeMap<FilePath, Sha>) {
        #[derive(Default)]
        struct FileOfSteps {
            read_at: Option<Sha>,
            has_unread_step: bool,
            has_changed_piece: bool,
        }
        let mut files: BTreeMap<&FilePath, FileOfSteps> = BTreeMap::new();
        for step in &repin.steps {
            let read_at = self
                .read_tick(&step.step)
                .or_else(|| self.read_before(&step.step))
                .map(|tick| tick.head.clone());
            for piece in step
                .pieces
                .iter()
                .filter(|piece| piece.status != RepinStatus::Gone)
            {
                let file = files.entry(piece.id.path()).or_default();
                file.has_unread_step |= read_at.is_none();
                file.has_changed_piece |=
                    matches!(piece.status, RepinStatus::Changed | RepinStatus::New);
                if file.read_at.is_none() {
                    file.read_at.clone_from(&read_at);
                }
            }
        }
        let (mut kept, mut changed) = (BTreeMap::new(), BTreeMap::new());
        for (path, file) in files {
            let Some(read_at) = file.read_at.filter(|_| !file.has_unread_step) else {
                continue;
            };
            let reads = if file.has_changed_piece {
                &mut changed
            } else {
                &mut kept
            };
            reads.insert(path.clone(), read_at);
        }
        (kept, changed)
    }

    /// The hand-over of a local review's move to this PR. The PR's review may have gone on since the
    /// move was worked out, or a first try may have saved already, so a mark the PR's state has is kept
    /// and a draft it has already is never written twice.
    pub(super) fn take_moved_review(&mut self, moved: &MovedReview) {
        for (path, head) in &moved.file_ticks {
            if !self.viewer.file_read_ticks.contains_key(path) {
                self.tick_file(path, head);
            }
        }
        keep_read_before(
            &mut self.viewer.file_read_before,
            &self.viewer.file_read_ticks,
            &moved.file_read_before,
        );
        for (step, head) in &moved.steps.ticked {
            if !self.viewer.read_ticks.contains_key(step) {
                self.tick(step, head);
            }
        }
        keep_read_before(
            &mut self.viewer.read_before,
            &self.viewer.read_ticks,
            moved
                .steps
                .read_before
                .iter()
                .map(|(step, head)| (step, head)),
        );
        for comment in &moved.comments {
            self.take_moved_comment(&comment.place, &comment.body, &moved.head);
        }
    }

    fn take_moved_comment(&mut self, place: &CommentPlace, body: &str, head: &Sha) {
        self.open_draft(place.clone(), Some(head));
        let Some(draft) = self
            .typed
            .comment_drafts
            .iter_mut()
            .find(|d| &d.place == place)
        else {
            return;
        };
        if draft.body.is_empty() {
            body.clone_into(&mut draft.body);
        } else if !draft.body.contains(body) {
            draft.body = format!("{}\n\n{body}", draft.body);
        }
    }

    /// Every head a tick or a read-before mark names.
    pub(super) fn read_heads(&self) -> BTreeSet<Sha> {
        let viewer = &self.viewer;
        viewer
            .read_ticks
            .values()
            .chain(viewer.read_before.values())
            .chain(viewer.file_read_ticks.values())
            .chain(viewer.file_read_before.values())
            .map(|tick| tick.head.clone())
            .collect()
    }

    pub(super) fn read_heads_kept_in(&self) -> Option<&Path> {
        self.read_heads_kept_in.as_deref()
    }

    pub(super) fn keep_read_heads_in(&mut self, clone: &Path) {
        self.read_heads_kept_in = Some(clone.to_path_buf());
    }

    pub(super) fn read_heads_kept_nowhere(&mut self) {
        self.read_heads_kept_in = None;
    }

    pub(super) fn has_declined_move_to(&self, pr: &PullRequestUrl) -> bool {
        self.declined_moves.contains(pr)
    }

    pub(super) fn decline_move_to(&mut self, pr: &PullRequestUrl) {
        if !self.has_declined_move_to(pr) {
            self.declined_moves.push(pr.clone());
        }
    }

    pub(super) fn local_threads(&self) -> &LocalThreads {
        &self.local_threads
    }

    pub(super) fn local_threads_mut(&mut self) -> &mut LocalThreads {
        &mut self.local_threads
    }

    pub(super) fn pr_description(&self) -> &str {
        &self.viewer.pr_description
    }

    pub(super) fn saw_pr_description(&mut self, body: &str) {
        body.clone_into(&mut self.viewer.pr_description);
    }

    /// The Ask desk keeps the Asks and saves them itself, so the viewer's copy holds none.
    pub(super) fn take_asks(&mut self) -> Vec<Ask> {
        std::mem::take(&mut self.asks)
    }

    pub(super) fn take_viewer_sections(&mut self, from: &Self) {
        self.viewer = from.viewer.clone();
        self.typed = from.typed.clone();
    }

    pub(super) fn typed_text(&self) -> TypedText {
        self.typed.clone()
    }

    pub(super) fn take_typed_text(&mut self, typed: TypedText) {
        self.typed = typed;
    }

    pub(super) fn reply_drafts(&self) -> &BTreeMap<ThreadId, String> {
        &self.typed.reply_drafts
    }

    pub(super) fn reply_draft(&self, thread_id: &ThreadId) -> Option<&str> {
        self.typed.reply_drafts.get(thread_id).map(String::as_str)
    }

    pub(super) fn open_reply(&mut self, thread_id: &ThreadId) {
        self.typed
            .reply_drafts
            .entry(thread_id.clone())
            .or_default();
    }

    pub(super) fn write_reply(&mut self, thread_id: &ThreadId, text: &str) {
        if let Some(draft) = self.typed.reply_drafts.get_mut(thread_id) {
            text.clone_into(draft);
        }
    }

    pub(super) fn drop_reply(&mut self, thread_id: &ThreadId) {
        self.typed.reply_drafts.remove(thread_id);
    }

    pub(super) fn finish_body(&self) -> &str {
        &self.typed.finish_body
    }

    pub(super) fn write_finish_body(&mut self, text: &str) {
        text.clone_into(&mut self.typed.finish_body);
    }

    pub(super) fn comment_drafts(&self) -> &[CommentDraft] {
        &self.typed.comment_drafts
    }

    pub(super) fn comment_queue(&self) -> &[QueuedComment] {
        &self.viewer.comment_queue
    }

    pub(super) fn draft_at(&self, place: &CommentPlace) -> Option<&CommentDraft> {
        self.typed
            .comment_drafts
            .iter()
            .find(|draft| &draft.place == place)
    }

    pub(super) fn open_draft(&mut self, place: CommentPlace, head: Option<&Sha>) {
        if self.draft_at(&place).is_none() {
            self.typed.comment_drafts.push(CommentDraft {
                place,
                body: String::new(),
                written_at: head.cloned(),
            });
        }
    }

    pub(super) fn write_draft(&mut self, place: &CommentPlace, body: &str) {
        if let Some(draft) = self
            .typed
            .comment_drafts
            .iter_mut()
            .find(|d| &d.place == place)
        {
            body.clone_into(&mut draft.body);
        }
    }

    pub(super) fn drop_draft(&mut self, place: &CommentPlace) -> Option<CommentDraft> {
        let at = self
            .typed
            .comment_drafts
            .iter()
            .position(|d| &d.place == place)?;
        Some(self.typed.comment_drafts.remove(at))
    }

    pub(super) fn queue_comment(&mut self, queued: QueuedComment) {
        self.viewer.comment_queue.push(queued);
    }

    pub(super) fn queued_at(&mut self, place: &CommentPlace) -> Option<&mut QueuedComment> {
        self.viewer
            .comment_queue
            .iter_mut()
            .find(|queued| &queued.draft.place == place)
    }

    pub(super) fn drop_queued(&mut self, place: &CommentPlace) -> Option<QueuedComment> {
        let at = self
            .viewer
            .comment_queue
            .iter()
            .position(|queued| &queued.draft.place == place)?;
        Some(self.viewer.comment_queue.remove(at))
    }

    pub fn read_tick(&self, step: &StepId) -> Option<&ReadTick> {
        self.viewer.read_ticks.get(step)
    }
}

/// The text boxes of the viewer: what is saved while typing.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TypedText {
    /// Comment text not sent yet; each one is an open comment box.
    #[serde(default)]
    comment_drafts: Vec<CommentDraft>,
    /// Reply text not sent yet, by thread id; each one is an open reply box.
    #[serde(default)]
    reply_drafts: BTreeMap<ThreadId, String>,
    /// The body of the review being finished, not sent yet.
    #[serde(default)]
    finish_body: String,
}

/// `<owner>/<repo>/<pr>.json` in the state files folder
#[derive(Debug, Clone)]
pub struct ReviewStateFile {
    path: PathBuf,
    local_key: Option<String>,
    /// A local review's tour files. With none of them on disk the review was dropped, and its state is
    /// never written again.
    local_tours: Option<[PathBuf; 2]>,
    tour_files: PathBuf,
}

impl ReviewStateFile {
    pub fn for_subject(folders: &AppFolders, subject: &ReviewSubject) -> Self {
        Self {
            local_key: subject.local_key(),
            local_tours: subject.local().map(|_| {
                [
                    Tour::basic_path_for(folders, subject),
                    Tour::path_for(folders, subject),
                ]
            }),
            path: folders
                .state_files()
                .join(subject.files_folder())
                .join(format!("{}.json", subject.file_stem())),
            tour_files: Tour::files_for(folders, subject),
        }
    }

    /// The folder of the PR's tour, whose pictures its summary shows. Named here, beside the state, so
    /// every way of opening a review finds it.
    pub fn tour_files(&self) -> &Path {
        &self.tour_files
    }

    /// The folders any PR of the repo was last opened from, for the PRs the app knows. Unreadable files
    /// and folders count for nothing: this only looks for a clone to work in.
    pub fn folders_opened_from_in(folders: &AppFolders, pr: &PullRequestUrl) -> Vec<PathBuf> {
        Self::folders_opened_from_repo(folders, &pr.repo)
    }

    /// The folders any PR of any repo was last opened from, as far as the state files tell. Unreadable
    /// folders and files are skipped on purpose: this only seeds where to look for clones, a folder
    /// missed here is still found through the configured clone folders and the reviews on disk, and a
    /// state file that cannot be read is raised by the review that owns it when it is opened.
    pub fn folders_opened_from_any_repo(folders: &AppFolders) -> Vec<PathBuf> {
        let subfolders = |parent: &Path| -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(parent)
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .collect();
            names.sort_unstable();
            names
        };
        let state_files = folders.state_files();
        subfolders(&state_files)
            .into_iter()
            .filter(|owner| owner != LOCAL_FILES_FOLDER)
            .flat_map(|owner| {
                subfolders(&state_files.join(&owner))
                    .into_iter()
                    .map(move |name| RepoName::new(&owner, &name))
            })
            .flat_map(|repo| Self::folders_opened_from_repo(folders, &repo))
            .collect()
    }

    fn folders_opened_from_repo(folders: &AppFolders, repo: &RepoName) -> Vec<PathBuf> {
        let Ok(files) = std::fs::read_dir(folders.state_files().join(repo.to_string())) else {
            return Vec::new();
        };
        let mut numbers: Vec<u64> = files
            .filter_map(|file| file.ok()?.path().file_stem()?.to_str()?.parse().ok())
            .collect();
        numbers.sort_unstable();
        numbers
            .into_iter()
            .filter_map(|number| {
                let other = PullRequestUrl {
                    repo: repo.clone(),
                    number,
                };
                Self::for_subject(folders, &ReviewSubject::from(other))
                    .opened_from()
                    .ok()
                    .flatten()
            })
            .collect()
    }

    /// Whether the move of this review to the PR was declined. An unreadable file declines nothing, which
    /// is safe: the offer only shows again, a move reads the same file and fails with its error before it
    /// changes anything, and opening the review raises the unreadable file as a problem of its own.
    pub fn has_declined_move_to(&self, pr: &PullRequestUrl) -> bool {
        self.load()
            .is_ok_and(|state| state.has_declined_move_to(pr))
    }

    /// The one writer of `declined_moves`, which the review's own saves never write, so neither side
    /// of a move writes a stale copy of the other's sections.
    pub(super) fn decline_move_to(&self, pr: &PullRequestUrl) -> anyhow::Result<()> {
        self.update(|state| state.decline_move_to(pr))
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// For a dropped review; a file never written is already gone.
    /// Under the writers' lock, so a save that began before it ends before the file goes.
    pub fn delete(&self) -> anyhow::Result<()> {
        let _lock = self.lock()?;
        remove_if_there(&self.path, std::fs::remove_file(&self.path))
    }

    /// A PR not reviewed before has no file yet: that is an empty state, not an error.
    pub fn load(&self) -> anyhow::Result<ReviewState> {
        match self.read_text()? {
            Some(text) => self
                .parse(&text)
                .and_then(|state| self.of_this_review(state)),
            None => Ok(ReviewState::default()),
        }
    }

    pub fn remember_folder(&self, folder: &Path) -> anyhow::Result<()> {
        self.update(|state| state.opened_from = Some(folder.to_path_buf()))
    }

    /// The folder the PR was last opened from, when it was opened before.
    pub fn opened_from(&self) -> anyhow::Result<Option<PathBuf>> {
        Ok(self.load()?.opened_from)
    }

    /// Opens the app on a file it cannot parse: the file is moved aside, before anything new is
    /// written, and the state starts empty. Says where the file went. When the move fails the
    /// error of `load` is returned, so the reviewer's data is never overwritten.
    pub fn load_or_set_aside(&self) -> anyhow::Result<(ReviewState, Option<PathBuf>)> {
        let Some(text) = self.read_text()? else {
            return Ok((ReviewState::default(), None));
        };
        match self.parse(&text) {
            Ok(state) => Ok((self.of_this_review(state)?, None)),
            Err(not_valid) => {
                let backup = self
                    .set_aside()
                    .map_err(|move_error| not_valid.context(format!("{move_error:#}")))?;
                Ok((ReviewState::default(), Some(backup)))
            }
        }
    }

    fn read_text(&self) -> anyhow::Result<Option<String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                Err(error).with_context(|| format!("could not read {}", self.path.display()))
            }
        }
    }

    fn parse(&self, text: &str) -> anyhow::Result<ReviewState> {
        serde_json::from_str(text)
            .with_context(|| format!("{} is not a valid review state file", self.path.display()))
    }

    /// A file of another review is not this review's to read, nor to set aside.
    fn of_this_review(&self, state: ReviewState) -> anyhow::Result<ReviewState> {
        match (&self.local_key, &state.subject) {
            (Some(wanted), Some(held)) if wanted != held => anyhow::bail!(
                "{} holds the review of {held}, not of {wanted}",
                self.path.display()
            ),
            _ => Ok(state),
        }
    }

    /// A hard link fails when the name is taken, so an earlier backup is never overwritten.
    fn set_aside(&self) -> anyhow::Result<PathBuf> {
        let _lock = self.lock()?;
        let stamp = UtcTimestamp::now();
        let stem = self
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .with_context(|| format!("{} has no name", self.path.display()))?;
        for attempt in 1_u32.. {
            let suffix = if attempt == 1 {
                String::new()
            } else {
                format!("-{attempt}")
            };
            let backup = self
                .path
                .with_file_name(format!("{stem}.broken-{stamp}{suffix}.json"));
            match std::fs::hard_link(&self.path, &backup) {
                Ok(()) => {
                    std::fs::remove_file(&self.path)
                        .with_context(|| format!("could not move {} aside", self.path.display()))?;
                    return Ok(backup);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "could not move {} to {}",
                            self.path.display(),
                            backup.display()
                        )
                    });
                }
            }
        }
        anyhow::bail!("no free backup name for {}", self.path.display())
    }

    /// Reads the file again before changing it: the viewer and the Asks each keep their own
    /// copy in memory, and neither may write the other's section back stale.
    pub fn update(&self, change: impl FnOnce(&mut ReviewState)) -> anyhow::Result<()> {
        self.update_if(|state| {
            change(state);
            true
        })
    }

    /// `update` that writes only when `change` says it changed something.
    pub(super) fn update_if(
        &self,
        change: impl FnOnce(&mut ReviewState) -> bool,
    ) -> anyhow::Result<()> {
        let _lock = self.lock()?;
        let mut state = self.load()?;
        if change(&mut state) {
            self.write(&state)?;
        }
        Ok(())
    }

    pub fn save(&self, state: &ReviewState) -> anyhow::Result<()> {
        let _lock = self.lock()?;
        self.write(state)
    }

    /// One lock file per state file, so writers in this app, in its background work and in a
    /// CLI run at the same time take turns instead of overwriting each other.
    fn lock(&self) -> anyhow::Result<std::fs::File> {
        let folder = self
            .path
            .parent()
            .with_context(|| format!("{} has no folder", self.path.display()))?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        let lock_path = self.path.with_extension("json.lock");
        let lock = std::fs::File::create(&lock_path)
            .with_context(|| format!("could not create {}", lock_path.display()))?;
        lock.lock()
            .with_context(|| format!("could not lock {}", lock_path.display()))?;
        Ok(lock)
    }

    fn unique_temp_path(&self) -> PathBuf {
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let write = WRITES.fetch_add(1, Ordering::Relaxed);
        self.path
            .with_extension(format!("json.{}.{write}.tmp", std::process::id()))
    }

    /// Written to a temp file of its own first, so a crash mid-write never leaves half a file behind.
    /// Drop review deletes the tours before the state file, so a writer that finds no tour never puts
    /// back the state of a review that is gone.
    fn write(&self, state: &ReviewState) -> anyhow::Result<()> {
        if let (Some(key), Some(tours)) = (&self.local_key, &self.local_tours) {
            anyhow::ensure!(
                tours.iter().any(|tour| tour.exists()),
                "the review of {key} was dropped, so nothing of it is saved"
            );
        }
        let mut value = serde_json::to_value(state)?;
        if let (Some(key), Some(object)) = (&self.local_key, value.as_object_mut()) {
            object.insert("subject".into(), key.clone().into());
        }
        let text = serde_json::to_string_pretty(&value)? + "\n";
        let temp = self.unique_temp_path();
        std::fs::write(&temp, text)
            .with_context(|| format!("could not write {}", temp.display()))?;
        std::fs::rename(&temp, &self.path)
            .with_context(|| format!("could not write {}", self.path.display()))
    }
}

impl AskStore for ReviewStateFile {
    fn load(&self) -> anyhow::Result<Vec<Ask>> {
        Ok(Self::load(self)?.asks)
    }

    fn save(&self, asks: &[Ask]) -> anyhow::Result<()> {
        self.update(|state| state.asks = asks.to_vec())
    }
}

/// What a move marks as read before its change, but not where the PR's review has a tick or a mark of
/// its own.
fn keep_read_before<'a, K: Ord + Clone + 'a>(
    marks: &mut BTreeMap<K, ReadTick>,
    ticks: &BTreeMap<K, ReadTick>,
    moved: impl IntoIterator<Item = (&'a K, &'a Sha)>,
) {
    for (key, head) in moved {
        if !ticks.contains_key(key) && !marks.contains_key(key) {
            marks.insert(key.clone(), ReadTick { head: head.clone() });
        }
    }
}
