//! Where the reviewer is and what they have read: the reading order and the moves through it, read
//! ticks per step and file, folds, progress and the viewed marks synced with the host.

use super::{
    ReadProgress, ReadUnit, ReadingPlace, ReviewCore, RouteStop, StepId, SummaryStats, TourLead,
    UnreadFile, UnreadStep,
};
use crate::file_path::FilePath;
use crate::tour::{Step, Tour};
use crate::viewer::host_outage::Affected;
use crate::viewer::jump::JumpTarget;
use crate::viewer::ports::HostError;
use crate::viewer::problems::{Problem, ProblemKind};
use crate::viewer::screen::{NextScreen, ReadSubject, Screen};
use crate::viewer::viewed_sync::{Coverage, SyncOutcome, SyncRequest};
use crate::viewer::work_agenda::{PUSH_DEBOUNCE, WorkKind};

impl ReviewCore {
    /// Where a screen sits in the reading order, without building the order; the safety net is last.
    pub fn reading_position(&self, screen: &Screen) -> Option<usize> {
        match screen {
            Screen::SafetyNet => Some(self.steps_in_reading_order.len()),
            Screen::Summary | Screen::Overview | Screen::File(_) | Screen::Step(_) => self
                .steps_in_reading_order
                .iter()
                .position(|shown| shown == screen),
        }
    }

    pub(super) fn reading_order(&self) -> Vec<Screen> {
        let has_safety_net = self.has_uncovered_changes || !self.threads.in_safety_net().is_empty();
        self.steps_in_reading_order
            .iter()
            .cloned()
            .chain(has_safety_net.then_some(Screen::SafetyNet))
            .collect()
    }

    /// Ids the tour does not hold are ignored: the sidebar only offers real steps.
    pub fn go_to(&mut self, id: &StepId) {
        self.go_to_screen(Screen::Step(id.clone()));
    }

    /// A folded step opens for the session so the file or symbol the reviewer asked for is on screen.
    pub fn jump_to(&mut self, target: &JumpTarget) {
        self.go_to_screen(target.screen.clone());
        if let Screen::Step(id) = &target.screen
            && self.is_folded(id)
        {
            self.toggle_fold(id);
        }
    }

    pub fn go_to_summary(&mut self) {
        self.go_to_screen(Screen::Summary);
    }

    pub fn go_to_overview(&mut self) {
        self.go_to_screen(Screen::Overview);
    }

    /// Ids of files the tour does not hold are ignored, like ids of steps.
    pub fn go_to_file(&mut self, path: &FilePath) {
        self.go_to_screen(Screen::File(path.clone()));
    }

    pub fn go_to_safety_net(&mut self) {
        self.go_to_screen(Screen::SafetyNet);
    }

    pub(super) fn go_to_screen(&mut self, screen: Screen) {
        if self.reading_order().contains(&screen) {
            self.screen = screen;
        }
    }

    /// False when there is no screen after this one.
    pub fn go_forward(&mut self) -> bool {
        self.step_by(1)
    }

    /// For the scroll script, which scrolls through every piece, so a folded step is opened.
    pub fn go_forward_unfolded(&mut self) -> bool {
        if !self.go_forward() {
            return false;
        }
        if let Screen::Step(id) = self.screen.clone()
            && self.is_folded(&id)
        {
            self.toggle_fold(&id);
        }
        true
    }

    /// The Next button: on to the next screen that is not a step already read, which the sidebar
    /// still reaches. False when there is none.
    pub fn go_on(&mut self) -> bool {
        match self.onward_screen() {
            Some(screen) => {
                self.screen = screen;
                true
            }
            None => false,
        }
    }

    fn onward_screen(&self) -> Option<Screen> {
        let reading_order = self.reading_order();
        let here = reading_order
            .iter()
            .position(|screen| screen == &self.screen)?;
        reading_order
            .into_iter()
            .skip(here + 1)
            .find(|screen| !self.is_read_screen(screen))
    }

    fn is_read_screen(&self, screen: &Screen) -> bool {
        match screen {
            Screen::Step(id) => self.is_ticked(id),
            Screen::File(path) => self.is_file_ticked(path),
            Screen::Summary | Screen::Overview | Screen::SafetyNet => false,
        }
    }

    fn is_unread_screen(&self, screen: &Screen) -> bool {
        match screen {
            Screen::Step(id) => !self.is_ticked(id),
            Screen::File(path) => !self.is_file_ticked(path),
            Screen::Summary | Screen::Overview | Screen::SafetyNet => false,
        }
    }

    /// False on the summary, the first screen.
    pub fn go_back(&mut self) -> bool {
        self.step_by(-1)
    }

