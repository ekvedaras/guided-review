//! What the review knows of its PR on GitHub: the other side of its branch, the route and the related
//! issues, the description's notes, the reviews, the reviewing mark and the finish.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::ReviewCore;
use crate::coverage;
use crate::issue::IssueLink;
use crate::tour::Tour;
use crate::viewer::branch_link::{BranchOffer, LocalReviewsOfRepo};
use crate::viewer::finish_review::{
    ApproveWarning, FinishOutcome, FinishRefusal, FinishRequest, FinishResult,
};
use crate::viewer::hidden_text::HiddenText;
use crate::viewer::host_outage::Affected;
use crate::viewer::host_outage::Read;
use crate::viewer::markdown::Markdown;
use crate::viewer::ports::ReviewVerdict;
use crate::viewer::ports::{HostError, PullOfBranch};
use crate::viewer::pr_notes::{Note, NoteSubject, NotesButton, ToNotes, ToNotesOutcome};
#[cfg(feature = "snapshots")]
use crate::viewer::pr_origin::PullRequestOrigin;
use crate::viewer::pr_origin::{HeadBranch, MergeRoute, PrAuthor};
use crate::viewer::pr_reviewers::SubmittedReview;
use crate::viewer::pr_reviewers::{PullRequestReviews, ReviewerLine, StanceAge, YourReview};
use crate::viewer::problems::{Problem, ProblemKind};
use crate::viewer::reactions::ReactionChange;
use crate::viewer::related_issues::{IssueDetails, RelatedIssue};
use crate::viewer::replacing_jobs::ReplacingJob;
use crate::viewer::reviewing_mark::ReviewingMarkRemoval;
use crate::viewer::work_agenda::WorkKind;

impl ReviewCore {
    /// What GitHub said of the PR opened from a local review's branch.
    pub fn pull_of_branch_is(&mut self, found: Option<PullOfBranch>) {
        self.branch_link.pull_of_branch_is(found);
    }

    /// The local reviews on disk that are of this PR's repo, so the review looks for one of the PR's branch.
    /// A PR's head branch never changes, so it is looked up once here, never again on focus.
    pub(in crate::viewer) fn local_reviews_of_repo_are(&mut self, reviews: LocalReviewsOfRepo) {
        self.branch_link.local_reviews_are(reviews);
        self.work.want(WorkKind::LookUpHeadBranch);
    }

    /// The lasting line of the sidebar, or Open PR, from what is known of the other side of the branch.
    pub fn branch_offer(&self) -> Option<BranchOffer> {
        self.branch_link.offer(&self.state)
    }

    /// Not now, on either side's offer, unless the move runs. A decline that could not be written to
    /// the state file is a problem, as the offer then comes back at the next open.
    pub fn decline_the_move(&mut self) {
        if self.jobs.is_running(ReplacingJob::MoveToPr) {
            return;
        }
        let declined = self.branch_link.decline(&mut self.state);
        match declined {
            Ok(()) => self.clear_problem(ProblemKind::MoveNotDeclined),
            Err(error) => self.raise_problem(Problem::MoveNotDeclined(format!("{error:#}"))),
        }
    }

    /// What GitHub last said of the issue; `None` for a ticket and until it answers.
    pub fn issue_details(&self, link: &IssueLink) -> Option<Arc<IssueDetails>> {
        self.related_issues.details_of(link)
    }

    /// The issues the review is about, best source first; empty when none names one.
    pub fn related_issues(&self) -> Arc<[RelatedIssue]> {
        self.related_issues.shown()
    }

    /// The branch the review's change goes into and the one it comes from: a PR's as GitHub last said,
    /// a local review's from git (its branch and the repo's default branch). `None` until either is known.
    pub fn merge_route(&self) -> Option<MergeRoute> {
        match &self.local_facts {
            Some(facts) => Some(MergeRoute {
                base: facts.default_branch.without_remote(),
                head: HeadBranch::of_repo(facts.branch.clone()),
            }),
            None => self.pr_origin.as_ref().map(|origin| origin.route.clone()),
        }
    }

    /// Who opened the PR; `None` for a local review, for a PR GitHub has not answered for yet, and for
    /// one whose author's account is gone.
    pub fn pr_author(&self) -> Option<&PrAuthor> {
        self.pr_origin.as_ref()?.author.as_ref()
    }

    /// What the author and route chip says when hovered: "ada wants to merge feat/x into main".
    pub fn merge_route_sentence(&self) -> Option<String> {
        let MergeRoute { base, head } = self.merge_route()?;
        Some(match (&self.local_facts, self.pr_author()) {
            (Some(_), _) => format!("Reviewing {head} against {base}"),
            (None, Some(author)) => format!("{} wants to merge {head} into {base}", author.login),
            (None, None) => format!("Merges {head} into {base}"),
        })
    }

    /// The author and branches a snapshot scene shows, as if GitHub said them of the PR.
    #[cfg(feature = "snapshots")]
    pub fn stage_pr_origin(&mut self, origin: Option<PullRequestOrigin>) {
        self.pr_origin = origin;
    }

