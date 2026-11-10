use gpui::{
    App, ClickEvent, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

use crate::pull_request::Sha;
use crate::viewer::OldHead;

use super::button::{ButtonKind, ButtonSize, icon_label_button};
use super::chip::{dropping_in, notice_band};
use super::icon::Icon;
use super::palette::{Colors, Tone};

type OnRepin = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepinProgress {
    Idle,
    Running,
    Failed(String),
}

/// Offers to move the tour to the PR's new commits. Nothing moves until the button is clicked.
#[derive(IntoElement)]
pub struct NewCommitsBanner {
    old_head: OldHead,
    progress: RepinProgress,
    on_repin: OnRepin,
}

impl NewCommitsBanner {
    pub fn new(
        old_head: OldHead,
        progress: RepinProgress,
        on_repin: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            old_head,
            progress,
            on_repin: Box::new(on_repin),
        }
    }
}

impl RenderOnce for NewCommitsBanner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let message = match &self.progress {
            RepinProgress::Running => "Updating the tour…".to_owned(),
            RepinProgress::Idle | RepinProgress::Failed(_) => format!(
                "The PR has new commits ({} → {}). Update the tour to comment.",
                short(&self.old_head.tour_head),
                short(&self.old_head.pr_head)
            ),
        };
        let button_label = match &self.progress {
            RepinProgress::Failed(_) => "Try again",
            RepinProgress::Idle | RepinProgress::Running => "Update tour",
        };
        let button = (self.progress != RepinProgress::Running).then(|| {
            icon_label_button(
                &colors,
                "update-tour",
                ButtonKind::Secondary,
                ButtonSize::Small,
                Icon::Refresh,
                button_label,
            )
            .on_click(self.on_repin)
        });
        let error = match &self.progress {
            RepinProgress::Failed(error) => Some(
                div()
                    .text_color(colors.danger)
                    .child(SharedString::from(error.clone())),
            ),
            RepinProgress::Idle | RepinProgress::Running => None,
        };
        let band = notice_band(&colors, Tone::Warning)
            .py_2()
            .flex_col()
            .items_start()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(message)
                    .children(button),
            )
            .children(error);
        dropping_in(band, "new-commits-banner")
    }
}

fn short(sha: &Sha) -> &str {
    sha.as_str().get(..7).unwrap_or(sha.as_str())
}
