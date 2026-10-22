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
        let progress = self.review.progress();
        let progress_row = div()
            .px_3()
            .pb_2()
            .text_color(Palette::muted_text())
            .child(format!("Read {} of {}", progress.read, progress.total));
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
                .when_ticked(entry.is_ticked)
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
            .child(progress_row)
            .child(summary_row)
            .children(step_rows)
    }

    fn screen(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.review.current_step() {
            None => summary_screen(self.review.title(), self.review.summary()).into_any_element(),
            Some(page) => step_screen(page, cx).into_any_element(),
        }
    }

    fn toggle_tick(&mut self, id: &StepId, cx: &mut Context<Self>) {
        let saved = if self.review.is_ticked(id) {
            self.review.untick(id)
        } else {
            self.review.tick(id)
        };
        if let Err(error) = saved {
            eprintln!("could not save the read tick: {error:#}");
        }
        cx.notify();
    }
}

trait TickedRow {
    fn when_ticked(self, is_ticked: bool) -> Self;
}

impl<E: Styled> TickedRow for E {
    fn when_ticked(self, is_ticked: bool) -> Self {
        if is_ticked {
            self.text_color(Palette::muted_text())
        } else {
            self
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

fn step_screen(page: StepPage<'_>, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let step = page.step;
    let screen = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(step_header(&page, cx));
    if page.is_folded {
        return screen.child(
            div()
                .text_sm()
                .text_color(Palette::muted_text())
                .child("Folded because it is read. Click the title to open it."),
        );
    }
    screen
        .child(
            div()
                .text_color(Palette::muted_text())
                .child(step.why.clone()),
        )
        .children(page.pieces.into_iter().map(DiffView::new))
}

fn step_header(page: &StepPage<'_>, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let id = StepId::from(page.step.id.as_str());
    let title = div()
        .id("step-title")
        .flex_1()
        .text_xl()
        .child(page.step.title.clone());
    let title = if page.is_ticked {
        let id = id.clone();
        title
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.review.toggle_fold(&id);
                cx.notify();
            }))
    } else {
        title
    };
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(title)
        .child(tick_button(page.is_ticked, id, cx))
}

fn tick_button(is_ticked: bool, id: StepId, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    div()
        .id("tick")
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .cursor_pointer()
        .text_sm()
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tick(&id, cx)))
        .child(if is_ticked {
            "✓ Read"
        } else {
            "Mark as read"
        })
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
                    .child(self.screen(cx)),
            )
    }
}
