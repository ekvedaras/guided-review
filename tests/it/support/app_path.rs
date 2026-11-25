//! Drives a `Review` the way the app's review session does: request, run, apply.
//! The app runs the middle part on a background thread; tests run it in line.

use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{
    Audience, CodeHost, CommentPlace, CommentResult, FinishResult, Read, Review, ReviewVerdict,
    SendMode, ThreadAction, ThreadActionResult,
};

pub trait AppPath {
    fn check_pr_head(&mut self, host: &dyn CodeHost) -> anyhow::Result<()>;
    fn pull_pr_description(&mut self, host: &dyn CodeHost) -> anyhow::Result<()>;
    fn sync(&mut self, host: &dyn CodeHost) -> anyhow::Result<()>;
    fn pull_threads(&mut self, host: &dyn CodeHost) -> anyhow::Result<()>;
    fn pull_pending_review(&mut self, host: &dyn CodeHost) -> anyhow::Result<()>;
    fn pull_viewer_authorship(&mut self, host: &dyn CodeHost) -> anyhow::Result<()>;
    fn pull_last_review(&mut self, host: &dyn CodeHost) -> anyhow::Result<()>;
    /// `None` when there is no draft at `place`.
    fn send_comment(
        &mut self,
        place: &CommentPlace,
        mode: SendMode,
        host: &dyn CodeHost,
    ) -> anyhow::Result<Option<CommentResult>>;
    fn act_on_thread(&mut self, action: ThreadAction, host: &dyn CodeHost) -> ThreadActionResult;
    fn finish_review(
        &mut self,
        verdict: ReviewVerdict,
        body: &str,
        host: &dyn CodeHost,
    ) -> FinishResult;
}

/// The fixtures review PRs only.
fn pr_of(review: &Review) -> PullRequestUrl {
    review.subject().pull_request().unwrap().clone()
}

impl AppPath for Review {
    fn check_pr_head(&mut self, host: &dyn CodeHost) -> anyhow::Result<()> {
        let tips = host
            .pull_request_tips(&pr_of(self))
            .inspect_err(|error| self.could_not_read(Read::PrHead, error))?;
        self.pr_head_is(tips.head);
        Ok(())
    }

    fn pull_pr_description(&mut self, host: &dyn CodeHost) -> anyhow::Result<()> {
        let body = host
            .pull_request_description(&pr_of(self))
            .inspect_err(|error| self.could_not_read(Read::PrDescription, error))?;
        self.set_pr_description(&body)
    }

    fn sync(&mut self, host: &dyn CodeHost) -> anyhow::Result<()> {
        let outcome = self.sync_request().unwrap().run(host);
        self.apply_sync(outcome)
    }

    fn pull_threads(&mut self, host: &dyn CodeHost) -> anyhow::Result<()> {
        let threads = host
            .review_threads(&pr_of(self))
            .inspect_err(|error| self.threads_not_read(error))?;
        self.set_threads(&threads);
        Ok(())
    }

    fn pull_pending_review(&mut self, host: &dyn CodeHost) -> anyhow::Result<()> {
        let exists = host
            .has_pending_review(&pr_of(self))
            .inspect_err(|error| self.could_not_read(Read::PendingReview, error))?;
        self.pending_review_is(exists);
        Ok(())
    }

    fn pull_last_review(&mut self, host: &dyn CodeHost) -> anyhow::Result<()> {
        let reviews = host
            .reviews(&pr_of(self))
            .inspect_err(|error| self.could_not_read(Read::LastReview, error))?;
        self.reviews_are(reviews);
        Ok(())
    }

    fn pull_viewer_authorship(&mut self, host: &dyn CodeHost) -> anyhow::Result<()> {
        let authored = host
            .viewer_authored(&pr_of(self))
            .inspect_err(|error| self.could_not_read(Read::Authorship, error))?;
        self.viewer_authorship_is(authored);
        Ok(())
    }

    fn send_comment(
        &mut self,
        place: &CommentPlace,
        mode: SendMode,
        host: &dyn CodeHost,
    ) -> anyhow::Result<Option<CommentResult>> {
        let Some(request) = self.start_sending(place, mode, Audience::Author) else {
            return Ok(None);
        };
        self.apply_comment(request.run(host)).map(Some)
    }

    fn act_on_thread(&mut self, action: ThreadAction, host: &dyn CodeHost) -> ThreadActionResult {
        let outcome = self.thread_action_request(action).run(host);
        self.apply_thread_action(outcome)
    }

    fn finish_review(
        &mut self,
        verdict: ReviewVerdict,
        body: &str,
        host: &dyn CodeHost,
    ) -> FinishResult {
        match self.finish_review_request(verdict, body) {
            Ok(request) => self.apply_finish(request.run(host)),
            Err(refusal) => FinishResult::Refused(refusal),
        }
    }
}
