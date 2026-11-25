//! The threads on the page and the comment boxes: selecting lines, typing, the queue, landing a sent
//! comment and the hand-offs to an agent.

use std::collections::BTreeMap;

use super::ReviewCore;
use crate::file_path::FilePath;
use crate::viewer::ask_context;
use crate::viewer::comment_desk::{CommentBox, CommentDesk, TextBox};
use crate::viewer::comment_send::{CommentOutcome, CommentRequest};
use crate::viewer::comments::Audience;
use crate::viewer::comments::{
    CommentPlace, CommentProblem, CommentRefusal, CommentResult, QueuedComment, SendMode,
};
use crate::viewer::comments_list::{CommentsList, ListedThread, ThreadVisit};
use crate::viewer::hand_offs::{HandOff, HandOffState, HandOffs, HandToAgentRefused, SentFrom};
use crate::viewer::host_outage::Affected;
use crate::viewer::line_location::LineLocation;
use crate::viewer::outdated_lines::FollowedLines;
use crate::viewer::page::Page;
use crate::viewer::ports::HostError;
use crate::viewer::problems::{Problem, ProblemKind};
use crate::viewer::review_thread::{CommentId, ReviewThread, ThreadId};
use crate::viewer::row_selection::RowSelection;
use crate::viewer::screen::Screen;
use crate::viewer::thread_actions::{
    ThreadAction, ThreadActionOutcome, ThreadActionRequest, ThreadActionResult,
};
use crate::viewer::threads::AgentOfThreads;
use crate::viewer::typing_save::TypingSave;
use crate::viewer::work_agenda::WorkKind;

impl ReviewCore {
    pub fn set_threads(&mut self, threads: &[ReviewThread]) {
        self.clear_problem(ProblemKind::ThreadsNotRead);
        self.host_answered(Affected::Comments);
        self.pulled_threads = threads.to_vec();
        #[cfg(feature = "snapshots")]
        self.pulled_threads
            .extend(self.staged_threads.iter().cloned());
        self.place_threads();
    }

    /// Outdated threads hang under the lines they were followed to; the others stay after the code.
    pub fn set_followed_lines(&mut self, followed: BTreeMap<ThreadId, FollowedLines>) {
        self.followed_lines = followed;
        self.place_threads();
    }

    /// The threads shown stay as they were, named as possibly old until a read goes through.
    pub fn threads_not_read(&mut self, error: &HostError) {
        if let HostError::NotAnswering(host, cause) = error {
            self.clear_problem(ProblemKind::ThreadsNotRead);
            self.host_not_answering(*host, *cause, Affected::Comments);
        } else {
            self.host_answered(Affected::Comments);
            self.raise_problem(Problem::ThreadsNotRead(error.short_reason()));
        }
    }

    pub fn has_threads(&self) -> bool {
        !self.pulled_threads.is_empty()
    }

    /// Every thread of the review, in reading order, each with the screen that shows it.
    pub fn comments_list(&self) -> CommentsList {
        let mut listed: Vec<ListedThread> = Vec::new();
        let mut steps_before = 0;
        for screen in self.reading_order() {
            if matches!(screen, Screen::Step(_)) {
                steps_before += 1;
            }
            let title = self.screen_heading(&screen, steps_before);
            listed.extend(
                self.threads
                    .on_screen(&screen)
                    .threads()
                    .iter()
                    .map(|thread| ListedThread {
                        thread: thread.clone(),
                        shown_on: Some((screen.clone(), title.clone())),
                    }),
            );
        }
        let unshown: Vec<ListedThread> = self
            .pulled_threads
            .iter()
            .filter(|thread| !listed.iter().any(|listed| listed.thread.id == thread.id))
            .map(|thread| ListedThread {
                thread: thread.clone(),
                shown_on: None,
            })
            .collect();
        listed.extend(unshown);
        CommentsList::of(listed)
    }

    /// Goes to the screen that shows the thread, opening its step and the thread when folded.
    pub fn go_to_thread(&mut self, id: &ThreadId) -> ThreadVisit {
        let Some(thread) = self.pulled_threads.iter().find(|thread| thread.id == *id) else {
            return ThreadVisit::Nowhere;
        };
        let is_folded = thread.is_folded_by_default() != self.thread_fold_flipped.contains(id);
        let link = thread
            .comments
            .first()
            .map(|comment| comment.url.clone())
            .filter(|url| !url.is_empty());
        let shown_on = self.reading_order().into_iter().find(|screen| {
            self.threads
                .on_screen(screen)
                .threads()
                .iter()
                .any(|placed| placed.id == *id)
        });
        let Some(screen) = shown_on else {
            return link.map_or(ThreadVisit::Nowhere, ThreadVisit::OnHost);
        };
        self.go_to_screen(screen);
        if let Screen::Step(step) = self.screen.clone()
            && self.is_folded(&step)
        {
            self.toggle_fold(&step);
        }
        if is_folded {
            self.toggle_thread_fold(id);
        }
        ThreadVisit::OnScreen
    }

