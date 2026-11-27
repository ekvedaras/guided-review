use super::ReviewCore;
use super::finding_outcome::{FindingRefusal, FindingsBatch};
use crate::file_path::FilePath;
use crate::findings::{Anchor, Finding, FindingId};
use crate::viewer::ai_review::AiReview;
use crate::viewer::comment_send::CommentRequest;
use crate::viewer::comments::Audience;
use crate::viewer::comments::{CommentPlace, CommentRefusal, SendMode};
use crate::viewer::file_block::FileBlock;
use crate::viewer::finding_decisions::Fate;
use crate::viewer::finding_placement::FindingHome;
use crate::viewer::finding_threads::WhyAuthor;
use crate::viewer::findings_list::{FindingLine, FindingsList};
use crate::viewer::fix_requests::{FixNote, FixReply, FixState, FixTiming};
use crate::viewer::line_location::{LineLocation, Side};
use crate::viewer::piece_diff::PieceDiff;
use crate::viewer::review_thread::ThreadId;
use crate::viewer::row_selection::RowSelection;
use crate::viewer::screen::Screen;
use crate::viewer::step_page::{FilePage, StepPage};
use crate::viewer::thread_actions::TourFreshness;
use crate::viewer::work_agenda::WorkKind;

/// A finding to fix, with what the reviewer wants of the fix when they said.
type NotedFinding = (Finding, Option<FixNote>);

/// Starts a comment made from a finding the reviewer did not edit.
const AI_MARK: &str = "🤖";

/// Where a finding comes in a batch to fix: by the screen that shows it in reading order, then by its
/// first line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ReadingOrder {
    screen: ScreenOrder,
    first_line: u32,
}

/// A finding no screen shows comes after every screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ScreenOrder {
    At(usize),
    Nowhere,
}

impl ReviewCore {
    pub(super) fn open_finding(&self, id: &FindingId) -> Result<Finding, FindingRefusal> {
        let AiReview::Findings(file) = &self.ai_review else {
            return Err(FindingRefusal::NotThere);
        };
        file.findings
            .iter()
            .find(|finding| finding.id == *id)
            .cloned()
            .ok_or(FindingRefusal::NotThere)
    }

    /// An open finding the reviewer may still use or dismiss: not one the tour agent is fixing.
    fn decidable_finding(&self, id: &FindingId) -> Result<Finding, FindingRefusal> {
        let finding = self.open_finding(id)?;
        if self
            .state
            .fix_requests()
            .state_of(id)
            .is_some_and(FixState::blocks_decisions)
        {
            return Err(FindingRefusal::BeingFixed);
        }
        Ok(finding)
    }

    /// Kept in the state file, then taken off the screens.
    fn record_decision(&mut self, finding: Finding, fate: Fate) -> Result<(), FindingRefusal> {
        let id = finding.id.clone();
        self.state
            .record(|state| state.decide_finding(finding, fate))
            .map_err(|error| FindingRefusal::NotKept(format!("{error:#}")))?;
        self.close_finding(&id);
        Ok(())
    }

    /// The finding goes from the open ones and from the screens.
    pub(super) fn close_finding(&mut self, id: &FindingId) {
        if let AiReview::Findings(file) = &mut self.ai_review {
            file.findings.retain(|open| open.id != *id);
        }
        self.place_threads();
    }

    /// The finding goes, and a later run is told it was dismissed.
    pub fn dismiss_finding(&mut self, id: &FindingId) -> Result<(), FindingRefusal> {
        let finding = self.decidable_finding(id)?;
        self.record_decision(finding, Fate::Dismissed)
    }

    /// The finding's text goes into a comment box on its lines, after any text there. The finding
    /// stays open until that comment is sent.
    pub fn use_finding(&mut self, id: &FindingId) -> Result<CommentPlace, FindingRefusal> {
        let finding = self.decidable_finding(id)?;
        let place = self.comment_place_of(&finding.anchor)?;
        self.comment_desk
            .add_text_at(
                &mut self.state,
                &place,
                &self.head,
                &finding.body,
                std::slice::from_ref(&finding),
            )
            .map_err(FindingRefusal::Comment)?;
        Ok(place)
    }

