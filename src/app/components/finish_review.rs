use std::rc::Rc;

use gpui::{
    App, ClickEvent, Context, Entity, FocusHandle, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use gpui::prelude::FluentBuilder;

use crate::viewer::{ApproveWarning, FinishResult, ReviewVerdict};

use crate::app::sidebar_rows::SidebarFinish;

use super::button::{ButtonKind, ButtonSize, button, button_icon};
use super::icon::Icon;
use super::motion::{BOX_RISE, Edge, STATE, busy_dot, slide_in};
use super::palette::{Colors, Tone};
use super::segmented::{Segment, SegmentLayout, Segmented};
use super::text_box::{Field, TextBox};
use super::text_input::Lines;

/// Sends the review and calls back with how it went. The layout owns the review and the network.
pub type OnFinish =
    Rc<dyn Fn(ReviewVerdict, String, Box<dyn FnOnce(&FinishResult, &mut App)>, &mut App)>;

/// Hands the review body to the core, which saves it, each time it changes.
pub type OnBodyEdit = Rc<dyn Fn(String, &mut App)>;

/// Marks a re-review done, which the layout carries out as the review's intent.
pub type OnMarkDone = Rc<dyn Fn(&mut Window, &mut App)>;

/// What the reviewer is doing in the finish dialog. The body is not here: the core keeps it.
struct FinishUi {
    is_open: bool,
    verdict: ReviewVerdict,
    /// The reviewer saw the unread warning and has to say yes to it once more.
    is_confirming_approval: bool,
    is_busy: bool,
    problem: Option<String>,
    focus: FocusHandle,
    /// The last ask to open the form from elsewhere (the palette, the finish card) it answered.
    answered_open_request: u32,
}

impl FinishUi {
    fn new(has_draft: bool, open_request: u32, cx: &mut Context<Self>) -> Self {
        Self {
            answered_open_request: open_request,
            is_open: has_draft,
            verdict: ReviewVerdict::Comment,
            is_confirming_approval: false,
            is_busy: false,
            problem: None,
            focus: cx.focus_handle(),
        }
    }
}

/// Which ways of finishing the review the PR allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finishing {
    /// The viewer's own PR with no pending comments: there is nothing to send and no verdict to give.
    NothingToFinish,
    CommentOnly,
    AnyVerdict,
}

impl Finishing {
    pub fn of(review: &crate::viewer::Review) -> Self {
        if review.can_approve_or_request_changes() {
            Self::AnyVerdict
        } else if review.can_finish_review() {
            Self::CommentOnly
        } else {
            Self::NothingToFinish
        }
    }
}

/// The "Finish review" button and the form it opens: Comment, Approve or Request changes, with a body.
#[derive(IntoElement)]
pub struct FinishReviewDialog {
    /// Then finishing is what the reader does next, so its button is the primary one.
    is_everything_read: bool,
    finishing: Finishing,
    warning: Option<ApproveWarning>,
    finish: SidebarFinish,
    body: String,
    on_body_edit: OnBodyEdit,
    on_finish: OnFinish,
    /// Offered only with this, and only on a re-review with nothing new to send.
    on_mark_done: Option<OnMarkDone>,
    open_request: u32,
}

impl FinishReviewDialog {
    pub fn new(
        is_everything_read: bool,
        finishing: Finishing,
        warning: Option<ApproveWarning>,
        finish: SidebarFinish,
        body: String,
        on_body_edit: OnBodyEdit,
        on_finish: OnFinish,
    ) -> Self {
        Self {
            is_everything_read,
            finishing,
            warning,
            finish,
            body,
            on_body_edit,
            on_finish,
            on_mark_done: None,
            open_request: 0,
        }
    }

    pub fn on_mark_done(mut self, on_mark_done: OnMarkDone) -> Self {
        self.on_mark_done = Some(on_mark_done);
        self
    }

    /// A new count opens the form and puts the keys in its body box.
    pub fn opened_by(mut self, open_request: u32) -> Self {
        self.open_request = open_request;
        self
    }
}

