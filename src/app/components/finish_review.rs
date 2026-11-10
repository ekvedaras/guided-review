use std::rc::Rc;

use gpui::{
    App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::viewer::{ApproveWarning, CLAMPED_BODY_LINES, FinishResult, ReviewVerdict};

use crate::app::sidebar_rows::SidebarLastReview;

use super::button::{ButtonKind, ButtonSize, button, button_icon};
use super::icon::{Icon, sized_icon};
use super::motion::{BOX_RISE, Edge, STATE, busy_dot, slide_in, turn_to};
use super::palette::{Colors, Tone};
use super::segmented::{Segment, SegmentLayout, Segmented};
use super::text_box::{Field, TextBox};
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
    /// A review of the viewer's is on GitHub already, so this one updates it.
    last_review: Option<SidebarLastReview>,
    body: String,
    on_body_edit: OnBodyEdit,
    on_finish: OnFinish,
}

impl FinishReviewDialog {
    pub fn new(
        is_everything_read: bool,
        finishing: Finishing,
        warning: Option<ApproveWarning>,
        last_review: Option<SidebarLastReview>,
        body: String,
        on_body_edit: OnBodyEdit,
        on_finish: OnFinish,
    ) -> Self {
        Self {
            is_everything_read,
            finishing,
            warning,
            last_review,
            body,
            on_body_edit,
            on_finish,
        }
    }
}

fn icon_of(verdict: ReviewVerdict) -> Icon {
    match verdict {
        ReviewVerdict::Comment => Icon::MessageSquare,
        ReviewVerdict::Approve => Icon::CircleCheck,
        ReviewVerdict::RequestChanges => Icon::Diff,
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
            let has_draft = !self.body.is_empty();
            move |_, cx| FinishUi::new(has_draft, cx)
        });
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
        let is_last_review_body_open = state.is_last_review_body_open;
        let (problem, focus) = (state.problem.clone(), state.focus.clone());
        let focus_of_body = focus.clone();
        let body = if is_busy {
            div().child(self.body.clone()).into_any_element()
        } else {
            body_box(&ui, &focus, &self.body, &self.on_body_edit).into_any_element()
        };

        let open_button = {
            let (ui, focus) = (ui.clone(), focus);
            let kind = if is_open {
                ButtonKind::Pressed
            } else if self.is_everything_read {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            button(&colors, "finish-review", kind, ButtonSize::Regular)
                .child(button_icon(
                    &colors,
                    kind,
                    ButtonSize::Regular,
                    Icon::ListChecks,
                ))
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
                        .flex()
                        .items_center()
                        .gap_1()
                        .cursor_pointer()
                        .text_color(colors.muted_text)
                        .hover(|toggle| toggle.text_color(colors.text))
                        .on_click(move |_, _, cx| {
                            update(&ui, cx, |ui| {
                                ui.is_last_review_body_open = !ui.is_last_review_body_open;
                            });
                        })
                        .child(turn_to(
                            sized_icon(Icon::ChevronRight, colors.muted_text, px(12.)),
                            "last-review-chevron",
                            is_last_review_body_open,
                            1.,
                        ))
                        .child(if is_last_review_body_open {
                            "Show less"
                        } else {
                            "Show all"
                        })
                }))
        });
        let can_finish = self.finishing != Finishing::NothingToFinish;
        let wrapper = div()
            .flex()
            .flex_col()
            .gap_2()
            .children(can_finish.then_some(open_button))
            .children(last_review);
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
