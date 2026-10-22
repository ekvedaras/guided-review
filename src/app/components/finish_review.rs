use std::rc::Rc;

use gpui::{
    App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
    red,
};

use crate::viewer::{ApproveWarning, FinishResult, ReviewVerdict};

use super::palette::Palette;
use super::text_input::TextDraft;

/// Sends the review and calls back with how it went. The layout owns the review and the network.
pub type OnFinish =
    Rc<dyn Fn(ReviewVerdict, String, Box<dyn FnOnce(&FinishResult, &mut App)>, &mut App)>;

/// What the reviewer is doing in the finish dialog.
struct FinishUi {
    is_open: bool,
    verdict: ReviewVerdict,
    body: TextDraft,
    /// The reviewer saw the unread warning and has to say yes to it once more.
    is_confirming_approval: bool,
    is_busy: bool,
    problem: Option<String>,
    focus: FocusHandle,
}

impl FinishUi {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            is_open: false,
            verdict: ReviewVerdict::Comment,
            body: TextDraft::default(),
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
    can_judge: bool,
    warning: Option<ApproveWarning>,
    on_finish: OnFinish,
}

impl FinishReviewDialog {
    pub fn new(can_judge: bool, warning: Option<ApproveWarning>, on_finish: OnFinish) -> Self {
        Self {
            can_judge,
            warning,
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

fn button(id: &'static str, text: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .cursor_pointer()
        .child(text.to_string())
}

fn update(ui: &Entity<FinishUi>, cx: &mut App, change: impl FnOnce(&mut FinishUi)) {
    ui.update(cx, |ui, cx| {
        change(ui);
        cx.notify();
    });
}

fn submit(ui: &Entity<FinishUi>, on_finish: &OnFinish, cx: &mut App) {
    let (verdict, body) = {
        let state = ui.read(cx);
        (state.verdict, state.body.text().to_string())
    };
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
                        ui.body.clear();
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

fn on_key(ui: &Entity<FinishUi>, event: &KeyDownEvent, cx: &mut App) {
    let keystroke = &event.keystroke;
    if ui.read(cx).is_busy {
        return;
    }
    update(ui, cx, |ui| match keystroke.key.as_str() {
        "escape" => ui.is_open = false,
        "enter" => ui.body.newline(),
        _ => {
            ui.body.edit(keystroke);
        }
    });
}

impl RenderOnce for FinishReviewDialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let ui = window.use_keyed_state("finish-review-ui", cx, |_, cx| FinishUi::new(cx));
        let state = ui.read(cx);
        let (is_open, verdict, is_busy, is_confirming) = (
            state.is_open,
            state.verdict,
            state.is_busy,
            state.is_confirming_approval,
        );
        let (body, problem, focus) = (
            if state.is_busy {
                state.body.text().to_string()
            } else {
                state.body.shown()
            },
            state.problem.clone(),
            state.focus.clone(),
        );

        let open_button = {
            let (ui, focus) = (ui.clone(), focus.clone());
            button("finish-review", "Finish review").on_click(move |_, window, cx| {
                update(&ui, cx, |ui| ui.is_open = !ui.is_open);
                window.focus(&focus, cx);
            })
        };
        let wrapper = div()
            .px_3()
            .pb_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(open_button);
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
            let chosen = div()
                .id(SharedString::from(format!(
                    "verdict-{}",
                    choice.github_name()
                )))
                .px_2()
                .rounded_md()
                .border_1()
                .border_color(Palette::border())
                .cursor_pointer()
                .child(label(choice))
                .on_click(move |_, _, cx| {
                    update(&ui, cx, |ui| {
                        ui.verdict = choice;
                        ui.is_confirming_approval = false;
                    })
                });
            if choice == verdict {
                chosen.bg(Palette::current())
            } else {
                chosen
            }
        });

        let unread_warning = (verdict == ReviewVerdict::Approve)
            .then_some(self.warning)
            .flatten();
        let must_confirm = unread_warning.is_some() && !is_confirming;
        let submit_button = {
            let (ui, on_finish) = (ui.clone(), self.on_finish.clone());
            button(
                "submit-review",
                if must_confirm {
                    "Approve..."
                } else {
                    "Submit review"
                },
            )
            .on_click(move |_: &ClickEvent, _, cx| {
                if must_confirm {
                    update(&ui, cx, |ui| ui.is_confirming_approval = true);
                } else {
                    submit(&ui, &on_finish, cx);
                }
            })
        };
        let key_ui = ui.clone();
        let warning_text = unread_warning.filter(|_| is_confirming).map(|warning| {
            div().text_sm().text_color(red()).child(format!(
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
                    .key_context("CommentInput")
                    .track_focus(&focus)
                    .on_key_down(move |event, _, cx| on_key(&key_ui, event, cx))
                    .min_h(px(64.))
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(Palette::border())
                    .bg(Palette::background())
                    .child(body),
            )
            .children(warning_text)
            .children(problem.map(|problem| div().text_sm().text_color(red()).child(problem)))
            .child(if is_busy {
                div().child("Sending...").into_any_element()
            } else {
                submit_button.into_any_element()
            })
            .into_any_element()
    }
}
