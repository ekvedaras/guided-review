//! What the sidebar shows, read from the review once per change (with `PageRows`), so a frame, which
//! comes on every scroll step, asks the review nothing.

use crate::app::components::finish_review::Finishing;
use gpui::SharedString;

use crate::viewer::{ApproveWarning, ReadProgress, Review, Screen, StepId, TourAgentState};

pub struct SidebarStep {
    pub id: StepId,
    pub label: SharedString,
    pub thread_count: usize,
    pub depth: usize,
    pub is_current: bool,
    pub is_ticked: bool,
    /// Every piece of the step went away in a re-pin.
    pub is_gone: bool,
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
    pub line: SharedString,
    pub can_show: bool,
    /// Why the last Show agent did not bring the terminal up.
    pub show_failed: Option<SharedString>,
}

pub struct SidebarRows {
    pub tour_agent: SidebarTourAgent,
    pub progress: ReadProgress,
    pub last_review: Option<SidebarLastReview>,
    /// Approve and Request changes show only on someone else's PR.
    pub finishing: Finishing,
    pub approve_warning: Option<ApproveWarning>,
    pub sync_banner: Option<SharedString>,
    pub not_synced: bool,
    /// What went wrong that the reviewer must know about, until it is put right.
    pub problems: Vec<SharedString>,
    /// Changed code no page shows, named so the reviewer reads it on GitHub.
    pub changes_not_shown: Option<SharedString>,
    pub is_summary_current: bool,
    pub steps: Vec<SidebarStep>,
    pub safety_net: Option<SidebarSafetyNet>,
}

impl SidebarRows {
    pub fn of(review: &Review, tour_agent: &TourAgentState, show_failed: Option<&str>) -> Self {
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
                line: tour_agent.to_string().into(),
                can_show: tour_agent.can_show(),
                show_failed: show_failed
                    .map(|reason| format!("Could not show the agent: {reason}").into()),
            },
            last_review: review.last_review().map(|last| SidebarLastReview {
                headline: last.headline(is_old).into(),
                body: last.body.clone().into(),
                is_body_long: last.body_is_long(),
                is_old,
            }),
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
                .iter()
                .map(|problem| problem.to_string().into())
                .collect(),
            is_summary_current: review.screen() == &Screen::Summary,
            steps,
            safety_net,
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
