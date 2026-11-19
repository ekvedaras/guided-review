use std::fmt;

use super::ReviewCore;
use crate::file_path::FilePath;
use crate::findings::{Anchor, AnchorSide, Finding, FindingId};
use crate::line_number::LineNumber;
use crate::viewer::ai_review::AiReview;
use crate::viewer::comment_desk::TourFreshness;
use crate::viewer::comments::{CommentPlace, CommentRefusal, CommentRequest, SendMode};
use crate::viewer::finding_decisions::Fate;
use crate::viewer::finding_threads::WhyAuthor;
use crate::viewer::findings_list::{FindingLine, FindingsList};
#[cfg(feature = "snapshots")]
use crate::viewer::fix_requests::FixRequests;
use crate::viewer::fix_requests::{FixReply, FixState};
use crate::viewer::hand_offs::Audience;
use crate::viewer::step_page::{LineLocation, Side};
use crate::viewer::work_agenda::WorkKind;

/// Why an action on a finding did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingRefusal {
    /// The finding is not among the open ones: it was decided already, or a new run replaced it.
    NotThere,
    /// The decision could not be kept.
    NotKept(String),
    /// A comment box with text is open on those lines: sending the finding would send that text unread.
    DraftInTheWay,
    /// A fix is asked for already, or under way: asking again would send it twice.
    BeingFixed,
    /// A question on it waits for its answer: another would cross it.
    BeingAsked,
    Comment(CommentRefusal),
}

impl fmt::Display for FindingRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotThere => f.write_str("This finding is not open any more"),
            Self::NotKept(reason) => write!(f, "The decision could not be kept: {reason}"),
            Self::DraftInTheWay => {
                f.write_str("A comment box is open on these lines: use the finding there, or send that comment first")
            }
            Self::BeingFixed => f.write_str("A fix of this finding is asked for already"),
            Self::BeingAsked => {
                f.write_str("A question on this finding waits for the review agent's answer")
            }
            Self::Comment(refusal) => refusal.fmt(f),
        }
    }
}

