use gpui::{
    App, ClickEvent, IntoElement, ParentElement, RenderOnce, StatefulInteractiveElement, Styled,
    Window, div,
};

use crate::viewer::{JobProgress, RefreshBrief};

use super::button::{ButtonKind, ButtonSize, icon_label_button};
use super::chip::{dropping_in, notice_band};
use super::icon::Icon;
use super::motion::busy_dot;
use super::palette::{Colors, Tone};
use super::selectable_text::SelectableText;

type OnRefresh = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Offers the AI Refresh of a re-pinned tour. Nothing runs until the button is clicked,
/// and a redo is only ever suggested, never started.
#[derive(IntoElement)]
pub struct RefreshBanner {
    brief: RefreshBrief,
    progress: JobProgress,
    on_refresh: OnRefresh,
}

impl RefreshBanner {
    pub fn new(
        brief: RefreshBrief,
        progress: JobProgress,
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
            JobProgress::Running { .. } => "Refreshing the tour with the tour agent…".to_owned(),
            JobProgress::Idle | JobProgress::Failed(_) => format!(
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
            JobProgress::Failed(_) => "Try again",
            JobProgress::Idle | JobProgress::Running { .. } => "Refresh",
        };
        let activity = match &self.progress {
            JobProgress::Running {
                activity: Some(activity),
            } => Some(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(colors.muted_text)
                    .child(busy_dot(&colors, "refresh-activity"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(format!("{activity}...")),
                    ),
            ),
            JobProgress::Idle | JobProgress::Running { .. } | JobProgress::Failed(_) => None,
        };
        let is_running = matches!(self.progress, JobProgress::Running { .. });
        let button = (!is_running).then(|| {
            icon_label_button(
                &colors,
                "refresh-tour",
                ButtonKind::Secondary,
                ButtonSize::Small,
                Icon::Refresh,
                button_label,
            )
            .on_click(self.on_refresh)
        });
        let redo_hint = self.brief.redo_hint().filter(|_| !is_running).map(|hint| {
            div().child(SelectableText::new(
                "refresh-redo-hint",
                format!("{hint}. A full redo may read better: `/tour --redo` in claude."),
            ))
        });
        // The reason is a problem of the review, which the sidebar lists.
        let failed = matches!(self.progress, JobProgress::Failed(_)).then(|| {
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
            .children(activity)
            .children(redo_hint)
            .children(failed);
        dropping_in(band, "refresh-banner")
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
