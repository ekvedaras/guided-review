//! What the sidebar shows, read from the review once per change (with `PageRows`), so a frame, which
//! comes on every scroll step, asks the review nothing.

use crate::app::components::finish_review::Finishing;
use gpui::SharedString;

use crate::viewer::{
    ApproveWarning, DiskNews, DiskOffer, GuidedTourBanner, GuidedTourProgress, GuidedTourRefusal,
    GuidedTourStage, MoveOffer, Problem, ReadProgress, ReadUnit, Review, Screen, StepId,
    TerminalId, TourAgentState, TreeRow,
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

/// How the viewer's last review on GitHub ended, shown under the finish button.
#[derive(Clone)]
pub struct SidebarLastReview {
    pub headline: SharedString,
    pub body: SharedString,
    pub is_body_long: bool,
    pub is_old: bool,
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
    /// Only a basic review has one.
    pub guided_tour: Option<SidebarGuidedTour>,
    pub progress: ReadProgress,
    pub progress_unit: ReadUnit,
    pub last_review: Option<SidebarLastReview>,
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
    pub move_offer: Option<MoveOffer>,
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
        let is_old = review.last_review_is_old();
        Self {
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
            last_review: review.last_review().map(|last| SidebarLastReview {
                headline: last.headline(is_old).into(),
                body: last.body.clone().into(),
                is_body_long: last.body_is_long(),
                is_old,
            }),
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
            move_offer: review.move_offer(),
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