    /// The first step after the open screen that is not read yet, starting over from the top.
    /// False when every step is read.
    pub fn go_to_next_unread(&mut self) -> bool {
        let reading_order = self.reading_order();
        let here = reading_order
            .iter()
            .position(|screen| screen == &self.screen)
            .unwrap_or(0);
        let after = reading_order.iter().skip(here + 1);
        let next = after
            .chain(reading_order.iter().take(here + 1))
            .find(|screen| self.is_unread_screen(screen))
            .cloned();
        match next {
            Some(screen) if screen != self.screen => {
                self.screen = screen;
                true
            }
            _ => false,
        }
    }

    pub fn tour_lead(&self) -> TourLead {
        let progress = self.progress();
        if progress.total == 0 {
            return TourLead::Nowhere;
        }
        let finish = self.finish_action();
        if let Tour::Basic(_) = &self.tour {
            return match self.first_unread_file() {
                _ if progress.is_complete() => TourLead::FinishFiles {
                    files: progress.total,
                    finish,
                },
                Some(first) if progress.read == 0 => TourLead::StartFile(first),
                Some(next) => TourLead::ContinueFile(next),
                None => TourLead::Nowhere,
            };
        }
        if progress.is_complete() {
            return TourLead::Finish {
                steps: progress.total,
                finish,
            };
        }
        match self.first_unread_step() {
            Some(first) if progress.read == 0 => TourLead::Start(first),
            Some(next) => TourLead::Continue(next),
            None => TourLead::Nowhere,
        }
    }

    pub fn route(&self) -> Vec<RouteStop> {
        fn count(steps: &[Step]) -> usize {
            steps.iter().map(|step| 1 + count(&step.children)).sum()
        }
        let steps = self.tour.guided().map_or(&[][..], |guided| &guided.steps);
        steps
            .iter()
            .map(|step| RouteStop {
                id: step.id.clone(),
                title: step.title.clone(),
                why: step.why.clone(),
                details: count(&step.children),
                is_read: self.is_ticked(&step.id),
                has_check: step.check.is_some(),
            })
            .collect()
    }

    pub fn summary_stats(&self) -> SummaryStats {
        SummaryStats {
            steps: self.progress().total,
            files: self.files.len(),
            added_lines: self.files.iter().map(|file| file.added).sum(),
            removed_lines: self.files.iter().map(|file| file.removed).sum(),
            open_threads: self
                .pulled_threads
                .iter()
                .filter(|thread| !thread.is_resolved)
                .count(),
        }
    }

    /// The first step in reading order that is not read yet.
    pub fn first_unread_step(&self) -> Option<UnreadStep> {
        self.steps_in_reading_order.iter().find_map(|screen| {
            let Screen::Step(id) = screen else {
                return None;
            };
            (!self.is_ticked(id)).then(|| UnreadStep {
                id: id.clone(),
                title: self.step(id).map_or_default(|step| step.title.clone()),
            })
        })
    }

    pub fn is_file_ticked(&self, path: &FilePath) -> bool {
        self.state.file_read_tick(path).is_some()
    }

    /// The file on screen, if it is not read yet: the footer ticks it on the way on.
    pub fn file_to_tick_on_screen(&self) -> Option<FilePath> {
        match &self.screen {
            Screen::File(path) if !self.is_file_ticked(path) => Some(path.clone()),
            _ => None,
        }
    }

    pub fn is_read(&self, subject: &ReadSubject) -> bool {
        match subject {
            ReadSubject::Step(id) => self.is_ticked(id),
            ReadSubject::File(path) => self.is_file_ticked(path),
        }
    }

    /// The first file in reading order that is not read yet; none in a guided tour.
    pub fn first_unread_file(&self) -> Option<UnreadFile> {
        self.steps_in_reading_order.iter().find_map(|screen| {
            let Screen::File(path) = screen else {
                return None;
            };
            (!self.is_file_ticked(path)).then(|| UnreadFile { path: path.clone() })
        })
    }

    /// Only a PR on a host with viewed marks has marks to keep in step with the ticks.
    fn want_viewed_synced(&mut self) {
        if self.subject.pull_request().is_some() && self.viewed_sync.has_host_marks() {
            self.work.want_in(WorkKind::SyncViewed, PUSH_DEBOUNCE);
        }
    }

