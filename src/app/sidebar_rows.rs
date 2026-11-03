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
                label: entry.title.to_string().into(),
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
            is_summary_current: review.screen() == &Screen::Summary,
            steps,
            safety_net,
        }
    }
}
