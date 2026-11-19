//! What the sidebar shows, read from the review once per change (with `PageRows`), so a frame, which
//! comes on every scroll step, asks the review nothing.

use crate::app::components::finish_review::Finishing;
use gpui::SharedString;
use std::sync::Arc;

use crate::viewer::{
    AiReview, ApproveWarning, BranchOffer, DiskNews, DiskOffer, GuidedTourBanner,
    GuidedTourProgress, GuidedTourRefusal, GuidedTourStage, Problem, ReadProgress, ReadUnit,
    Review, ReviewerLine, ReviewerStatus, Screen, StanceAge, StepId, TerminalId, TourAgentState,
    TreeRow,
};

pub struct SidebarStep {
    pub id: StepId,
    pub label: SharedString,
    pub thread_count: usize,
    pub depth: usize,
    pub is_current: bool,
    pub is_ticked: bool,
    /// Every piece of the step went away in a re-pin.
    pub is_gone: bool,
    pub has_check: bool,
    pub is_skim: bool,
}

/// A basic review's sidebar: the overview, then the changed files as a folder tree.
pub struct SidebarTree {
    pub is_overview_current: bool,
    pub rows: Vec<TreeRow>,
}

pub struct SidebarSafetyNet {
    pub pieces: usize,
    pub thread_count: usize,
    pub is_current: bool,
}

/// Where the viewer stands on the PR, the lead of the sidebar's reviews.
#[derive(Clone)]
pub struct SidebarYourReview {
    pub title: SharedString,
    /// When the last review was sent and what happened since; empty when none was sent.
    pub since: SharedString,
    pub body: SharedString,
    pub is_body_long: bool,
    pub status: ReviewerStatus,
    pub age: StanceAge,
    pub is_marked_done: bool,
    pub avatar_url: Option<SharedString>,
}

/// The PR's reviews in the sidebar: the viewer's, then everyone else's.
#[derive(Clone, Default)]
pub struct SidebarReviews {
    pub yours: Option<SidebarYourReview>,
    pub others: Vec<ReviewerLine>,
}

impl SidebarReviews {
    pub fn of(review: &Review) -> Self {
        Self {
            yours: review.your_review().map(|yours| SidebarYourReview {
                title: yours.title().into(),
                since: yours.since().into(),
                body: yours.body().to_owned().into(),
                is_body_long: yours.body_is_long(),
                status: yours.status,
                age: yours.age,
                is_marked_done: yours.is_marked_done,
                avatar_url: yours.avatar_url.as_deref().map(|url| url.to_owned().into()),
            }),
            others: review
                .reviewer_lines()
                .into_iter()
                .filter(|line| !line.is_viewer)
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.yours.is_none() && self.others.is_empty()
    }
}

/// How finishing reads in the sidebar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SidebarFinish {
    /// A review of the viewer's is on GitHub already, so finishing updates it.
    pub has_last_review: bool,
    /// A re-review with nothing new to send can be marked done.
    pub can_mark_done: bool,
    pub is_marked_done: bool,
}

/// The PR's tour agent as the sidebar says it.
pub struct SidebarTourAgent {
    /// An agent, or news about one; "none" is not worth the sidebar's room.
    pub is_shown: bool,
    pub line: SharedString,
    pub can_show: bool,
    pub can_retry: bool,
    /// Several agents work in a local review's folder: Use this agent for each.
    pub choices: Vec<TerminalId>,
}

/// The review agent's findings as the sidebar says it: shown once there is news of them.
pub struct SidebarAiReview {
    pub line: SharedString,
    pub is_unreadable: bool,
}

/// The guided tour of a basic review's PR as the sidebar says it: the stage of its writing, Write
/// guided tour while nothing is asked, and the banner once it is ready.
pub struct SidebarGuidedTour {
    pub stage: Option<GuidedTourStage>,
    /// Show agent beside the stage, when the tour agent can be shown.
    pub can_show_agent: bool,
    /// Write guided tour, offered while nothing is asked; the reason when it is off.
    pub write: Option<WriteOffer>,
    pub banner: Option<GuidedTourBanner>,
}

/// Write guided tour in the sidebar: a button, or the reason it is off.
#[derive(Clone, Copy)]
pub enum WriteOffer {
    Available,
    Refused(GuidedTourRefusal),
}

/// A problem as the sidebar lists it: the headline, and under it, quieter, what it leaves out.
#[derive(Clone)]
pub struct SidebarProblem {
    pub headline: SharedString,
    pub detail: Option<SharedString>,
    pub problem: Problem,
}

impl SidebarProblem {
    fn of(problem: Problem) -> Self {
        Self {
            headline: problem.to_string().into(),
            detail: problem.detail().map(SharedString::from),
            problem,
        }
    }
}

