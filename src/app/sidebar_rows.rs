//! What the sidebar shows, read from the review once per change (with `PageRows`), so a frame, which
//! comes on every scroll step, asks the review nothing.

use gpui::SharedString;

use crate::viewer::{ApproveWarning, ReadProgress, Review, Screen, StepId};

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

pub struct SidebarRows {
    pub progress: ReadProgress,
    /// Approve and Request changes show only on someone else's PR.
    pub can_judge: bool,
    pub approve_warning: Option<ApproveWarning>,
    pub sync_banner: Option<SharedString>,
    pub not_synced: bool,
    /// Ticks and typed text are only in memory until a save goes through again.
    pub save_problem: Option<SharedString>,
    /// Changed code no page shows, named so the reviewer reads it on GitHub.
    pub changes_not_shown: Option<SharedString>,
    pub is_summary_current: bool,
    pub steps: Vec<SidebarStep>,
    pub safety_net: Option<SidebarSafetyNet>,
}

impl SidebarRows {
    pub fn of(review: &Review) -> Self {
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
        Self {
            can_judge: review.can_approve_or_request_changes(),
            approve_warning: review.approve_warning(),
            progress,
            sync_banner: review
                .sync_banner()
                .map(|reason| format!("Not syncing with GitHub: {reason}").into()),
            not_synced: review.not_synced(),
            changes_not_shown: changes_not_shown_notice(&review.changes_not_shown()),
            save_problem: review
                .save_problem()
                .map(|reason| format!("Not saved to disk: {reason}").into()),
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
