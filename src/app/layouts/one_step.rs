use gpui::{
    AnyElement, App, Context, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, actions, div,
    px,
};

use crate::app::components::diff_view::DiffView;
use crate::app::components::palette::{Palette, TEXT_FONT};
use crate::viewer::{Review, Screen, StepId, StepPage};

actions!(one_step, [GoForward, GoBack]);

/// The one layout: step tree on the left, one screen on the right (the summary, or one step).
pub struct OneStepLayout {
    review: Review,
    focus: FocusHandle,
}

impl OneStepLayout {
    pub const KEY_CONTEXT: &str = "OneStepLayout";

    pub fn new(review: Review, cx: &mut Context<Self>) -> Self {
        Self {
            review,
            focus: cx.focus_handle(),
        }
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.review.go_forward() {
            cx.notify();
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.review.go_back() {
            cx.notify();
        }
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let summary_row = div()
            .id("summary")
            .px_3()
            .py_1()
            .cursor_pointer()
            .when_current(self.review.screen() == &Screen::Summary)
            .on_click(cx.listener(|this, _, _, cx| {
                this.review.go_to_summary();
                cx.notify();
            }))
            .child("Summary");
        let step_rows = self.review.sidebar().into_iter().map(|entry| {
            let id: StepId = entry.id.clone();
            div()
                .id(SharedString::from(format!("step-{id}")))
                .pl(px(12. + 16. * entry.depth as f32))
                .pr_3()
                .py_1()
                .cursor_pointer()
                .when_current(entry.is_current)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.review.go_to(&id);
                    cx.notify();
                }))
                .child(entry.title.to_string())
        });
        div()
            .id("sidebar")
            .w(px(280.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .py_2()
            .bg(Palette::sidebar())
            .border_r_1()
            .border_color(Palette::border())
            .text_sm()
            .child(summary_row)
            .children(step_rows)
    }

    fn screen(&self) -> AnyElement {
        match self.review.current_step() {
            None => summary_screen(self.review.title(), self.review.summary()).into_any_element(),
            Some(page) => step_screen(page).into_any_element(),
        }
    }
}

trait CurrentRow {
    fn when_current(self, is_current: bool) -> Self;
}

impl<E: Styled> CurrentRow for E {
    fn when_current(self, is_current: bool) -> Self {
        if is_current {
            self.bg(Palette::current())
        } else {
            self
        }
    }
}

fn summary_screen(title: &str, summary: &[String]) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_2xl().child(title.to_string()))
        .children(
            summary
                .iter()
                .map(|bullet| div().child(format!("• {bullet}"))),
        )
}

fn step_screen(page: StepPage<'_>) -> impl IntoElement {
    let step = page.step;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_xl().child(step.title.clone()))
        .child(
            div()
                .text_color(Palette::muted_text())
                .child(step.why.clone()),
        )
        .children(page.pieces.into_iter().map(DiffView::new))
}

impl Focusable for OneStepLayout {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for OneStepLayout {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::go_back))
            .size_full()
            .flex()
            .bg(Palette::background())
            .font_family(TEXT_FONT)
            .text_color(Palette::text())
            .child(self.sidebar(cx))
            .child(
                div()
                    .id("screen")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .p_6()
                    .child(self.screen()),
            )
    }
}