    /// "Step 3 · Title" with the number the page's "Step 3 of 8" gives, which counts steps in reading order.
    fn screen_heading(&self, screen: &Screen, step_number: usize) -> String {
        match screen {
            Screen::Step(id) => {
                let title = self
                    .tour
                    .guided()
                    .and_then(|guided| guided.steps().into_iter().find(|step| step.id == *id))
                    .map_or_default(|step| step.title.clone());
                format!("Step {step_number} · {title}")
            }
            Screen::File(path) => path.to_string(),
            Screen::SafetyNet => "Safety net".to_owned(),
            Screen::Summary => "Summary".to_owned(),
            Screen::Overview => "Overview".to_owned(),
        }
    }

    pub fn toggle_thread_fold(&mut self, thread_id: &ThreadId) {
        if !self.thread_fold_flipped.remove(thread_id) {
            self.thread_fold_flipped.insert(thread_id.clone());
        }
    }

    pub fn can_comment_now(&self) -> bool {
        self.comment_desk.can_comment_now()
    }

    /// Whether a comment shows the reactions to add, as the PR's host has them.
    pub fn can_react_to_comments(&self) -> bool {
        self.comment_desk.can_react()
    }

    /// A reply in a local thread is a follow-up task, so it always reaches the agent too.
    pub fn thread_action_request(&self, action: ThreadAction) -> ThreadActionRequest {
        let (action, reply_body, send_refusal) = match action {
            ThreadAction::Reply {
                thread_id,
                mode,
                audience,
            } => {
                let reply_body = self.state.reply_draft(&thread_id).map(str::to_owned);
                let audience = if mode.is_task() {
                    Audience::Agent
                } else {
                    audience
                };
                let action = ThreadAction::Reply {
                    thread_id,
                    mode,
                    audience,
                };
                (action, reply_body, self.send_refusal(mode))
            }
            other => (other, None, None),
        };
        let mut request = self.comment_desk.thread_action_request(
            (self.thread_home(), &self.head),
            &self.pulled_threads,
            action,
            reply_body,
            self.tour_freshness(),
        );
        if send_refusal.is_some() {
            request.refusal = send_refusal;
        }
        request
    }

    /// Asks the reviewer to confirm deleting one of their own comments, which a deletion needs.
    pub fn ask_to_delete(&mut self, comment_id: &CommentId) {
        self.comment_desk
            .ask_to_delete(&self.pulled_threads, comment_id);
    }

    /// The reviewer said no, or looked away.
    pub fn withdraw_delete(&mut self) {
        self.comment_desk.withdraw_delete();
    }

    /// The text in the box as typed so far; `None` while it is not open.
    pub fn text_of(&self, text_box: &TextBox) -> Option<&str> {
        self.comment_desk.text_of(&self.state, text_box)
    }

    /// Opens a reply box, or the edit of one of the viewer's own comments on its current text;
    /// nothing opens for a comment someone else wrote.
    pub fn open_box(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk
            .open(&mut self.state, &self.pulled_threads, text_box)
    }

    /// For every key: changes the text in memory only. `typing_save` takes it to disk, off the UI thread.
    pub fn type_in(&mut self, text_box: &TextBox, text: &str) -> anyhow::Result<()> {
        self.comment_desk.type_in(&mut self.state, text_box, text)
    }

    /// The reviewer leaves the box (Escape): a box with nothing written goes, a written one stays.
    pub fn leave(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk.leave(&mut self.state, text_box)
    }

    pub fn discard(&mut self, text_box: &TextBox) -> anyhow::Result<()> {
        self.comment_desk.discard(&mut self.state, text_box)
    }

    /// Whatever came of it, the next hand-off may go now.
    pub fn apply_thread_action(&mut self, outcome: ThreadActionOutcome) -> ThreadActionResult {
        self.work.want(WorkKind::DeliverHandOff);
        self.comment_desk
            .apply_thread_action(&mut self.state, &outcome, sent_from(&self.screen));
        self.show_pulled_threads(outcome.threads.as_ref());
        if outcome.result == ThreadActionResult::Done {
            match &outcome.action {
                ThreadAction::Resolve { thread_id } => self.fold_thread(thread_id, true),
                ThreadAction::Unresolve { thread_id } => self.fold_thread(thread_id, false),
                _ => {}
            }
        }
        outcome.result
    }