    /// Only files of this tour can be read; a file the tour does not hold is ignored. Viewed
    /// marks follow it one to one.
    pub fn tick_file(&mut self, path: &FilePath) -> anyhow::Result<()> {
        self.want_viewed_synced();
        if !self.reading_order().contains(&Screen::File(path.clone())) {
            return Ok(());
        }
        let coverage = self.coverage();
        let subject = ReadSubject::File(path.clone());
        self.state.change(|state| {
            state.tick_file(path, &self.head);
            self.viewed_sync.wish_after_tick(state, &coverage, &subject);
        })
    }

    pub fn untick_file(&mut self, path: &FilePath) -> anyhow::Result<()> {
        self.want_viewed_synced();
        let coverage = self.coverage();
        let subject = ReadSubject::File(path.clone());
        self.state.change(|state| {
            state.untick_file(path);
            self.viewed_sync
                .wish_after_untick(state, &coverage, &subject);
        })
    }

    pub fn is_ticked(&self, id: &StepId) -> bool {
        self.state.read_tick(id).is_some()
    }

    /// The tick is kept in memory even when saving fails, so the reviewer loses nothing this session.
    /// Viewed marks sync once no other tick or untick came for `PUSH_DEBOUNCE`.
    pub fn tick(&mut self, id: &StepId) -> anyhow::Result<()> {
        self.want_viewed_synced();
        if !self.reading_order().contains(&Screen::Step(id.clone())) {
            return Ok(());
        }
        let coverage = self.coverage();
        let subject = ReadSubject::Step(id.clone());
        self.state.change(|state| {
            state.tick(id, &self.head);
            self.fold_flipped.remove(id);
            self.viewed_sync.wish_after_tick(state, &coverage, &subject);
        })
    }

    pub fn untick(&mut self, id: &StepId) -> anyhow::Result<()> {
        self.want_viewed_synced();
        let coverage = self.coverage();
        let subject = ReadSubject::Step(id.clone());
        self.state.change(|state| {
            state.untick(id);
            self.fold_flipped.remove(id);
            self.viewed_sync
                .wish_after_untick(state, &coverage, &subject);
        })
    }

    /// Skim, ticked and gone steps fold; the reviewer can flip that for the session without losing the tick.
    pub fn is_folded(&self, id: &StepId) -> bool {
        let folds_by_default = self.is_ticked(id)
            || self
                .step(id)
                .is_some_and(|step| step.skim || self.is_gone(step));
        folds_by_default != self.fold_flipped.contains(id)
    }

    /// Opens or folds every file box of the step at once.
    pub fn toggle_fold(&mut self, id: &StepId) {
        if !self.fold_flipped.remove(id) {
            self.fold_flipped.insert(id.clone());
        }
        let step = Screen::Step(id.clone());
        self.box_fold_flipped.retain(|(screen, _)| *screen != step);
    }

    /// A folded step still lists its file boxes, each folded on its own. The box of a file screen
    /// starts open.
    pub fn is_box_folded(&self, screen: &Screen, path: &FilePath) -> bool {
        let folds_by_default = match screen {
            Screen::Step(id) => self.is_folded(id),
            Screen::Summary | Screen::Overview | Screen::File(_) | Screen::SafetyNet => false,
        };
        folds_by_default
            != self
                .box_fold_flipped
                .contains(&(screen.clone(), path.clone()))
    }

    /// Folds or opens each of the boxes, leaving one already as asked alone.
    pub fn set_boxes_folded(&mut self, screen: &Screen, paths: &[FilePath], folded: bool) {
        for path in paths {
            if self.is_box_folded(screen, path) != folded {
                self.toggle_box_fold(screen, path);
            }
        }
    }

    pub fn toggle_box_fold(&mut self, screen: &Screen, path: &FilePath) {
        let key = (screen.clone(), path.clone());
        if !self.box_fold_flipped.remove(&key) {
            self.box_fold_flipped.insert(key);
        }
    }

    pub fn read_unit(&self) -> ReadUnit {
        match &self.tour {
            Tour::Guided(_) => ReadUnit::Steps,
            Tour::Basic(_) => ReadUnit::Files,
        }
    }

    /// Only steps of this tour count: ticks of steps a refresh removed stay in the file but not here.
    pub fn progress(&self) -> ReadProgress {
        let reading_order = self.reading_order();
        let subjects = reading_order.iter().filter_map(|screen| match screen {
            Screen::Step(id) => Some(ReadSubject::Step(id.clone())),
            Screen::File(path) => Some(ReadSubject::File(path.clone())),
            Screen::Summary | Screen::Overview | Screen::SafetyNet => None,
        });
        ReadProgress {
            read: subjects
                .clone()
                .filter(|subject| self.is_read(subject))
                .count(),
            total: subjects.count(),
        }
    }

    /// The banner text while viewed sync is off; ticks keep working without it.
    pub fn sync_banner(&self) -> Option<&str> {
        self.viewed_sync.banner()
    }

