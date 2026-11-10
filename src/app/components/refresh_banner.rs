use gpui::{
    App, ClickEvent, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

use crate::viewer::RefreshBrief;

use super::button::{ButtonKind, ButtonSize, button};
use super::chip::{dropping_in, notice_band};
use super::palette::{Colors, Tone};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshProgress {
    Idle,
    Running,
    /// The reason is a problem of the review, shown in the sidebar.
    Failed,
}

type OnRefresh = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Offers the AI Refresh of a re-pinned tour. Nothing runs until the button is clicked,
/// and a redo is only ever suggested, never started.
#[derive(IntoElement)]
pub struct RefreshBanner {
    brief: RefreshBrief,
    progress: RefreshProgress,
    on_refresh: OnRefresh,
}

impl RefreshBanner {
    pub fn new(
        brief: RefreshBrief,
        progress: RefreshProgress,
        on_refresh: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            brief,
            progress,
            on_refresh: Box::new(on_refresh),
        }
    }
}

impl RenderOnce for RefreshBanner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let message = match &self.progress {
            RefreshProgress::Running => "Refreshing the tour with your claude…".to_owned(),
            RefreshProgress::Idle | RefreshProgress::Failed => format!(
                "{} to rewrite, {} to place since the new commits.",
                counted(self.brief.stale_steps.len(), "step", "steps"),
                counted(
                    self.brief.unplaced_new_pieces.len(),
                    "new piece",
                    "new pieces"
                ),
            ),
        };
        let button_label = match &self.progress {
            RefreshProgress::Failed => "Try again",
            RefreshProgress::Idle | RefreshProgress::Running => "Refresh",
        };
        let button = (self.progress != RefreshProgress::Running).then(|| {
            button(
                &colors,
                "refresh-tour",
                ButtonKind::Primary,
                ButtonSize::Small,
            )
            .on_click(self.on_refresh)
            .child(button_label)
        });
        let redo_hint = self.brief.redo_hint().map(|hint| {
            div().child(SharedString::from(format!(
                "{hint}. A full redo may read better: `/tour --redo` in claude."
            )))
        });
        let failed = (self.progress == RefreshProgress::Failed).then(|| {
            div()
                .text_color(colors.danger)
                .child("The last refresh did not go through, the sidebar says why.")
        });
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
            .children(redo_hint)
            .children(failed);
        dropping_in(band, "refresh-banner")
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
