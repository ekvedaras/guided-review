//! What a snapshot scene shows of the findings' fixes and threads, and of the comments list.

use super::ReviewCore;
use crate::file_path::FilePath;
use crate::findings::{Anchor, Finding};
use crate::viewer::ai_review::AiReview;
use crate::viewer::finding_decisions::Fate;
use crate::viewer::finding_threads::{FindingThreads, WhyAuthor};
use crate::viewer::fix_requests::FixRequests;
use crate::viewer::line_location::{LineLocation, Side};
use crate::viewer::review_thread::{CommentId, ReviewThread, ThreadComment, ThreadId};

const STAGED_THREAD: &str = "staged-from-finding";

impl ReviewCore {
    /// The fixes a snapshot scene shows on the open findings.
    pub fn stage_fix_requests(&mut self) {
        let findings = match &self.ai_review {
            AiReview::Findings(file) => file.findings.clone(),
            _ => Vec::new(),
        };
        self.show_staged_fix_requests(&findings);
    }

    /// The first open finding, fixed by the tour agent, waiting for Mark fixed or a request for more.
    pub fn stage_fixed_finding(&mut self) {
        let Some(finding) = (match &self.ai_review {
            AiReview::Findings(file) => file.findings.first().cloned(),
            _ => None,
        }) else {
            return;
        };
        let fixed = std::slice::from_ref(&finding);
        self.record_reporting("the staged fixed finding", |state| {
            *state.fix_requests_mut() = FixRequests::staged_fixed(fixed);
            state.finding_threads_mut().say(
                finding.clone(),
                WhyAuthor::Reviewer,
                "Fix requested".to_owned(),
            );
            state.finding_threads_mut().say(
                finding.clone(),
                WhyAuthor::TourAgent,
                "Fixed in abc1234: Returns early when the list is empty, with a test for it."
                    .to_owned(),
            );
        });
        self.place_threads();
    }

    pub fn unstage_fix_requests(&mut self) {
        self.show_staged_fix_requests(&[]);
    }

    fn show_staged_fix_requests(&mut self, findings: &[Finding]) {
        self.record_reporting("the staged fix requests", |state| {
            *state.fix_requests_mut() = FixRequests::staged(findings);
        });
        self.place_threads();
    }

    /// The last open finding, dismissed, as a snapshot scene shows the dismissed section.
    pub fn stage_dismissed_finding(&mut self) {
        let Some(finding) = (match &self.ai_review {
            AiReview::Findings(file) => file.findings.last().cloned(),
            _ => None,
        }) else {
            return;
        };
        let id = finding.id.clone();
        self.record_reporting("the staged dismissed finding", |state| {
            state.decide_finding(finding, Fate::Dismissed);
        });
        self.close_finding(&id);
    }

    /// The first open finding, asked to fix and asked why about, then used: its comment became a thread on
    /// its lines, which carries what was said about it.
    pub fn stage_thread_from_finding(&mut self) {
        let Some(finding) = (match &self.ai_review {
            AiReview::Findings(file) => file.findings.first().cloned(),
            _ => None,
        }) else {
            return;
        };
        let Anchor::Lines { file, side, lines } = &finding.anchor else {
            return;
        };
        let id = ThreadId::new(STAGED_THREAD);
        let thread = ReviewThread {
            id: id.clone(),
            is_resolved: false,
            is_outdated: false,
            path: file.clone(),
            line: Some(lines.end),
            start_line: None,
            side: Side::from(*side),
            is_file_comment: false,
            original_line: Some(lines.end),
            original_start_line: None,
            original_commit: None,
            comments: vec![ThreadComment {
                id: CommentId::new(STAGED_THREAD),
                author: "reviewer".to_owned(),
                avatar_url: None,
                is_bot: false,
                by_agent: None,
                by_viewer: true,
                body: finding.body.clone(),
                url: String::new(),
                reactions: Vec::new(),
            }],
        };
        let used = std::slice::from_ref(&finding);
        self.record_reporting("the staged thread from a finding", |state| {
            *state.fix_requests_mut() = FixRequests::staged(used);
            *state.finding_threads_mut() = FindingThreads::staged(used);
            state.finding_used(finding.clone(), Some(id));
        });
        self.close_finding(&finding.id);
        self.staged_threads = vec![thread.clone()];
        self.pulled_threads.push(thread);
        self.place_threads();
    }