    /// The issues a snapshot scene shows, as if GitHub linked them to the PR.
    #[cfg(feature = "snapshots")]
    pub fn stage_related_issues(&mut self, issues: Vec<(RelatedIssue, Option<IssueDetails>)>) {
        self.related_issues.staged(issues);
    }

    #[cfg(feature = "snapshots")]
    pub fn unstage_related_issues(&mut self) {
        self.related_issues.unstaged();
    }

    /// Whether the review has a PR page to open.
    pub fn can_open_on_github(&self) -> bool {
        self.subject.has_host_page()
    }

    /// A local review whose branch has no open PR yet, so its agent can be asked to open one.
    pub fn can_open_pr(&self) -> bool {
        self.branch_link.can_open_pr()
    }

    /// The PR's body, under the summary: the one GitHub gave last, until it answers again.
    /// `None` for a PR with no description.
    pub fn pr_description(&self) -> Option<Markdown> {
        let body = self.state.pr_description();
        (!body.trim().is_empty()).then(|| Markdown::parse(body).nested_one_level())
    }

    /// What the PR's description hides from its reader; `None` when it hides nothing.
    pub fn pr_description_hidden_text(&self) -> Option<HiddenText> {
        HiddenText::hiding(self.state.pr_description())
    }

    /// `None` when the tour has no such diagram, picture or summary, or the review has no PR.
    pub fn notes_button(&self, subject: &NoteSubject) -> Option<NotesButton> {
        self.subject.pull_request()?;
        let note = self.note(subject)?;
        Some(
            self.pr_notes
                .button(subject, self.state.pr_description(), &note),
        )
    }

    /// `None` while the subject is being added, or when the description holds it.
    pub fn start_adding_to_notes(&mut self, subject: &NoteSubject) -> Option<ToNotes> {
        let note = self.note(subject)?;
        if note.is_in(self.state.pr_description()) {
            return None;
        }
        self.pr_notes
            .start(self.subject.pull_request()?, subject, note)
    }

    pub fn apply_to_notes(&mut self, outcome: ToNotesOutcome) -> anyhow::Result<()> {
        match self.pr_notes.finish(outcome) {
            Some(description) => self.set_pr_description(&description),
            None => Ok(()),
        }
    }

    fn note(&self, subject: &NoteSubject) -> Option<Note> {
        match subject {
            NoteSubject::StepDiagram(step) => {
                let found = self.step(step)?;
                Some(Note::diagram(&found.title, found.diagram.as_deref()?))
            }
            NoteSubject::SummaryPicture(path) => {
                let summary = self.reviewer_summary()?;
                let (_, alt) = summary
                    .pictures()
                    .into_iter()
                    .find(|(picture, _)| picture == path)?;
                Note::picture(path, alt)
            }
            NoteSubject::ReviewerSummary => {
                let summary = self.reviewer_summary_text();
                if summary.trim().is_empty() {
                    return None;
                }
                Note::reviewer_summary(summary, &self.tour_files)
            }
        }
    }

    pub fn pending_review_is(&mut self, exists: bool) {
        self.clear_problem(ProblemKind::NotRead(Read::PendingReview));
        self.host_answered(Affected::PendingReview);
        self.comment_desk.pending_review_is(exists);
    }

    pub fn reviews_are(&mut self, reviews: PullRequestReviews) {
        self.clear_problem(ProblemKind::NotRead(Read::LastReview));
        self.host_answered(Affected::LastReview);
        self.reviewing_mark
            .last_review_is(reviews.viewer_last.as_ref());
        self.last_review = reviews.viewer_last;
        self.standings = reviews.standings;
        self.want_reviewing_mark_sent();
    }

    /// Where the viewer stands: by GitHub's standing, else by the last review sent. `None` when the
    /// viewer neither reviewed nor is asked to.
    pub fn your_review(&self) -> Option<YourReview> {
        let age = if self.last_review_is_old() {
            StanceAge::BeforeNewCommits
        } else {
            StanceAge::Current
        };
        YourReview::of(
            self.standings.iter().find(|standing| standing.is_viewer()),
            self.last_review.as_ref(),
            age,
            self.is_marked_done(),
        )
    }

    /// The sidebar's reviews: the viewer first, then what blocks the PR, then what lets it through.
    pub fn reviewer_lines(&self) -> Vec<ReviewerLine> {
        self.standings
            .iter()
            .map(|standing| standing.line(self.pr_head.as_ref()))
            .collect()
    }

    fn want_reviewing_mark_sent(&mut self) {
        if self.reviewing_mark.change_to_send().is_some() {
            self.work.want(WorkKind::ChangeReviewingMark);
        }
    }

    pub(super) fn reviewing_mark_landed(
        &mut self,
        change: ReactionChange,
        result: Result<(), HostError>,
    ) {
        match result {
            Ok(()) => {
                self.clear_problem(ProblemKind::ReviewingMarkNotChanged);
                self.reviewing_mark.sent(change);
            }
            Err(error) => {
                self.raise_problem(Problem::ReviewingMarkNotChanged(error.short_reason()));
            }
        }
    }

