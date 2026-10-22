use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

use crate::pull_request::Sha;
use crate::viewer::OldHead;

use super::palette::Palette;

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
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let message = match &self.progress {
            RepinProgress::Running => "Updating the tour…".to_string(),
            RepinProgress::Idle | RepinProgress::Failed(_) => format!(
                "New commits. The tour is for {}, the PR is now at {}. Update the tour to comment.",
                short(&self.old_head.tour_head),
                short(&self.old_head.pr_head)
            ),
        };
        let button_label = match &self.progress {
            RepinProgress::Failed(_) => "Try again",
            RepinProgress::Idle | RepinProgress::Running => "Update tour",
        };
        let button = (self.progress != RepinProgress::Running).then(|| {
            div()
                .id("update-tour")
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(Palette::border())
                .cursor_pointer()
                .on_click(self.on_repin)
                .child(button_label)
        });
        let error = match &self.progress {
            RepinProgress::Failed(error) => Some(div().child(SharedString::from(error.clone()))),
            RepinProgress::Idle | RepinProgress::Running => None,
        };
        div()
            .px_6()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .bg(Palette::current())
            .border_b_1()
            .border_color(Palette::border())
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(message)
                    .children(button),
            )
            .children(error)
    }
}

fn short(sha: &Sha) -> &str {
    sha.as_str().get(..7).unwrap_or(sha.as_str())
}