    fn fold_thread(&mut self, thread_id: &ThreadId, folded: bool) {
        let Some(thread) = self
            .pulled_threads
            .iter()
            .find(|thread| &thread.id == thread_id)
        else {
            return;
        };
        if thread.is_folded_by_default() == folded {
            self.thread_fold_flipped.remove(thread_id);
        } else {
            self.thread_fold_flipped.insert(thread_id.clone());
        }
    }

    fn show_pulled_threads(&mut self, threads: Option<&Result<Vec<ReviewThread>, String>>) {
        match threads {
            Some(Ok(threads)) => self.set_threads(threads),
            Some(Err(reason)) => self.raise_problem(Problem::ThreadsNotRead(reason.clone())),
            None => {}
        }
    }

    pub fn begin_selection(&mut self, row: LineLocation) {
        self.comment_desk.begin_selection(row);
    }

    pub fn extend_selection(&mut self, row: LineLocation) {
        self.comment_desk.extend_selection(row);
    }

    pub fn is_selecting(&self) -> bool {
        self.comment_desk.row_selection().is_some()
    }

    /// The drag in progress, for a view that knows its rows and draws them without building the page.
    pub fn row_selection(&self) -> Option<&RowSelection> {
        self.comment_desk.row_selection()
    }

    pub fn cancel_selection(&mut self) {
        self.comment_desk.cancel_selection();
    }

    /// The drag ends: when the rows of the page can take a comment, a comment box opens on them.
    pub(super) fn finish_selection(
        &mut self,
        page: &Page,
    ) -> Result<Option<CommentPlace>, CommentRefusal> {
        let freshness = self.tour_freshness();
        self.comment_desk
            .finish_selection(&mut self.state, page.lines(), freshness, &self.head)
    }

    /// The comment box of a picture opens on the whole file, under the same rules as on lines.
    pub(super) fn begin_file_comment(
        &mut self,
        path: &FilePath,
    ) -> Result<CommentPlace, CommentRefusal> {
        let freshness = self.tour_freshness();
        self.comment_desk
            .begin_file_comment(&mut self.state, path, freshness, &self.head)
    }

    pub fn queued_comments(&self) -> &[QueuedComment] {
        self.state.comment_queue()
    }

    /// The save of what was typed since the last save, to run in the background; `None` when nothing is unsaved.
    pub fn typing_save(&mut self) -> Option<TypingSave> {
        self.state.typing_save()
    }

    /// "Suggest change" shows only when the range starts and ends on lines of the head.
    pub fn can_suggest_change(&self, place: &CommentPlace) -> bool {
        CommentDesk::can_suggest_change(place)
    }