    /// The review is closing: the reviewing mark comes off, if it is on, by the job returned. It is
    /// run at once, since nothing is left to run it later.
    pub fn close(&mut self) -> Option<ReviewingMarkRemoval> {
        let may_be_on = self.reviewing_mark.close();
        let pr = self.subject.pull_request()?;
        may_be_on.then(|| ReviewingMarkRemoval::of(pr.clone()))
    }

    /// A re-review can end with nothing to send: the reviewer's last review saw an older commit, and
    /// no pending review waits to go.
    pub fn can_mark_done(&self) -> bool {
        self.subject.pull_request().is_some()
            && self
                .last_review
                .as_ref()
                .is_some_and(|last| last.commit != self.head)
            && self.comment_desk.can_comment_now()
            && !self.is_marked_done()
    }

    /// Done with this head without a new review on GitHub: saved, and the 👀 comes off.
    pub fn mark_done(&mut self) -> anyhow::Result<()> {
        let head = self.head.clone();
        let saved = self.state.change(|state| state.mark_done(&head));
        self.reviewing_mark.done();
        self.want_reviewing_mark_sent();
        saved
    }

    pub fn is_marked_done(&self) -> bool {
        self.state.is_done_at(&self.head)
    }

    /// The viewer's latest sent review, from this app or the GitHub web page.
    pub fn last_review(&self) -> Option<&SubmittedReview> {
        self.last_review.as_ref()
    }

    pub fn has_last_review(&self) -> bool {
        self.last_review.is_some()
    }

    /// The PR has commits the last review did not see. Not so while the PR's head is unknown.
    pub fn last_review_is_old(&self) -> bool {
        match (&self.last_review, &self.pr_head) {
            (Some(review), Some(pr_head)) => review.commit != *pr_head,
            _ => false,
        }
    }

    pub fn viewer_authorship_is(&mut self, authored: bool) {
        self.clear_problem(ProblemKind::NotRead(Read::Authorship));
        self.host_answered(Affected::Authorship);
        self.comment_desk.viewer_authorship_is(authored);
        self.reviewing_mark.viewer_authorship_is(authored);
        self.want_reviewing_mark_sent();
    }

    /// GitHub refuses both on the viewer's own PR, so the buttons show only when the PR is surely someone else's.
    /// A local review gives no verdict.
    pub fn can_approve_or_request_changes(&self) -> bool {
        self.finishes_with_send_tasks().is_none()
            && self.comment_desk.can_approve_or_request_changes()
    }

    /// On the viewer's own PR there is no verdict to give, so finishing only makes sense with comments
    /// to send; a local review's finish sends its queued tasks, so it needs some.
    pub fn can_finish_review(&self) -> bool {
        match self.finishes_with_send_tasks() {
            Some(queued) => queued > 0,
            None => self.comment_desk.can_finish_review(),
        }
    }

    /// A local review finishes with Send tasks, never with a review on GitHub: how many tasks it
    /// would send. `None` for a PR.
    pub fn finishes_with_send_tasks(&self) -> Option<usize> {
        self.subject
            .comments_are_agent_tasks()
            .then(|| self.queued_tasks())
    }

    /// What the reviewer has not looked at: steps with no read tick and the pieces of the safety net.
    pub fn approve_warning(&self) -> Option<ApproveWarning> {
        if self.finishes_with_send_tasks().is_some() {
            return None;
        }
        let progress = self.progress();
        let unread_safety_net_pieces = self.tour.guided().map_or(0, |guided| {
            coverage::safety_net(guided, &self.files)
                .iter()
                .map(|part| part.piece.id.to_string())
                .collect::<BTreeSet<_>>()
                .len()
        });
        let unread = progress.total - progress.read;
        let warning = match &self.tour {
            Tour::Guided(_) => ApproveWarning {
                unread_steps: unread,
                unread_files: 0,
                unread_safety_net_pieces,
            },
            Tour::Basic(_) => ApproveWarning {
                unread_steps: 0,
                unread_files: unread,
                unread_safety_net_pieces,
            },
        };
        (warning.unread_steps + warning.unread_files + warning.unread_safety_net_pieces > 0)
            .then_some(warning)
    }

    pub fn finish_review_request(
        &self,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<FinishRequest, FinishRefusal> {
        let Some(pr) = self.subject.pull_request() else {
            return Err(FinishRefusal::NoPullRequest);
        };
        self.comment_desk.finish_review_request(pr, verdict, body)
    }

    /// A sent review is read back, so the finish button shows it.
    pub fn apply_finish(&mut self, outcome: FinishOutcome) -> FinishResult {
        let result = self.comment_desk.apply_finish(&mut self.state, outcome);
        if result == FinishResult::Sent {
            self.work.want(WorkKind::PullReviews);
        }
        result
    }
}