    /// A sent comment closes the findings whose text it carries, linked to the thread it became.
    pub(super) fn decide_used(
        &mut self,
        findings: Vec<Finding>,
        thread: Option<&ThreadId>,
    ) -> anyhow::Result<()> {
        let ids: Vec<FindingId> = findings.iter().map(|finding| finding.id.clone()).collect();
        if ids.is_empty() {
            return Ok(());
        }
        self.state.record(|state| {
            for finding in findings {
                state.finding_used(finding, thread.cloned());
            }
        })?;
        for id in &ids {
            self.close_finding(id);
        }
        Ok(())
    }

    /// The finding, marked as AI, goes straight to the send action, with no box to edit it in. It
    /// goes when the send lands; a failed send keeps it.
    pub fn use_finding_as_is(
        &mut self,
        id: &FindingId,
        mode: SendMode,
    ) -> Result<CommentRequest, FindingRefusal> {
        let finding = self.decidable_finding(id)?;
        let place = self.comment_place_of(&finding.anchor)?;
        self.send_findings_as_is(&place, &[finding], mode)
    }

    /// One comment on `place` with every finding's text, each marked as AI; each goes when it lands.
    fn send_findings_as_is(
        &mut self,
        place: &CommentPlace,
        findings: &[Finding],
        mode: SendMode,
    ) -> Result<CommentRequest, FindingRefusal> {
        if let Some(refusal) = self.send_refusal(mode) {
            return Err(FindingRefusal::Comment(refusal));
        }
        if self.state.draft_at(place).is_some() {
            return Err(FindingRefusal::DraftInTheWay(place.clone()));
        }
        let text = findings
            .iter()
            .map(|finding| format!("{AI_MARK} {}", finding.body))
            .collect::<Vec<_>>()
            .join("\n\n");
        self.comment_desk
            .add_text_at(&mut self.state, place, &self.head, &text, findings)
            .map_err(FindingRefusal::Comment)?;
        self.start_sending(place, mode, Audience::Author)
            .ok_or(FindingRefusal::NotThere)
    }

    /// Sends each finding to the tour agent as one message to fix, in reading order.
    pub fn fix_findings(&mut self, ids: &[FindingId]) -> FindingsBatch<FindingId> {
        self.request_fixes(ids.iter().map(|id| (id.clone(), None)).collect())
    }

    /// One finding goes to the tour agent, now or with Send fixes, with what the reviewer wants of the fix.
    pub fn fix_with_note(
        &mut self,
        id: &FindingId,
        note: FixNote,
        timing: FixTiming,
    ) -> Result<(), FindingRefusal> {
        let order = vec![(id.clone(), Some(note))];
        let batch = match timing {
            FixTiming::Now => self.request_fixes(order),
            FixTiming::WithSendFixes => self.hold_fixes(order),
        };
        batch
            .refused
            .into_iter()
            .next()
            .map_or(Ok(()), |(_, refusal)| Err(refusal))
    }

    /// The open findings not being fixed, each with its note.
    fn fixable(
        &self,
        orders: Vec<(FindingId, Option<FixNote>)>,
    ) -> (Vec<NotedFinding>, Vec<(FindingId, FindingRefusal)>) {
        let mut fixable = Vec::new();
        let mut refused = Vec::new();
        for (id, note) in orders {
            match self.open_finding(&id) {
                Ok(_) if self.state.fix_requests().is_in_progress(&id) => {
                    refused.push((id, FindingRefusal::BeingFixed));
                }
                Ok(finding) => fixable.push((finding, note)),
                Err(refusal) => refused.push((id, refusal)),
            }
        }
        (fixable, refused)
    }

