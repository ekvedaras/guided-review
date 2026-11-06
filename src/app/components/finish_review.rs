use std::rc::Rc;

use gpui::{
    App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{ApproveWarning, CLAMPED_BODY_LINES, FinishResult, ReviewVerdict};

use crate::app::sidebar_rows::SidebarLastReview;

use super::button::{ButtonKind, ButtonSize, button, toggle_button};
use super::palette::Colors;
use super::text_box::TextBox;
use super::text_input::Lines;

/// Sends the review and calls back with how it went. The layout owns the review and the network.
pub type OnFinish =
    Rc<dyn Fn(ReviewVerdict, String, Box<dyn FnOnce(&FinishResult, &mut App)>, &mut App)>;

/// Hands the review body to the core, which saves it, each time it changes.
pub type OnBodyEdit = Rc<dyn Fn(String, &mut App)>;

/// What the reviewer is doing in the finish dialog. The body is not here: the core keeps it.
struct FinishUi {
    is_open: bool,
    is_last_review_body_open: bool,
    verdict: ReviewVerdict,
    /// The reviewer saw the unread warning and has to say yes to it once more.
    is_confirming_approval: bool,
    is_busy: bool,
    problem: Option<String>,
    focus: FocusHandle,
}

impl FinishUi {
    fn new(has_draft: bool, cx: &mut Context<Self>) -> Self {
        Self {
            is_open: has_draft,
            is_last_review_body_open: false,
            verdict: ReviewVerdict::Comment,
            is_confirming_approval: false,
            is_busy: false,
            problem: None,
            focus: cx.focus_handle(),
        }
    }
}

/// The "Finish review" button and the form it opens: Comment, Approve or Request changes, with a body.
#[derive(IntoElement)]
pub struct FinishReviewDialog {
    /// Then finishing is what the reader does next, so its button is the primary one.
    is_everything_read: bool,
    can_judge: bool,
    warning: Option<ApproveWarning>,
    /// A review of the viewer's is on GitHub already, so this one updates it.
    last_review: Option<SidebarLastReview>,
    body: String,
    on_body_edit: OnBodyEdit,
    on_finish: OnFinish,
}

impl FinishReviewDialog {
    pub fn new(
        is_everything_read: bool,
        can_judge: bool,
        warning: Option<ApproveWarning>,
        last_review: Option<SidebarLastReview>,
        body: String,
        on_body_edit: OnBodyEdit,
        on_finish: OnFinish,
    ) -> Self {
        Self {
            is_everything_read,
            can_judge,
            warning,
            last_review,
            body,
            on_body_edit,
            on_finish,
        }
    }
}

fn label(verdict: ReviewVerdict) -> &'static str {
    match verdict {
        ReviewVerdict::Comment => "Comment",
        ReviewVerdict::Approve => "Approve",
        ReviewVerdict::RequestChanges => "Request changes",
    }
}

fn update(ui: &Entity<FinishUi>, cx: &mut App, change: impl FnOnce(&mut FinishUi)) {
    ui.update(cx, |ui, cx| {
        change(ui);
        cx.notify();
    });
}

fn submit(ui: &Entity<FinishUi>, body: String, on_finish: &OnFinish, cx: &mut App) {
    let verdict = ui.read(cx).verdict;
    update(ui, cx, |ui| {
        ui.is_busy = true;
        ui.problem = None;
    });
    let done_ui = ui.clone();
    on_finish(
        verdict,
        body,
        Box::new(move |result, cx| {
            update(&done_ui, cx, |ui| {
                ui.is_busy = false;
                ui.is_confirming_approval = false;
                match result {
                    FinishResult::Sent => {
                        ui.is_open = false;
                        ui.verdict = ReviewVerdict::Comment;
                    }
                    FinishResult::Refused(refusal) => ui.problem = Some(refusal.to_string()),
                    FinishResult::Failed(reason) => ui.problem = Some(reason.clone()),
                }
            });
        }),
        cx,
    );
}

fn body_box(
    ui: &Entity<FinishUi>,
    focus: &FocusHandle,
    body: &str,
    on_body_edit: &OnBodyEdit,
) -> TextBox {
    let (on_cancel, on_body_edit) = (ui.clone(), Rc::clone(on_body_edit));
    TextBox::new("finish-review-body-text", focus, body, Lines::Multi)
        .on_edit(move |body, _, cx| on_body_edit(body, cx))
        .on_cancel(move |_, cx| update(&on_cancel, cx, |ui| ui.is_open = false))
}

