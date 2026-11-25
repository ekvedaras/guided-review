use std::cell::OnceCell;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Instant;

use super::ReviewCore;
use crate::viewer::ask_desk::AskDesk;
use crate::viewer::page::Page;
use crate::viewer::replacing_jobs::{ReplacingJob, StartedJob};
use crate::viewer::work_agenda::Work;

/// The open review, with the page of its screen built once and kept until the review changes.
///
/// The review is read through `Deref` and changed only through `DerefMut`, which drops the kept page
/// first. Its fields are private to this module, so no method can change the review and leave a page
/// built from what it was before: the next `page()` builds it again.
pub struct Review {
    review: ReviewCore,
    page: OnceCell<Arc<Page>>,
}

impl Review {
    pub(super) fn keeping_page_of(review: ReviewCore) -> Self {
        Self {
            review,
            page: OnceCell::new(),
        }
    }

    /// The page of the open screen, as the review is now.
    pub fn page(&self) -> &Arc<Page> {
        self.page.get_or_init(|| Arc::new(Page::of(&self.review)))
    }

    pub(super) fn into_core(self) -> ReviewCore {
        self.review
    }

    /// The window had the end of the screen on show. No input of the page, so the page is kept:
    /// the window reports this on every frame.
    pub fn saw_end_of_screen(&mut self) {
        self.review.saw_end_of_screen();
    }

    /// Asks change on every line an answer streams in, and the page shows only which lines have
    /// Asks, so the page is dropped only when those lines change.
    pub fn change_asks<T>(&mut self, change: impl FnOnce(&mut AskDesk) -> T) -> T {
        let before = self.review.ask_desk.asked_lines_on(&self.review.screen);
        let result = change(&mut self.review.ask_desk);
        if self.review.ask_desk.asked_lines_on(&self.review.screen) != before {
            self.page.take();
        }
        result
    }

    /// The work agenda is no input of the page, so work falls due and is wanted without dropping
    /// the page.
    pub fn due_work(&mut self, now: Instant) -> Vec<Work> {
        self.review.due_work(now)
    }

    pub fn pull_from_host(&mut self) {
        self.review.pull_from_host();
    }

    /// Nor are the replacing jobs. A due job is a reload asked for while another job ran, which
    /// changes nothing else until it lands.
    pub fn due_job(&mut self) -> Option<StartedJob> {
        self.review.due_job()
    }

    pub fn job_did(&mut self, job: ReplacingJob, activity: String) {
        self.review.job_did(job, activity);
    }
}

impl Deref for Review {
    type Target = ReviewCore;

    fn deref(&self) -> &ReviewCore {
        &self.review
    }
}

impl DerefMut for Review {
    fn deref_mut(&mut self) -> &mut ReviewCore {
        self.page.take();
        &mut self.review
    }
}