    fn request_fixes(
        &mut self,
        orders: Vec<(FindingId, Option<FixNote>)>,
    ) -> FindingsBatch<FindingId> {
        let (mut to_fix, refused) = self.fixable(orders);
        let mut batch = FindingsBatch {
            done: Vec::new(),
            refused,
        };
        to_fix.sort_by_key(|(finding, _)| self.reading_order_of(finding));
        batch.done = to_fix
            .iter()
            .map(|(finding, _)| finding.id.clone())
            .collect();
        if to_fix.is_empty() {
            return batch;
        }
        match self.state.record(|state| {
            for (finding, note) in &to_fix {
                let note = note
                    .as_ref()
                    .or_else(|| state.fix_requests().note_to_send(&finding.id));
                let said = note.map_or_else(
                    || "Fix requested".to_owned(),
                    |note| format!("Fix requested: {}", note.as_str()),
                );
                state
                    .finding_threads_mut()
                    .say(finding.clone(), WhyAuthor::Reviewer, said);
            }
            state.fix_requests_mut().request(to_fix);
        }) {
            Ok(()) => {
                self.work.want(WorkKind::DeliverHandOff);
                self.place_threads();
            }
            Err(error) => {
                let reason = format!("{error:#}");
                for id in std::mem::take(&mut batch.done) {
                    batch
                        .refused
                        .push((id, FindingRefusal::NotKept(reason.clone())));
                }
            }
        }
        batch
    }

    /// Each finding waits for Send fixes, so the reviewer goes on reading and sends them together.
    pub fn add_to_fixes(&mut self, ids: &[FindingId]) -> FindingsBatch<FindingId> {
        self.hold_fixes(ids.iter().map(|id| (id.clone(), None)).collect())
    }

    fn hold_fixes(
        &mut self,
        orders: Vec<(FindingId, Option<FixNote>)>,
    ) -> FindingsBatch<FindingId> {
        let (to_add, refused) = self.fixable(orders);
        let mut batch = FindingsBatch {
            done: Vec::new(),
            refused,
        };
        if to_add.is_empty() {
            return batch;
        }
        let added: Vec<FindingId> = to_add
            .iter()
            .map(|(finding, _)| finding.id.clone())
            .collect();
        match self
            .state
            .record(|state| state.fix_requests_mut().add_to_send(to_add))
        {
            Ok(()) => {
                batch.done = added;
                self.place_threads();
            }
            Err(error) => {
                let reason = format!("{error:#}");
                batch.refused.extend(
                    added
                        .into_iter()
                        .map(|id| (id, FindingRefusal::NotKept(reason.clone()))),
                );
            }
        }
        batch
    }

    pub fn remove_from_fixes(&mut self, id: &FindingId) -> Result<(), FindingRefusal> {
        if self.state.fix_requests().state_of(id) != Some(&FixState::ToSend) {
            return Err(FindingRefusal::NotInFixes);
        }
        self.state
            .record(|state| state.fix_requests_mut().remove_to_send(id))
            .map_err(|error| FindingRefusal::NotKept(format!("{error:#}")))?;
        self.place_threads();
        Ok(())
    }

    /// How many findings wait for Send fixes.
    pub fn fixes_to_send(&self) -> usize {
        self.state.fix_requests().to_send().count()
    }

    /// Every finding added to fixes goes to the tour agent as one batch, in reading order.
    pub fn send_fixes(&mut self) -> FindingsBatch<FindingId> {
        let ids: Vec<FindingId> = self.state.fix_requests().to_send().cloned().collect();
        self.fix_findings(&ids)
    }

    /// The tour agent's word on one finding. A finding it fixed is closed.
    pub fn fix_replied(&mut self, id: &FindingId, reply: &FixReply) -> anyhow::Result<()> {
        self.state.record(|state| state.fix_replied(id, reply))??;
        self.place_threads();
        Ok(())
    }

    /// The reviewer accepts the tour agent's fix: the finding closes, and a later run is told it was fixed.
    pub fn mark_fixed(&mut self, id: &FindingId) -> Result<(), FindingRefusal> {
        let finding = self.open_finding(id)?;
        if !matches!(
            self.state.fix_requests().state_of(id),
            Some(FixState::Fixed { .. })
        ) {
            return Err(FindingRefusal::NotFixedYet);
        }
        self.record_decision(finding, Fate::Fixed)
    }

    /// The reviewer's answer to what the tour agent asked before fixing: it goes to the agent as the next message.
    pub fn answer_fix_question(
        &mut self,
        id: &FindingId,
        answer: &str,
    ) -> Result<(), FindingRefusal> {
        if self.state.fix_requests().state_of(id) != Some(&FixState::Asked) {
            return Err(FindingRefusal::NoFixQuestion);
        }
        let kept = self
            .state
            .record(|state| state.fix_question_answered(id, answer))
            .map_err(|error| FindingRefusal::NotKept(format!("{error:#}")))?;
        kept.map_err(|_unanswerable| FindingRefusal::NoFixQuestion)?;
        self.work.want(WorkKind::DeliverHandOff);
        self.place_threads();
        Ok(())
    }