impl RenderOnce for FinishReviewDialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let ui = window.use_keyed_state("finish-review-ui", cx, {
            let has_draft = !self.body.is_empty();
            move |_, cx| FinishUi::new(has_draft, cx)
        });
        let state = ui.read(cx);
        let (is_open, verdict, is_busy, is_confirming) = (
            state.is_open,
            state.verdict,
            state.is_busy,
            state.is_confirming_approval,
        );
        let is_last_review_body_open = state.is_last_review_body_open;
        let (problem, focus) = (state.problem.clone(), state.focus.clone());
        let body = if is_busy {
            div().child(self.body.clone()).into_any_element()
        } else {
            body_box(&ui, &focus, &self.body, &self.on_body_edit).into_any_element()
        };

        let open_button = {
            let (ui, focus) = (ui.clone(), focus);
            let open_button = if is_open {
                toggle_button(&colors, "finish-review", true, ButtonSize::Regular)
            } else if self.is_everything_read {
                button(
                    &colors,
                    "finish-review",
                    ButtonKind::Primary,
                    ButtonSize::Regular,
                )
            } else {
                button(
                    &colors,
                    "finish-review",
                    ButtonKind::Secondary,
                    ButtonSize::Regular,
                )
            };
            open_button
                .justify_center()
                .child(if self.last_review.is_some() {
                    "Update review"
                } else {
                    "Finish review"
                })
                .on_click(move |_, window, cx| {
                    update(&ui, cx, |ui| ui.is_open = !ui.is_open);
                    window.focus(&focus, cx);
                })
        };
        let last_review = self.last_review.as_ref().map(|last| {
            let headline_color = if last.is_old {
                colors.warning
            } else {
                colors.muted_text
            };
            div()
                .id("last-review")
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .child(
                    div()
                        .text_color(headline_color)
                        .child(last.headline.clone()),
                )
                .children((!last.body.is_empty()).then(|| {
                    let body = div().text_color(colors.muted_text).child(last.body.clone());
                    if last.is_body_long && !is_last_review_body_open {
                        body.line_clamp(CLAMPED_BODY_LINES)
                    } else {
                        body
                    }
                }))
                .children(last.is_body_long.then(|| {
                    let ui = ui.clone();
                    div()
                        .id("last-review-body-toggle")
                        .cursor_pointer()
                        .text_color(colors.muted_text)
                        .on_click(move |_, _, cx| {
                            update(&ui, cx, |ui| {
                                ui.is_last_review_body_open = !ui.is_last_review_body_open;
                            });
                        })
                        .child(if is_last_review_body_open {
                            "▾ Show less"
                        } else {
                            "▸ Show all"
                        })
                }))
        });
        let wrapper = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(open_button)
            .children(last_review);
        if !is_open {
            return wrapper.into_any_element();
        }

        let verdicts = [
            ReviewVerdict::Comment,
            ReviewVerdict::Approve,
            ReviewVerdict::RequestChanges,
        ]
        .into_iter()
        .filter(|verdict| self.can_judge || *verdict == ReviewVerdict::Comment)
        .map(|choice| {
            let ui = ui.clone();
            toggle_button(
                &colors,
                SharedString::from(format!("verdict-{}", choice.github_name())),
                choice == verdict,
                ButtonSize::Small,
            )
            .child(label(choice))
            .on_click(move |_, _, cx| {
                update(&ui, cx, |ui| {
                    ui.verdict = choice;
                    ui.is_confirming_approval = false;
                });
            })
        });

        let unread_warning = (verdict == ReviewVerdict::Approve)
            .then_some(self.warning)
            .flatten();
        let must_confirm = unread_warning.is_some() && !is_confirming;
        let submit_button = {
            let (ui, on_finish, text) = (ui.clone(), Rc::clone(&self.on_finish), self.body.clone());
            button(
                &colors,
                "submit-review",
                ButtonKind::Primary,
                ButtonSize::Regular,
            )
            .justify_center()
            .child(if must_confirm {
                "Approve..."
            } else {
                "Submit review"
            })
            .on_click(move |_: &ClickEvent, _, cx| {
                if must_confirm {
                    update(&ui, cx, |ui| ui.is_confirming_approval = true);
                } else {
                    submit(&ui, text.clone(), &on_finish, cx);
                }
            })
        };
        let warning_text = unread_warning.filter(|_| is_confirming).map(|warning| {
            div().text_sm().text_color(colors.danger).child(format!(
                "Not read yet: {} step{} and {} safety net piece{}. Approve anyway?",
                warning.unread_steps,
                if warning.unread_steps == 1 { "" } else { "s" },
                warning.unread_safety_net_pieces,
                if warning.unread_safety_net_pieces == 1 {
                    ""
                } else {
                    "s"
                },
            ))
        });
        wrapper
            .child(div().flex().flex_wrap().gap_1().children(verdicts))
            .child(
                div()
                    .id("finish-review-body")
                    .min_h(px(64.))
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .text_sm()
                    .child(body),
            )
            .children(warning_text)
            .children(
                problem.map(|problem| div().text_sm().text_color(colors.danger).child(problem)),
            )
            .child(if is_busy {
                div()
                    .text_sm()
                    .text_color(colors.muted_text)
                    .child("Sending...")
                    .into_any_element()
            } else {
                submit_button.into_any_element()
            })
            .into_any_element()
    }
}