/// What a batch did: what went through, and each finding that did not, with the reason.
#[derive(Debug)]
pub struct FindingsBatch<T> {
    pub done: Vec<T>,
    pub refused: Vec<(FindingId, FindingRefusal)>,
}

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

    /// Kept in the state file, then taken off the screens.
    fn record_decision(&mut self, finding: Finding, fate: Fate) -> Result<(), FindingRefusal> {
        let id = finding.id.clone();
        self.state
            .record(|state| state.decide_finding(finding, fate))
            .map_err(|error| FindingRefusal::NotKept(format!("{error:#}")))?;
        if let AiReview::Findings(file) = &mut self.ai_review {
            file.findings.retain(|open| open.id != id);
        }
        self.place_threads();
        Ok(())
    }

    /// The finding goes, and a later run is told it was dismissed.
    pub fn dismiss_finding(&mut self, id: &FindingId) -> Result<(), FindingRefusal> {
        let finding = self.open_finding(id)?;
        self.record_decision(finding, Fate::Dismissed)
    }

    /// The finding's text goes into a comment box on its lines, after any text there. The finding
    /// stays open until that comment is sent.
    pub fn use_finding(&mut self, id: &FindingId) -> Result<CommentPlace, FindingRefusal> {
        let finding = self.open_finding(id)?;
        let place = self.comment_place_of(&finding.anchor)?;
        self.comment_desk
            .add_text_at(&mut self.state, &place, &self.head, &finding.body)
            .map_err(FindingRefusal::Comment)?;
        self.findings_in_boxes
            .retain(|(_, boxed)| boxed.id != finding.id);
        self.findings_in_boxes.push((place.clone(), finding));
        Ok(place)
    }

    /// The comment sent from `place` closes the findings whose text it carries.
    pub(super) fn decide_findings_sent_from(&mut self, place: &CommentPlace) -> anyhow::Result<()> {
        let (sent, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.findings_in_boxes)
            .into_iter()
            .partition(|(boxed_at, _)| boxed_at == place);
        self.findings_in_boxes = waiting;
        for (_, finding) in sent {
            self.record_decision(finding, Fate::Used)
                .map_err(|refusal| anyhow::anyhow!("{refusal}"))?;
        }
        Ok(())
    }

    /// A box that was discarded carries no finding any more.
    pub(super) fn forget_findings_of_gone_boxes(&mut self) {
        let state = &self.state;
        self.findings_in_boxes
            .retain(|(place, _)| state.draft_at(place).is_some());
    }

    /// The finding, marked as AI, goes straight to the send action, with no box to edit it in. It
    /// goes when the send lands; a failed send keeps it.
    pub fn use_finding_as_is(
        &mut self,
        id: &FindingId,
        mode: SendMode,
    ) -> Result<CommentRequest, FindingRefusal> {
        let finding = self.open_finding(id)?;
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
            return Err(FindingRefusal::DraftInTheWay);
        }
        let text = findings
            .iter()
            .map(|finding| format!("{AI_MARK} {}", finding.body))
            .collect::<Vec<_>>()
            .join("\n\n");
        self.comment_desk
            .add_text_at(&mut self.state, place, &self.head, &text)
            .map_err(FindingRefusal::Comment)?;
        let request = self
            .start_sending(place, mode, Audience::Author)
            .ok_or(FindingRefusal::NotThere)?;
        self.findings_in_boxes.extend(
            findings
                .iter()
                .map(|finding| (place.clone(), finding.clone())),
        );
        Ok(request)
    }

    /// Sends each finding to the tour agent as one message to fix, in reading order.
    pub fn fix_findings(&mut self, ids: &[FindingId]) -> FindingsBatch<FindingId> {
        let mut batch = FindingsBatch {
            done: Vec::new(),
            refused: Vec::new(),
        };
        let mut to_fix = Vec::new();
        for id in ids {
            match self.open_finding(id) {
                Ok(_) if self.state.fix_requests().is_in_progress(id) => {
                    batch.refused.push((id.clone(), FindingRefusal::BeingFixed));
                }
                Ok(finding) => to_fix.push(finding),
                Err(refusal) => batch.refused.push((id.clone(), refusal)),
            }
        }
        to_fix.sort_by_key(|finding| self.reading_order_of(finding));
        batch.done = to_fix.iter().map(|finding| finding.id.clone()).collect();
        if to_fix.is_empty() {
            return batch;
        }
        match self.state.record(|state| {
            for finding in &to_fix {
                state.finding_threads_mut().say(
                    finding.clone(),
                    WhyAuthor::Reviewer,
                    "Fix requested".to_owned(),
                );
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

    /// The tour agent's word on one finding. A finding it fixed is closed.
    pub fn fix_replied(&mut self, id: &FindingId, reply: &FixReply) -> anyhow::Result<()> {
        self.state.record(|state| state.fix_replied(id, reply))??;
        if matches!(reply, FixReply::Fixed { .. })
            && let AiReview::Findings(file) = &mut self.ai_review
        {
            file.findings.retain(|open| open.id != *id);
        }
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

    /// The fixes a snapshot scene shows on the open findings.
    #[cfg(feature = "snapshots")]
    pub fn stage_fix_requests(&mut self) {
        let findings = match &self.ai_review {
            AiReview::Findings(file) => file.findings.clone(),
            _ => Vec::new(),
        };
        self.show_staged_fix_requests(&findings);
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_fix_requests(&mut self) {
        self.show_staged_fix_requests(&[]);
    }

    #[cfg(feature = "snapshots")]
    fn show_staged_fix_requests(&mut self, findings: &[Finding]) {
        self.record_reporting("the staged fix requests", |state| {
            *state.fix_requests_mut() = FixRequests::staged(findings);
        });
        self.place_threads();
    }

    /// Where the fix of a finding is, when it was asked for.
    pub fn fix_state_of(&self, id: &FindingId) -> Option<&FixState> {
        self.state.fix_requests().state_of(id)
    }

    /// The open findings as the findings list shows them.
    pub fn findings_list(&self) -> FindingsList {
        match &self.ai_review {
            AiReview::Findings(file) => FindingsList::of(&file.findings)
                .with_fixes(self.state.fix_requests())
                .with_lines(|finding| self.code_lines_of(finding)),
            AiReview::NotRun | AiReview::Unreadable(_) => FindingsList::default(),
        }
    }

    /// The code lines of a finding's anchor on the screen that shows them; none when no screen does.
    fn code_lines_of(&self, finding: &Finding) -> Vec<FindingLine> {
        let place = self.ask_place_of_finding(finding);
        self.lines_of(&place).map_or_default(|selected| {
            selected
                .lines()
                .iter()
                .map(|line| FindingLine {
                    number: line.location.line.get(),
                    text: line.text.clone(),
                })
                .collect()
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
            let placed = self.open_finding(id).and_then(|finding| {
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

    fn comment_place_of(&self, anchor: &Anchor) -> Result<CommentPlace, FindingRefusal> {
        if self.tour_freshness() == TourFreshness::Behind {
            return Err(FindingRefusal::Comment(CommentRefusal::TourIsOld));
        }
        let at = |file: &FilePath, side: AnchorSide, line: LineNumber| LineLocation {
            path: file.clone(),
            side: match side {
                AnchorSide::Base => Side::Base,
                AnchorSide::Head => Side::Head,
            },
            line,
        };
        match anchor {
            Anchor::File { file } if self.files.iter().any(|changed| changed.path == *file) => {
                Ok(CommentPlace::on_file(file))
            }
            Anchor::Lines { file, side, lines }
                if self.files.iter().any(|changed| changed.path == *file) =>
            {
                Ok(CommentPlace::Lines {
                    start: at(file, *side, lines.start),
                    end: at(file, *side, lines.end),
                })
            }
            Anchor::Change | Anchor::File { .. } | Anchor::Lines { .. } => {
                Err(FindingRefusal::Comment(CommentRefusal::NoLines))
            }
        }
    }
}