    /// The comment boxes open on this review, each with what it is doing; each hangs under the
    /// last line of its place, or under the picture of its file.
    pub fn comment_boxes(&self) -> impl Iterator<Item = CommentBox<'_>> {
        let agent_out_of_reach = SendMode::LOCAL
            .iter()
            .find_map(|mode| self.send_refusal(*mode))
            .filter(|refusal| *refusal == CommentRefusal::AgentLinkOff);
        self.comment_desk
            .comment_boxes(&self.state, self.tour_freshness())
            .map(move |mut comment_box| {
                if comment_box.problem.is_none() {
                    comment_box.problem = agent_out_of_reach.clone().map(CommentProblem::Refused);
                }
                comment_box
            })
    }

    pub fn comment_box(&self, place: &CommentPlace) -> Option<CommentBox<'_>> {
        self.comment_boxes()
            .find(|comment_box| &comment_box.draft.place == place)
    }

    /// The comment box that takes the keys.
    pub fn composing(&self) -> Option<&CommentPlace> {
        self.comment_desk.composing()
    }

    /// The reviewer hands someone else's comment to the tour agent, to handle as the reviewer would ask.
    pub fn hand_to_agent(&mut self, comment: &CommentId) -> Result<(), HandToAgentRefused> {
        if self.subject().pull_request().is_none() {
            return Err(HandToAgentRefused::NotAPullRequest);
        }
        let threads = &self.pulled_threads;
        let from = sent_from(&self.screen);
        self.state
            .record(|state| state.hand_offs_mut().hand_over(threads, comment, from))
            .map_err(|error| HandToAgentRefused::NotSaved(format!("{error:#}")))??;
        self.work.want(WorkKind::DeliverHandOff);
        Ok(())
    }

    pub(super) fn agent_of_threads(&self) -> AgentOfThreads<'_> {
        AgentOfThreads {
            hand_offs: self.state.hand_offs(),
            takes_others_comments: self.subject().pull_request().is_some(),
        }
    }

    pub fn hand_offs(&self) -> &HandOffs {
        self.state.hand_offs()
    }

    /// How many tasks wait for Send tasks.
    pub fn queued_tasks(&self) -> usize {
        self.state.hand_offs().waiting_for_send_tasks().count()
    }

    /// The finish of a local review: every queued task goes to the agent in one message, in
    /// reading order, each with its own hand-off state.
    pub fn send_tasks(&mut self) -> anyhow::Result<()> {
        let order = self.reading_order();
        let threads = self.pulled_threads.clone();
        let reading_place = |task: &HandOff, thread: &ReviewThread| {
            let screen = task.step().map_or_else(
                || Screen::File(thread.path.clone()),
                |step| Screen::Step(step.clone()),
            );
            let nth = order.iter().position(|shown| *shown == screen);
            (nth.is_none(), nth, thread.original_line)
        };
        self.record_hand_offs(|hand_offs| hand_offs.send_queued_tasks(&threads, reading_place))?;
        self.work.want(WorkKind::DeliverHandOff);
        Ok(())
    }

    /// What the tour agent, GitHub or the reviewer did to the hand-offs. It is made even while a
    /// re-pin runs, so a hand-off is not delivered twice; only its save is refused then.
    pub(super) fn record_hand_offs<T>(
        &mut self,
        change: impl FnOnce(&mut HandOffs) -> T,
    ) -> anyhow::Result<T> {
        self.state.record(|state| change(state.hand_offs_mut()))
    }

    /// The oldest comment in the queue, to send again.
    pub fn queued_comment_request(&self) -> Option<CommentRequest> {
        CommentDesk::queued_comment_request(&self.state, &self.head, self.thread_home())
    }

    /// A comment sent from its box. Whatever came of it, the next hand-off may go now.
    pub fn apply_comment(&mut self, outcome: CommentOutcome) -> anyhow::Result<CommentResult> {
        let result = self.land_comment(outcome);
        self.work.want(WorkKind::DeliverHandOff);
        result
    }

    pub(super) fn land_comment(
        &mut self,
        outcome: CommentOutcome,
    ) -> anyhow::Result<CommentResult> {
        let from_findings = outcome.request.findings_carried().to_vec();
        let threads = outcome.threads.clone();
        let made = outcome.thread_made(&self.pulled_threads);
        let hand_offs_before = self.state.hand_offs().all().len();
        let code = self.code_commented_on(&outcome);
        let result = self.comment_desk.apply_comment(
            &mut self.state,
            outcome,
            (sent_from(&self.screen), code),
        );
        self.note_new_hand_off(hand_offs_before);
        let kept_on_screen = match (&result, &made) {
            (Ok(CommentResult::Sent), Some(thread)) => self.keep_thread_on_this_screen(thread),
            _ => Ok(()),
        };
        self.show_pulled_threads(threads.as_ref());
        match result {
            Ok(CommentResult::Sent) => {
                kept_on_screen?;
                self.decide_used(from_findings, made.as_ref())?;
            }
            Ok(CommentResult::Queued(_)) => self.decide_used(from_findings, None)?,
            _ => {}
        }
        result
    }

    /// The thread GitHub sends back shows under the same row on every step that shows it, so the step it
    /// was written on is kept; the first step showing the row would take it otherwise.
    fn keep_thread_on_this_screen(&mut self, thread: &ThreadId) -> anyhow::Result<()> {
        let Screen::Step(step) = &self.screen else {
            return Ok(());
        };
        let step = step.clone();
        self.state
            .record(|state| state.thread_written_on(thread.clone(), step))
    }

    /// The lines a task of a local review is about, as the page shows them; a PR's hand-off has none.
    fn code_commented_on(&self, outcome: &CommentOutcome) -> Option<String> {
        self.subject.local()?;
        let place = outcome.request.place();
        let selected = Page::of(self).selected(&RowSelection::of(place)?)?;
        Some(ask_context::code_lines(selected.lines()))
    }

    fn note_new_hand_off(&mut self, hand_offs_before: usize) {
        let Some(newest) = self
            .state
            .hand_offs()
            .all()
            .get(hand_offs_before..)
            .and_then(<[_]>::last)
        else {
            return;
        };
        match (newest.thread(), newest.state()) {
            (None, HandOffState::Failed(reason)) => {
                let comment = newest.comment().chars().take(60).collect();
                let reason = reason.clone();
                self.raise_problem(Problem::HandOffNotSent { comment, reason });
            }
            _ => self.clear_problem(ProblemKind::HandOffNotSent),
        }
    }
}

fn sent_from(screen: &Screen) -> SentFrom {
    match screen {
        Screen::Step(step) => SentFrom::Step(step.clone()),
        Screen::File(path) => SentFrom::File(path.clone()),
        Screen::Summary | Screen::Overview | Screen::SafetyNet => SentFrom::Elsewhere,
    }
}