    /// The tour's own order: the screen that shows the finding, then its first line.
    fn reading_order_of(&self, finding: &Finding) -> ReadingOrder {
        let screen_at = self
            .findings
            .home_of(&finding.id)
            .map(|home| self.screen_of_home(home))
            .and_then(|screen| self.reading_position(&screen))
            .map_or(ScreenOrder::Nowhere, ScreenOrder::At);
        let first_line = match &finding.anchor {
            Anchor::Change | Anchor::File { .. } => 0,
            Anchor::Lines { lines, .. } => lines.start.get(),
        };
        ReadingOrder {
            screen: screen_at,
            first_line,
        }
    }

    /// Where the fix of a finding is, when it was asked for.
    pub fn fix_state_of(&self, id: &FindingId) -> Option<&FixState> {
        self.state.fix_requests().state_of(id)
    }

    /// The open findings as the findings list shows them, and the dismissed ones below.
    pub fn findings_list(&self) -> FindingsList {
        let open = match &self.ai_review {
            AiReview::Findings(file) => FindingsList::of(&file.findings)
                .with_fixes(self.state.fix_requests())
                .with_lines(|finding| self.code_lines_of(finding)),
            AiReview::NotRun | AiReview::Unreadable(_) => FindingsList::default(),
        };
        open.with_dismissed(self.dismissed_findings())
    }

    /// What the reviewer dismissed, in the order they did.
    pub(super) fn dismissed_findings(&self) -> Vec<Finding> {
        self.state
            .finding_decisions()
            .dismissed()
            .cloned()
            .collect()
    }

    /// The code lines of a finding's anchor on the screen that shows them; none when no screen does.
    fn code_lines_of(&self, finding: &Finding) -> Vec<FindingLine> {
        let Anchor::Lines { file, side, lines } = &finding.anchor else {
            return Vec::new();
        };
        let screen = match self.findings.home_of(&finding.id) {
            Some(FindingHome::Step(step)) => Screen::Step(step.clone()),
            Some(FindingHome::SafetyNet) => Screen::SafetyNet,
            Some(FindingHome::File(path)) => Screen::File(path.clone()),
            Some(FindingHome::Change) | None => return Vec::new(),
        };
        let Some(pieces) = self.pieces_on(&screen) else {
            return Vec::new();
        };
        let blocks = FileBlock::group(pieces);
        let at = |line| LineLocation {
            path: file.clone(),
            side: Side::from(*side),
            line,
        };
        let mut selection = RowSelection::start_at(at(lines.start));
        selection.extend_to(at(lines.end));
        let selected = selection.resolve(blocks.iter().flat_map(FileBlock::lines));
        selected.map_or_default(|selected| {
            selected
                .numbered_texts()
                .map(|(number, text)| FindingLine {
                    number,
                    text: text.to_owned(),
                })
                .collect()
        })
    }

    /// The pieces the screen shows, as its page draws them; `None` when the tour has no such screen.
    pub(in crate::viewer::review) fn pieces_on(&self, screen: &Screen) -> Option<Vec<PieceDiff>> {
        let links = self.links();
        let display = self.display(&links);
        Some(match screen {
            Screen::Step(step) => {
                let guided = self.tour.guided()?;
                StepPage::of(self.step(step)?, guided, &self.files, &display, false).pieces
            }
            Screen::SafetyNet => self.safety_net()?.pieces,
            Screen::File(_) if self.tour.guided().is_some() => return None,
            Screen::File(path) => {
                let file = self.files.iter().find(|file| file.path == *path)?;
                FilePage::of(file, &self.files, &display).pieces
            }
            Screen::Summary | Screen::Overview => return None,
        })
    }