fn icon_of(verdict: ReviewVerdict) -> Icon {
    match verdict {
        ReviewVerdict::Comment => Icon::MessageSquare,
        ReviewVerdict::Approve => Icon::CircleCheck,
        ReviewVerdict::RequestChanges => Icon::FilePen,
    }
}

/// The picked verdict's thumb wears what it tells the author.
fn tone_of(verdict: ReviewVerdict) -> Tone {
    match verdict {
        ReviewVerdict::Comment => Tone::Neutral,
        ReviewVerdict::Approve => Tone::Success,
        ReviewVerdict::RequestChanges => Tone::Danger,
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
    fn render(
        #[cfg_attr(
            not(feature = "snapshots"),
            expect(unused_mut, reason = "staged below")
        )]
        mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let colors = Colors::active(cx);
        let ui = window.use_keyed_state("finish-review-ui", cx, {
            let (has_draft, open_request) = (!self.body.is_empty(), self.open_request);
            move |_, cx| FinishUi::new(has_draft, open_request, cx)
        });
        if ui.read(cx).answered_open_request != self.open_request {
            let open_request = self.open_request;
            update(&ui, cx, |ui| {
                ui.is_open = true;
                ui.answered_open_request = open_request;
            });
            let focus = ui.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        // The demo PR is the maintainer's own, which offers no finishing; the picture needs the form.
        #[cfg(feature = "snapshots")]
        if let Some(staged) = cx.try_global::<crate::app::snapshots::StagedFinishReview>() {
            let is_open = staged.0;
            if is_open {
                self.finishing = Finishing::AnyVerdict;
            }
            if is_open != ui.read(cx).is_open {
                update(&ui, cx, |ui| ui.is_open = is_open);
            }
        }
        let state = ui.read(cx);
        let (is_open, verdict, is_busy, is_confirming) = (
            state.is_open,
            state.verdict,
            state.is_busy,
            state.is_confirming_approval,
        );
        let (problem, focus) = (state.problem.clone(), state.focus.clone());
        let focus_of_body = focus.clone();
        let body = if is_busy {
            div().child(self.body.clone()).into_any_element()
        } else {
            body_box(&ui, &focus, &self.body, &self.on_body_edit).into_any_element()
        };

        let finish = self.finish;
        let can_approve_again = (finish.can_mark_done || finish.is_marked_done)
            && self.finishing == Finishing::AnyVerdict;
        let is_finish_the_way_on =
            self.is_everything_read && !finish.can_mark_done && !finish.is_marked_done;
        let open_button = {
            let (ui, focus) = (ui.clone(), focus);
            let kind = if is_open {
                ButtonKind::Pressed
            } else if is_finish_the_way_on {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            button(&colors, "finish-review", kind, ButtonSize::Regular)
                .when(can_approve_again, Styled::flex_1)
                .when(!can_approve_again, |button| {
                    button.child(button_icon(
                        &colors,
                        kind,
                        ButtonSize::Regular,
                        Icon::ListChecks,
                    ))
                })
                .child(if finish.has_last_review {
                    "Update review"
                } else {
                    "Finish review"
                })
                .on_click(move |_, window, cx| {
                    update(&ui, cx, |ui| ui.is_open = !ui.is_open);
                    window.focus(&focus, cx);
                })
        };
        let can_finish = self.finishing != Finishing::NothingToFinish;
        let is_confirming_approve_again = can_approve_again && is_confirming && !is_open;
        let approve_again = can_approve_again.then(|| {
            let (ui, on_finish, text) = (ui.clone(), Rc::clone(&self.on_finish), self.body.clone());
            let must_confirm = self.warning.is_some() && !is_confirming;
            button(
                &colors,
                "approve-again",
                ButtonKind::Secondary,
                ButtonSize::Regular,
            )
            .flex_1()
            .child(if is_confirming_approve_again {
                "Approve anyway"
            } else {
                "Approve again"
            })
            .on_click(move |_: &ClickEvent, _, cx| {
                update(&ui, cx, |ui| {
                    ui.verdict = ReviewVerdict::Approve;
                    ui.is_confirming_approval = must_confirm;
                });
                if !must_confirm {
                    submit(&ui, text.clone(), &on_finish, cx);
                }
            })
        });
        let mark_done = self
            .on_mark_done
            .as_ref()
            .filter(|_| finish.can_mark_done)
            .map(|on_mark_done| {
                let on_mark_done = Rc::clone(on_mark_done);
                let kind = if is_open {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Primary
                };
                button(&colors, "mark-done", kind, ButtonSize::Regular)
                    .w_full()
                    .child(button_icon(&colors, kind, ButtonSize::Regular, Icon::Check))
                    .child("Mark done")
                    .on_click(move |_, window, cx| on_mark_done(window, cx))
            });
        let quick_note = (!is_open)
            .then(|| {
                let warning = self
                    .warning
                    .filter(|_| is_confirming_approve_again)
                    .map(|warning| unread_warning_text(&warning));
                let busy = is_busy.then(|| "Sending...".to_owned());
                busy.or(warning).or_else(|| problem.clone())
            })
            .flatten()
            .map(|note| {
                let color = if is_busy {
                    colors.muted_text
                } else {
                    colors.danger
                };
                div().text_sm().text_color(color).child(note)
            });
        let actions = if can_approve_again && can_finish {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(mark_done)
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children(approve_again)
                        .child(open_button),
                )
                .into_any_element()
        } else {
            open_button.into_any_element()
        };
        let wrapper = div()
            .flex()
            .flex_col()
            .gap_2()
            .children(can_finish.then_some(actions))
            .children(quick_note);
        if !is_open || !can_finish {
            return wrapper.into_any_element();
        }

        let shown_verdicts: Vec<ReviewVerdict> = [
            ReviewVerdict::Comment,
            ReviewVerdict::Approve,
            ReviewVerdict::RequestChanges,
        ]
        .into_iter()
        .filter(|verdict| {
            self.finishing == Finishing::AnyVerdict || *verdict == ReviewVerdict::Comment
        })
        .collect();
        let picked = shown_verdicts.iter().position(|shown| *shown == verdict);
        let segments = shown_verdicts
            .iter()
            .map(|&choice| {
                let ui = ui.clone();
                Segment::new(
                    SharedString::from(format!("verdict-{}", choice.github_name())),
                    label(choice),
                    move |_, _, cx| {
                        update(&ui, cx, |ui| {
                            ui.verdict = choice;
                            ui.is_confirming_approval = false;
                        });
                    },
                )
                .icon(icon_of(choice))
            })
            .collect();
        let verdicts = Segmented::new("verdict", SegmentLayout::Column, segments)
            .picked(picked)
            .thumb_tone(tone_of(verdict));

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
            .child(button_icon(
                &colors,
                ButtonKind::Primary,
                ButtonSize::Regular,
                Icon::Send,
            ))
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
            div()
                .text_sm()
                .text_color(colors.danger)
                .child(unread_warning_text(&warning))
        });
        let form = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(verdicts)
            .child(
                Field::new(&focus_of_body)
                    .min_h(px(64.))
                    .px_2()
                    .py_1p5()
                    .text_sm()
                    .child(body),
            )
            .children(warning_text)
            .children(
                problem.map(|problem| div().text_sm().text_color(colors.danger).child(problem)),
            )
            .child(if is_busy {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(colors.muted_text)
                    .child(busy_dot(&colors, "finish-review-sending-dot"))
                    .child("Sending...")
                    .into_any_element()
            } else {
                submit_button.into_any_element()
            });
        wrapper
            .child(slide_in(
                form,
                "finish-review-form",
                STATE,
                Edge::Bottom,
                BOX_RISE,
            ))
            .into_any_element()
    }
}

/// A basic review has files to name, a guided one steps and the safety net's pieces.
fn unread_warning_text(warning: &ApproveWarning) -> String {
    let plural = |count: usize| if count == 1 { "" } else { "s" };
    if warning.unread_files > 0 {
        return format!(
            "Not read yet: {} file{}. Approve anyway?",
            warning.unread_files,
            plural(warning.unread_files),
        );
    }
    format!(
        "Not read yet: {} step{} and {} safety net piece{}. Approve anyway?",
        warning.unread_steps,
        plural(warning.unread_steps),
        warning.unread_safety_net_pieces,
        plural(warning.unread_safety_net_pieces),
    )
}
