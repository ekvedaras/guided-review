use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

use crate::pull_request::Sha;
use crate::viewer::OldHead;

use super::palette::Palette;

type OnUpdate = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TourUpdate {
    Idle,
    Running,
    Failed(String),
}

/// Offers to move the tour to the PR's new commits. Nothing moves until the button is clicked.
#[derive(IntoElement)]
pub struct NewCommitsBanner {
    old_head: OldHead,
    update: TourUpdate,
    on_update: OnUpdate,
}

impl NewCommitsBanner {
    pub fn new(
        old_head: OldHead,
        update: TourUpdate,
        on_update: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            old_head,
            update,
            on_update: Box::new(on_update),
        }
    }
}

impl RenderOnce for NewCommitsBanner {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let message = match &self.update {
            TourUpdate::Running => "Updating the tour…".to_string(),
            TourUpdate::Idle | TourUpdate::Failed(_) => format!(
                "New commits. The tour is for {}, the PR is now at {}. Update the tour to comment.",
                short(&self.old_head.tour_head),
                short(&self.old_head.pr_head)
            ),
        };
        let button_label = match &self.update {
            TourUpdate::Failed(_) => "Try again",
            TourUpdate::Idle | TourUpdate::Running => "Update tour",
        };
        let button = (self.update != TourUpdate::Running).then(|| {
            div()
                .id("update-tour")
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(Palette::border())
                .cursor_pointer()
                .on_click(self.on_update)
                .child(button_label)
        });
        let error = match &self.update {
            TourUpdate::Failed(error) => Some(div().child(SharedString::from(error.clone()))),
            TourUpdate::Idle | TourUpdate::Running => None,
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
