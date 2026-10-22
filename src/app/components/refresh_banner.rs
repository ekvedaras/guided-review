use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

use crate::viewer::RefreshBrief;

use super::palette::Colors;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshProgress {
    Idle,
    Running,
    Failed(String),
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
            RefreshProgress::Running => "Refreshing the tour with your claude…".to_string(),
            RefreshProgress::Idle | RefreshProgress::Failed(_) => format!(
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
            RefreshProgress::Failed(_) => "Try again",
            RefreshProgress::Idle | RefreshProgress::Running => "Refresh",
        };
        let button = (self.progress != RefreshProgress::Running).then(|| {
            div()
                .id("refresh-tour")
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .cursor_pointer()
                .on_click(self.on_refresh)
                .child(button_label)
        });
        let redo_hint = self.brief.redo_hint().map(|hint| {
            div().child(SharedString::from(format!(
                "{hint}. A full redo may read better: `/tour --redo` in claude."
            )))
        });
        let error = match &self.progress {
            RefreshProgress::Failed(error) => Some(div().child(SharedString::from(error.clone()))),
            RefreshProgress::Idle | RefreshProgress::Running => None,
        };
        div()
            .px_6()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .bg(colors.current)
            .border_b_1()
            .border_color(colors.border)
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(message)
                    .children(button),
            )
            .children(redo_hint)
            .children(error)
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
