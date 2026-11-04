use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::pull_request::{PullRequestUrl, Sha};
use crate::repin::{Repin, RepinStatus};

use super::ask::{Ask, AskStore};
use super::comments::{CommentDraft, CommentPlace, QueuedComment};
use super::review::StepId;

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
    #[serde(default)]
    last_repin: Option<RepinRecord>,
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
    viewed_on_github: BTreeMap<String, bool>,
    /// Viewed changes GitHub has not taken yet, newest wish per file.
    #[serde(default)]
    unsent_viewed: BTreeMap<String, bool>,
    /// Ticks a re-pin took away; the step was read at that head, so it can show what changed since.
    #[serde(default)]
    read_before: BTreeMap<StepId, ReadTick>,
    /// After a re-pin GitHub's viewed state is taken as it is, so its "not viewed" for files
    /// the new commits touched is not read as the reviewer unticking.
    #[serde(default)]
    github_baseline_due: bool,
    /// Comments GitHub did not take, oldest first.
    #[serde(default)]
    comment_queue: Vec<QueuedComment>,
    /// The PR's body at the last pull, so the summary shows it before GitHub answers.
    #[serde(default)]
    pr_description: String,
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
    pub pieces: BTreeMap<String, RepinStatus>,
    /// By old id: a gone hunk's ordinal may belong to another hunk now.
    pub gone_by_step: BTreeMap<StepId, BTreeSet<String>>,
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
                .map(|piece| (piece.id.to_string(), piece.status))
                .collect(),
            gone_by_step: repin
                .steps
                .iter()
                .map(|step| {
                    let gone = step
                        .pieces
                        .iter()
                        .filter(|piece| piece.status == RepinStatus::Gone)
                        .map(|piece| piece.id.to_string())
                        .collect();
                    (StepId::from(step.step.as_str()), gone)
                })
                .filter(|(_, gone): &(StepId, BTreeSet<String>)| !gone.is_empty())
                .collect(),
        }
    }

    pub fn status_of(&self, piece: &str) -> Option<RepinStatus> {
        self.pieces.get(piece).copied()
    }

    pub fn gone_count(&self, step: &StepId) -> usize {
        self.gone_by_step.get(step).map_or(0, BTreeSet::len)
    }

    /// Two re-pins with no Refresh between read as one, from where the first one started:
    /// what changed in the first stays changed even when the second leaves it alone.
    fn following(self, earlier: &Self) -> Self {
        let was_new_then = |id: &String| earlier.pieces.get(id) == Some(&RepinStatus::New);
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

impl ReviewState {
    /// Steps whose pieces changed or gained new ones lose their tick; it is kept as `read_before`.
    pub fn record_repin(&mut self, record: RepinRecord, repin: &Repin) {
        for step in repin
            .steps
            .iter()
            .filter(|step| step.has_changed_or_new_piece())
        {
            let id = StepId::from(step.step.as_str());
            if let Some(tick) = self.viewer.read_ticks.remove(&id) {
                self.viewer.read_before.insert(id, tick);
            }
        }
        self.last_repin = Some(match self.last_repin.take() {
            Some(earlier) if earlier.to_head == record.from_head => record.following(&earlier),
            _ => record,
        });
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
        on_github: &BTreeMap<String, bool>,
        wishes: &BTreeMap<String, bool>,
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

    pub(super) fn was_viewed_on_github(&self, path: &str) -> bool {
        self.viewer
            .viewed_on_github
            .get(path)
            .copied()
            .unwrap_or(false)
    }

    pub(super) fn saw_on_github(&mut self, path: &str, viewed: bool) {
        self.viewer.viewed_on_github.insert(path.to_owned(), viewed);
    }

    /// A wish that equals what GitHub already says needs no push, so it cancels the queued one.
    pub(super) fn want_viewed(&mut self, path: &str, viewed: bool) {
        if self.was_viewed_on_github(path) == viewed {
            self.viewer.unsent_viewed.remove(path);
        } else {
            self.viewer.unsent_viewed.insert(path.to_owned(), viewed);
        }
    }

    pub(super) fn unsent_viewed(&self) -> &BTreeMap<String, bool> {
        &self.viewer.unsent_viewed
    }

    pub(super) fn sent_viewed(&mut self, path: &str, viewed: bool) {
        self.saw_on_github(path, viewed);
        if self.viewer.unsent_viewed.get(path) == Some(&viewed) {
            self.viewer.unsent_viewed.remove(path);
        }
    }

    pub(super) fn pr_description(&self) -> &str {
        &self.viewer.pr_description
    }

    /// Whether it differs from the one kept.
    pub(super) fn saw_pr_description(&mut self, body: &str) -> bool {
        let is_new = self.viewer.pr_description != body;
        if is_new {
            body.clone_into(&mut self.viewer.pr_description);
        }
        is_new
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

    pub(super) fn reply_drafts(&self) -> &BTreeMap<String, String> {
        &self.typed.reply_drafts
    }

    pub(super) fn reply_draft(&self, thread_id: &str) -> Option<&str> {
        self.typed.reply_drafts.get(thread_id).map(String::as_str)
    }

    pub(super) fn open_reply(&mut self, thread_id: &str) {
        self.typed
            .reply_drafts
            .entry(thread_id.to_owned())
            .or_default();
    }

    pub(super) fn write_reply(&mut self, thread_id: &str, text: &str) {
        if let Some(draft) = self.typed.reply_drafts.get_mut(thread_id) {
            text.clone_into(draft);
        }
    }

    pub(super) fn drop_reply(&mut self, thread_id: &str) {
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

    pub(super) fn open_draft(&mut self, place: CommentPlace, head: &Sha) {
        if self.draft_at(&place).is_none() {
            self.typed.comment_drafts.push(CommentDraft {
                place,
                body: String::new(),
                written_at: head.clone(),
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
    reply_drafts: BTreeMap<String, String>,
    /// The body of the review being finished, not sent yet.
    #[serde(default)]
    finish_body: String,
}

/// `~/.guided-review/state/<owner>/<repo>/<pr>.json`
#[derive(Clone)]
pub struct ReviewStateFile {
    path: PathBuf,
}

impl ReviewStateFile {
    pub fn for_pr(home: &Path, pr: &PullRequestUrl) -> Self {
        Self {
            path: home
                .join(".guided-review/state")
                .join(pr.repo.to_string())
                .join(format!("{}.json", pr.number)),
        }
    }

    /// A PR not reviewed before has no file yet: that is an empty state, not an error.
    pub fn load(&self) -> anyhow::Result<ReviewState> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| {
                format!("{} is not a valid review state file", self.path.display())
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ReviewState::default())
            }
            Err(error) => {
                Err(error).with_context(|| format!("could not read {}", self.path.display()))
            }
        }
    }

    /// Reads the file again before changing it: the viewer and the Asks each keep their own
    /// copy in memory, and neither may write the other's section back stale.
    pub fn update(&self, change: impl FnOnce(&mut ReviewState)) -> anyhow::Result<()> {
        let _lock = self.lock()?;
        let mut state = self.load()?;
        change(&mut state);
        self.write(&state)
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
    fn write(&self, state: &ReviewState) -> anyhow::Result<()> {
        let text = serde_json::to_string_pretty(state)? + "\n";
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