    pub fn unstage_thread_from_finding(&mut self) {
        self.record_reporting("the staged thread from a finding", |state| {
            state.forget_findings_used_in(&ThreadId::new(STAGED_THREAD));
        });
        self.show_staged_fix_requests(&[]);
        self.show_staged_finding_threads(&[]);
        let staged = std::mem::take(&mut self.staged_threads);
        self.pulled_threads
            .retain(|thread| !staged.iter().any(|gone| gone.id == thread.id));
        self.place_threads();
    }

    /// The Ask why threads a snapshot scene shows on the open findings.
    pub fn stage_finding_threads(&mut self) {
        let findings = match &self.ai_review {
            AiReview::Findings(file) => file.findings.clone(),
            _ => Vec::new(),
        };
        self.show_staged_finding_threads(&findings);
    }

    pub fn unstage_finding_threads(&mut self) {
        self.show_staged_finding_threads(&[]);
    }

    fn show_staged_finding_threads(&mut self, findings: &[Finding]) {
        self.record_reporting("the staged finding threads", |state| {
            *state.finding_threads_mut() = FindingThreads::staged(findings);
        });
        self.place_threads();
    }

    /// Three threads at `at`: one open with two replies, one resolved, and one outdated on a file no
    /// screen shows.
    pub fn stage_comment_threads(&mut self, at: &LineLocation) {
        let Ok(gone) = FilePath::new("src/removed_helper.rs") else {
            return;
        };
        let thread =
            |id: &str, path: FilePath, is_resolved, is_outdated, bodies: &[(&str, &str)]| {
                ReviewThread {
                    id: ThreadId::new(id),
                    is_resolved,
                    is_outdated,
                    line: (!is_outdated).then_some(at.line),
                    start_line: None,
                    side: at.side,
                    is_file_comment: false,
                    original_line: Some(at.line),
                    original_start_line: None,
                    original_commit: None,
                    path,
                    comments: bodies
                        .iter()
                        .enumerate()
                        .map(|(position, (author, body))| ThreadComment {
                            id: CommentId::new(&format!("{id}-{position}")),
                            author: (*author).to_owned(),
                            avatar_url: None,
                            is_bot: false,
                            by_agent: None,
                            by_viewer: false,
                            body: (*body).to_owned(),
                            url: String::new(),
                            reactions: Vec::new(),
                        })
                        .collect(),
                }
            };
        let staged = vec![
            thread(
                "staged-comments-open",
                at.path.clone(),
                false,
                false,
                &[
                    (
                        "octocat",
                        "Why does this skip the cache when the key is empty?",
                    ),
                    (
                        "ekvedaras",
                        "An empty key means the entry was never written.",
                    ),
                    ("octocat", "Then say so in a comment, I read it as a bug."),
                ],
            ),
            thread(
                "staged-comments-resolved",
                at.path.clone(),
                true,
                false,
                &[
                    ("ekvedaras", "Nit: this name hides what it returns."),
                    ("octocat", "Renamed."),
                ],
            ),
            thread(
                "staged-comments-outdated",
                gone,
                false,
                true,
                &[("octocat", "This helper is called from two places now.")],
            ),
        ];
        self.staged_threads.clone_from(&staged);
        self.pulled_threads.extend(staged);
        self.place_threads();
    }

    pub fn unstage_comment_threads(&mut self) {
        let staged = std::mem::take(&mut self.staged_threads);
        self.pulled_threads
            .retain(|thread| !staged.iter().any(|gone| gone.id == thread.id));
        self.place_threads();
    }
}