    /// A dismissed finding is open again, on its lines and in the list, and a later run is no longer
    /// told it was dismissed. It comes from the copy kept with the decision, so it returns even when a
    /// later run's findings file does not hold it any more.
    pub fn restore_finding(&mut self, id: &FindingId) -> Result<(), FindingRefusal> {
        let finding = self
            .state
            .finding_decisions()
            .dismissed()
            .find(|dismissed| dismissed.id == *id)
            .cloned()
            .ok_or(FindingRefusal::NotDismissed)?;
        if self.open_finding(id).is_ok() {
            return Err(FindingRefusal::IdTaken);
        }
        let AiReview::Findings(file) = &mut self.ai_review else {
            return Err(FindingRefusal::NoFindingsFile);
        };
        file.findings.push(finding);
        self.state
            .record(|state| state.restore_finding(id))
            .map_err(|error| FindingRefusal::NotKept(format!("{error:#}")))?;
        self.place_threads();
        Ok(())
    }

    /// Restores each; one that is not dismissed is reported and the rest still come back.
    pub fn restore_findings(&mut self, ids: &[FindingId]) -> FindingsBatch<FindingId> {
        self.batch(ids, |review, id| {
            review.restore_finding(id).map(|()| id.clone())
        })
    }

    /// Dismisses each; one that is not open any more is reported and the rest still go.
    pub fn dismiss_findings(&mut self, ids: &[FindingId]) -> FindingsBatch<FindingId> {
        self.batch(ids, |review, id| {
            review.dismiss_finding(id).map(|()| id.clone())
        })
    }

    /// Each finding as is, one request per place, in the order given: findings on the same lines go
    /// in one comment. A finding with no lines to comment on, or whose lines have a box open, stays
    /// open and is reported.
    pub fn comment_findings(
        &mut self,
        ids: &[FindingId],
        mode: SendMode,
    ) -> FindingsBatch<CommentRequest> {
        let mut batch = FindingsBatch {
            done: Vec::new(),
            refused: Vec::new(),
        };
        let mut by_place: Vec<(CommentPlace, Vec<Finding>)> = Vec::new();
        for id in ids {
            let placed = self.decidable_finding(id).and_then(|finding| {
                let place = self.comment_place_of(&finding.anchor)?;
                Ok((place, finding))
            });
            match placed {
                Ok((place, finding)) => match by_place.iter_mut().find(|(at, _)| *at == place) {
                    Some((_, findings)) => findings.push(finding),
                    None => by_place.push((place, vec![finding])),
                },
                Err(refusal) => batch.refused.push((id.clone(), refusal)),
            }
        }
        for (place, findings) in by_place {
            match self.send_findings_as_is(&place, &findings, mode) {
                Ok(request) => batch.done.push(request),
                Err(refusal) => batch.refused.extend(
                    findings
                        .iter()
                        .map(|finding| (finding.id.clone(), refusal.clone())),
                ),
            }
        }
        batch
    }

    fn batch<T>(
        &mut self,
        ids: &[FindingId],
        mut act: impl FnMut(&mut Self, &FindingId) -> Result<T, FindingRefusal>,
    ) -> FindingsBatch<T> {
        let mut batch = FindingsBatch {
            done: Vec::new(),
            refused: Vec::new(),
        };
        for id in ids {
            match act(self, id) {
                Ok(done) => batch.done.push(done),
                Err(refusal) => batch.refused.push((id.clone(), refusal)),
            }
        }
        batch
    }

    /// What "Use as is" sends with: into the pending review, or into the tasks of a local review.
    pub const fn add_mode(&self) -> SendMode {
        if self.subject.comments_are_agent_tasks() {
            SendMode::AddToTasks
        } else {
            SendMode::AddToReview
        }
    }

    pub(super) fn comment_place_of(&self, anchor: &Anchor) -> Result<CommentPlace, FindingRefusal> {
        if self.tour_freshness() == TourFreshness::Behind {
            return Err(FindingRefusal::Comment(CommentRefusal::TourIsOld));
        }
        let is_changed = |file: &FilePath| self.files.iter().any(|changed| changed.path == *file);
        match anchor {
            Anchor::File { file } | Anchor::Lines { file, .. } if is_changed(file) => {
                CommentPlace::of_finding(anchor)
                    .ok_or(FindingRefusal::Comment(CommentRefusal::NoLines))
            }
            Anchor::Change | Anchor::File { .. } | Anchor::Lines { .. } => {
                Err(FindingRefusal::Comment(CommentRefusal::NoLines))
            }
        }
    }
}
