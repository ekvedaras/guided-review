//! Drives a `Review` the way the app's review session does: request, run, apply.
//! The app runs the middle part on a background thread; tests run it in line.

use guided_review::viewer::{
    CommentPlace, CommentResult, FinishResult, GitHubPort, Read, Review, ReviewVerdict, SendMode,
    ThreadAction, ThreadActionResult,
};

pub trait AppPath {
    fn check_pr_head(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()>;
    fn pull_pr_description(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()>;
    fn sync(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()>;
    fn pull_threads(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()>;
    fn pull_pending_review(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()>;
    fn pull_viewer_authorship(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()>;
    /// `None` when there is no draft at `place`.
    fn send_comment(
        &mut self,
        place: &CommentPlace,
        mode: SendMode,
        github: &dyn GitHubPort,
    ) -> anyhow::Result<Option<CommentResult>>;
    /// Oldest first, stopping at the first comment that does not go, like the app's retry.
    fn send_queued_comments(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()>;
    fn act_on_thread(
        &mut self,
        action: ThreadAction,
        github: &dyn GitHubPort,
    ) -> ThreadActionResult;
    fn finish_review(
        &mut self,
        verdict: ReviewVerdict,
        body: &str,
        github: &dyn GitHubPort,
    ) -> FinishResult;
}

impl AppPath for Review {
    fn check_pr_head(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let tips = github
            .pull_request_tips(self.pr())
            .inspect_err(|error| self.could_not_read(Read::PrHead, error))?;
        self.pr_head_is(tips.head);
        Ok(())
    }

    fn pull_pr_description(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let body = github
            .pull_request_description(self.pr())
            .inspect_err(|error| self.could_not_read(Read::PrDescription, error))?;
        self.set_pr_description(&body)
    }

    fn sync(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let outcome = self.sync_request().run(github);
        self.apply_sync(outcome)
    }

    fn pull_threads(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let threads = github
            .review_threads(self.pr())
            .inspect_err(|error| self.threads_not_read(error))?;
        self.set_threads(&threads);
        Ok(())
    }

    fn pull_pending_review(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let exists = github
            .has_pending_review(self.pr())
            .inspect_err(|error| self.could_not_read(Read::PendingReview, error))?;
        self.pending_review_is(exists);
        Ok(())
    }

    fn pull_viewer_authorship(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        let authored = github.viewer_authored(self.pr())?;
        self.viewer_authorship_is(authored);
        Ok(())
    }

    fn send_comment(
        &mut self,
        place: &CommentPlace,
        mode: SendMode,
        github: &dyn GitHubPort,
    ) -> anyhow::Result<Option<CommentResult>> {
        let Some(request) = self.comment_request(place, mode) else {
            return Ok(None);
        };
        self.apply_comment(request.run(github)).map(Some)
    }

    fn send_queued_comments(&mut self, github: &dyn GitHubPort) -> anyhow::Result<()> {
        while let Some(request) = self.queued_comment_request() {
            if self.apply_comment(request.run(github))? != CommentResult::Sent {
                break;
            }
        }
        Ok(())
    }

    fn act_on_thread(
        &mut self,
        action: ThreadAction,
        github: &dyn GitHubPort,
    ) -> ThreadActionResult {
        let outcome = self.thread_action_request(action).run(github);
        self.apply_thread_action(outcome)
    }

    fn finish_review(
        &mut self,
        verdict: ReviewVerdict,
        body: &str,
        github: &dyn GitHubPort,
    ) -> FinishResult {
        match self.finish_review_request(verdict, body) {
            Ok(request) => self.apply_finish(request.run(github)),
            Err(refusal) => FinishResult::Refused(refusal),
        }
    }
}
