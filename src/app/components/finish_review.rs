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

/// Hands the review body to the core, which saves it, each time it changes.
pub type OnBodyEdit = Rc<dyn Fn(String, &mut App)>;

/// What the reviewer is doing in the finish dialog. The body is not here: the core keeps it.
struct FinishUi {
    is_open: bool,
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
    can_judge: bool,
    warning: Option<ApproveWarning>,
    body: String,
    on_body_edit: OnBodyEdit,
    on_finish: OnFinish,
}

impl FinishReviewDialog {
    pub fn new(
        can_judge: bool,
        warning: Option<ApproveWarning>,
        body: String,
        on_body_edit: OnBodyEdit,
        on_finish: OnFinish,
    ) -> Self {
        Self {
            can_judge,
            warning,
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

fn on_key(
    ui: &Entity<FinishUi>,
    body: &str,
    on_body_edit: &OnBodyEdit,
    event: &KeyDownEvent,
    cx: &mut App,
) {
    let keystroke = &event.keystroke;
    if ui.read(cx).is_busy {
        return;
    }
    if keystroke.key == "escape" {
        return update(ui, cx, |ui| ui.is_open = false);
    }
    let mut edited = TextDraft::with_text(body);
    let changed = match keystroke.key.as_str() {
        "enter" => {
            edited.newline();
            true
        }
        _ => edited.edit(keystroke),
    };
    if changed {
        on_body_edit(edited.text().to_string(), cx);
    }
}

impl RenderOnce for FinishReviewDialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
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
        let (body, problem, focus) = (
            if state.is_busy {
                self.body.clone()
            } else {
                TextDraft::with_text(&self.body).shown()
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
            let (ui, on_finish, text) = (ui.clone(), self.on_finish.clone(), self.body.clone());
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
                    submit(&ui, text.clone(), &on_finish, cx);
                }
            })
        };
        let (key_ui, key_body, on_body_edit) =
            (ui.clone(), self.body.clone(), self.on_body_edit.clone());
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
                    .on_key_down(move |event, _, cx| {
                        on_key(&key_ui, &key_body, &on_body_edit, event, cx)
                    })
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