    /// Drives the "not synced" dot.
    pub fn not_synced(&self) -> bool {
        self.viewed_sync.not_synced(&self.state)
    }

    /// `None` for a local review, which has no PR, and on a host with no viewed marks.
    pub fn sync_request(&self) -> Option<SyncRequest> {
        let pr = self.subject.pull_request()?;
        self.viewed_sync
            .request(&self.state, &self.coverage(), pr, &self.head)
    }

    /// An error means something stays queued or unread; the next sync tries again.
    /// An answer for another head, e.g. from before a re-pin, is dropped: the next sync asks again.
    pub fn apply_sync(&mut self, outcome: SyncOutcome) -> anyhow::Result<()> {
        if self.viewed_sync.is_off() || !outcome.is_for(&self.head) {
            return Ok(());
        }
        let coverage = self.coverage();
        let mut error = None;
        let saved = self.state.record(|state| {
            let (unticked, sync_error) = self
                .viewed_sync
                .take_in(state, &coverage, &self.head, outcome);
            for subject in &unticked {
                if let ReadSubject::Step(step) = subject {
                    self.fold_flipped.remove(step);
                }
            }
            error = sync_error;
        });
        self.clear_problem(ProblemKind::ViewedNotSynced);
        match error {
            Some(HostError::NotAnswering(host, cause)) => {
                self.host_not_answering(host, cause, Affected::ViewedMarks);
                Err(HostError::NotAnswering(host, cause).into())
            }
            Some(error) => {
                self.host_answered(Affected::ViewedMarks);
                let settled = self.viewed_sync.settle(error);
                if let Err(error) = &settled {
                    self.raise_problem(Problem::ViewedNotSynced(error.root_cause().to_string()));
                }
                settled
            }
            None => {
                self.host_answered(Affected::ViewedMarks);
                saved
            }
        }
    }

    fn coverage(&self) -> Coverage {
        match &self.tour {
            Tour::Guided(guided) => Coverage::of(guided, &self.files),
            Tour::Basic(_) => Coverage::of_files(self.files.iter().map(|file| &file.path)),
        }
    }

    /// Changed code no page can show, by piece id: the reviewer has to read it on the host.
    pub fn changes_not_shown(&self) -> Vec<String> {
        let unplaced = self
            .files
            .iter()
            .flat_map(|file| file.not_shown.iter().cloned());
        let outside_their_file = self.files.iter().flat_map(|file| {
            file.pieces
                .iter()
                .filter(|piece| {
                    !self
                        .files
                        .iter()
                        .any(|other| other.path == *piece.id.path())
                })
                .map(|piece| piece.id.to_string())
        });
        unplaced.chain(outside_their_file).collect()
    }

    pub fn reading_place(&self) -> ReadingPlace {
        let reading_order = self.reading_order();
        let position = reading_order
            .iter()
            .position(|screen| screen == &self.screen);
        let is_step = |screen: &&Screen| matches!(screen, Screen::Step(_));
        let is_file = |screen: &&Screen| matches!(screen, Screen::File(_));
        let next = self.onward_screen().and_then(|screen| match &screen {
            Screen::Step(id) => Some(NextScreen::Step {
                title: self.step(id)?.title.clone(),
            }),
            Screen::SafetyNet => Some(NextScreen::SafetyNet),
            Screen::File(path) => Some(NextScreen::File { path: path.clone() }),
            Screen::Summary | Screen::Overview => None,
        });
        ReadingPlace {
            step_number: match (&self.screen, position) {
                (Screen::Step(_), Some(position)) => Some(
                    reading_order
                        .iter()
                        .take(position + 1)
                        .filter(is_step)
                        .count(),
                ),
                _ => None,
            },
            step_count: reading_order.iter().filter(is_step).count(),
            file_number: match (&self.screen, position) {
                (Screen::File(_), Some(position)) => Some(
                    reading_order
                        .iter()
                        .take(position + 1)
                        .filter(is_file)
                        .count(),
                ),
                _ => None,
            },
            file_count: reading_order.iter().filter(is_file).count(),
            has_previous: position.is_some_and(|position| position > 0),
            next,
        }
    }

    fn step_by(&mut self, offset: isize) -> bool {
        let reading_order = self.reading_order();
        let next = reading_order
            .iter()
            .position(|screen| screen == &self.screen)
            .and_then(|position| position.checked_add_signed(offset))
            .and_then(|position| reading_order.get(position));
        match next {
            Some(screen) => {
                self.screen = screen.clone();
                true
            }
            None => false,
        }
    }
}