pub struct SidebarRows {
    pub tour_agent: SidebarTourAgent,
    /// None until the review agent has left findings or a file that cannot be read.
    pub ai_review: Option<SidebarAiReview>,
    /// Only a basic review has one.
    pub guided_tour: Option<SidebarGuidedTour>,
    pub progress: ReadProgress,
    pub progress_unit: ReadUnit,
    pub reviews: Arc<SidebarReviews>,
    pub finish: SidebarFinish,
    /// Approve and Request changes show only on someone else's PR.
    pub finishing: Finishing,
    pub approve_warning: Option<ApproveWarning>,
    pub sync_banner: Option<SharedString>,
    pub not_synced: bool,
    /// What went wrong that the reviewer must know about, until it is put right.
    pub problems: Vec<SidebarProblem>,
    /// Changed code no page shows, named so the reviewer reads it on GitHub.
    pub changes_not_shown: Option<SharedString>,
    pub is_summary_current: bool,
    pub steps: Vec<SidebarStep>,
    pub safety_net: Option<SidebarSafetyNet>,
    /// Only a basic review has one; its steps are then empty.
    pub tree: Option<SidebarTree>,
    pub basic_mode_label: Option<&'static str>,
    /// What a local review is of, leading the sidebar where a PR has no such line.
    pub local_heading: Option<SharedString>,
    /// A local review's folder is not its snapshot any more: Changed on disk, another branch, or gone.
    pub disk_news: Option<SidebarDiskNews>,
    /// A local review's branch has an open PR, which the review offers to move to.
    pub move_offer: Option<SidebarMoveOffer>,
}

/// The lasting line that offers to move a local review to its PR, seen from either side.
pub struct SidebarMoveOffer {
    pub heading: SharedString,
    pub button: SharedString,
    pub keep: &'static str,
}

pub struct SidebarDiskNews {
    pub line: SharedString,
    pub offer: Option<DiskOffer>,
}

impl SidebarRows {
    pub fn of(review: &Review, tour_agent: &TourAgentState) -> Self {
        let progress = review.progress();
        let steps = review
            .sidebar()
            .into_iter()
            .map(|entry| SidebarStep {
                label: entry.title.to_owned().into(),
                thread_count: entry.thread_count,
                id: entry.id,
                depth: entry.depth,
                is_current: entry.is_current,
                is_ticked: entry.is_ticked,
                is_gone: entry.is_gone,
                has_check: entry.has_check,
                is_skim: entry.is_skim,
            })
            .collect();
        let safety_net = review.safety_net_size().map(|size| SidebarSafetyNet {
            pieces: size.pieces,
            thread_count: size.threads,
            is_current: review.screen() == &Screen::SafetyNet,
        });
        Self {
            ai_review: match review.ai_review() {
                AiReview::NotRun => None,
                ai_review => Some(SidebarAiReview {
                    line: ai_review.line().into(),
                    is_unreadable: matches!(ai_review, AiReview::Unreadable(_)),
                }),
            },
            tour_agent: SidebarTourAgent {
                is_shown: tour_agent.is_worth_a_line() || review.show_agent_failed(),
                line: tour_agent.line_for(review.subject()).into(),
                can_show: tour_agent.can_show(),
                can_retry: tour_agent.can_retry(),
                choices: tour_agent.choices().to_vec(),
            },
            guided_tour: review.basic_mode_label().map(|_| SidebarGuidedTour {
                stage: review.guided_tour_stage(),
                can_show_agent: tour_agent.can_show(),
                write: (review.guided_tour_progress() == &GuidedTourProgress::NotAsked).then(
                    || match review.write_guided_tour_refusal() {
                        None => WriteOffer::Available,
                        Some(refusal) => WriteOffer::Refused(refusal),
                    },
                ),
                banner: review.guided_tour_banner(),
            }),
            reviews: Arc::new(SidebarReviews::of(review)),
            finish: SidebarFinish {
                has_last_review: review.last_review().is_some(),
                can_mark_done: review.can_mark_done(),
                is_marked_done: review.is_marked_done(),
            },
            progress_unit: review.read_unit(),
            finishing: Finishing::of(review),
            approve_warning: review.approve_warning(),
            progress,
            sync_banner: review
                .sync_banner()
                .map(|reason| format!("Not syncing with GitHub: {reason}").into()),
            not_synced: review.not_synced(),
            changes_not_shown: changes_not_shown_notice(&review.changes_not_shown()),
            problems: review
                .problems()
                .into_iter()
                .map(SidebarProblem::of)
                .collect(),
            is_summary_current: review.screen() == &Screen::Summary,
            steps,
            safety_net,
            tree: review.basic_mode_label().map(|_| SidebarTree {
                is_overview_current: review.screen() == &Screen::Overview,
                rows: review.basic_sidebar(),
            }),
            basic_mode_label: review.basic_mode_label(),
            local_heading: review.local_heading().map(Into::into),
            move_offer: match review.branch_offer() {
                Some(BranchOffer::MoveToPr(offer)) => Some(SidebarMoveOffer {
                    heading: format!("{} is open from this branch", offer.pr.short()).into(),
                    button: offer.label().into(),
                    keep: "Keep local review",
                }),
                Some(BranchOffer::MoveHere(offer)) => Some(SidebarMoveOffer {
                    heading: format!(
                        "A local review of {} is on this machine",
                        offer.change_set.branch().as_str()
                    )
                    .into(),
                    button: offer.label().into(),
                    keep: "Keep it separate",
                }),
                Some(BranchOffer::OpenPr) | None => None,
            },
            disk_news: review
                .disk_news()
                .map(|DiskNews { line, offer }| SidebarDiskNews {
                    line: line.into(),
                    offer,
                }),
        }
    }
}

fn changes_not_shown_notice(ids: &[String]) -> Option<SharedString> {
    (!ids.is_empty()).then(|| {
        format!(
            "Changed code not shown here, read it on GitHub: {}",
            ids.join(", ")
        )
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_code_no_page_shows_is_named_in_the_sidebar() {
        let ids = ["app/Ghost.php#Ghost::run".to_owned()];

        let notice = changes_not_shown_notice(&ids).unwrap();

        assert!(notice.contains("app/Ghost.php#Ghost::run"));
        assert_eq!(changes_not_shown_notice(&[]), None);
    }
}
