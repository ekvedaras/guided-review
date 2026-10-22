use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled,
    Window, div, px, red,
};

use crate::viewer::{
    Markdown, ReactionCount, SendMode, ThreadAction, ThreadActionResult, ThreadComment, ThreadView,
};

use super::markdown_view::MarkdownView;
use super::palette::Palette;
use super::text_input::TextDraft;

type OnToggle = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Runs a thread action and calls back with how it went. The layout owns the review and the network.
pub type OnThreadAction =
    Rc<dyn Fn(ThreadAction, Box<dyn FnOnce(&ThreadActionResult, &mut App)>, &mut App)>;

/// What the reviewer is doing in one thread: the reply being written and the action under way.
struct ThreadUi {
    reply: Option<TextDraft>,
    is_busy: bool,
    problem: Option<String>,
    focus: FocusHandle,
}

impl ThreadUi {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            reply: None,
            is_busy: false,
            problem: None,
            focus: cx.focus_handle(),
        }
    }
}

/// One review thread as the reviewer sees it under a line, or after the code of a step.
/// Folded threads (resolved, outdated, by a bot) show one line and a toggle.
#[derive(IntoElement)]
pub struct ThreadElement {
    view: ThreadView,
    can_reply_now: bool,
    on_toggle: OnToggle,
    on_action: OnThreadAction,
}

impl ThreadElement {
    pub fn new(
        view: ThreadView,
        can_reply_now: bool,
        on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_action: OnThreadAction,
    ) -> Self {
        Self {
            view,
            can_reply_now,
            on_toggle: Box::new(on_toggle),
            on_action,
        }
    }

    fn kind(&self) -> String {
        let thread = &self.view.thread;
        let mut kinds = Vec::new();
        if thread.is_outdated {
            kinds.push("Outdated");
        }
        if thread.is_resolved {
            kinds.push("Resolved");
        }
        if thread
            .comments
            .first()
            .is_some_and(|comment| comment.is_bot)
        {
            kinds.push("Bot");
        }
        if kinds.is_empty() {
            "Thread".to_string()
        } else {
            kinds.join(", ")
        }
    }

    fn where_it_is(&self) -> String {
        let thread = &self.view.thread;
        let lines = match (thread.is_outdated, thread.start_line, thread.line) {
            (true, _, _) => thread
                .original_line
                .map(|end| match thread.original_start_line {
                    Some(start) if start != end => format!("lines {start}-{end}"),
                    _ => format!("line {end}"),
                }),
            (false, Some(start), Some(end)) if start != end => Some(format!("lines {start}-{end}")),
            (false, _, Some(line)) if !thread.is_file_comment => Some(format!("line {line}")),
            _ => None,
        };
        match (self.view.anchor.is_some(), lines) {
            (true, _) | (false, None) => thread.path.clone(),
            (false, Some(lines)) => format!("{}, {lines}", thread.path),
        }
    }
}

fn small_button(id: SharedString, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .cursor_pointer()
        .text_sm()
        .child(label.to_string())
}

/// Starts an action and shows it as under way until GitHub answers.
fn run_action(
    action: ThreadAction,
    ui: &Entity<ThreadUi>,
    on_action: &OnThreadAction,
    clear_reply: bool,
    cx: &mut App,
) {
    ui.update(cx, |ui, cx| {
        ui.is_busy = true;
        ui.problem = None;
        cx.notify();
    });
    let done_ui = ui.clone();
    on_action(
        action,
        Box::new(move |result, cx| {
            done_ui.update(cx, |ui, cx| {
                ui.is_busy = false;
                match result {
                    ThreadActionResult::Done if clear_reply => ui.reply = None,
                    ThreadActionResult::Done => {}
                    ThreadActionResult::Refused(refusal) => ui.problem = Some(refusal.to_string()),
                    ThreadActionResult::Failed(reason) => ui.problem = Some(reason.clone()),
                }
                cx.notify();
            });
        }),
        cx,
    );
}

impl ThreadElement {
    fn reactions(
        comment: &ThreadComment,
        ui: &Entity<ThreadUi>,
        on_action: &OnThreadAction,
    ) -> AnyElement {
        div()
            .flex()
            .gap_1()
            .children(comment.reaction_summary().into_iter().map(
                |ReactionCount {
                     reaction,
                     count,
                     viewer_has_reacted,
                 }| {
                    let (ui, on_action, comment_id) =
                        (ui.clone(), on_action.clone(), comment.id.clone());
                    let label = if count > 0 {
                        format!("{} {count}", reaction.emoji())
                    } else {
                        reaction.emoji().to_string()
                    };
                    let button = small_button(
                        SharedString::from(format!(
                            "react-{}-{}",
                            comment.id,
                            reaction.github_name()
                        )),
                        &label,
                    )
                    .when_else(viewer_has_reacted, |b| b.bg(Palette::current()))
                    .when_else(count == 0, |b| b.opacity(0.5));
                    button.on_click(move |_, _, cx| {
                        let action = ThreadAction::SetReaction {
                            comment_id: comment_id.clone(),
                            reaction,
                            on: !viewer_has_reacted,
                        };
                        run_action(action, &ui, &on_action, false, cx);
                    })
                },
            ))
            .into_any_element()
    }

    fn reply_box(
        thread_id: &str,
        can_reply_now: bool,
        on_action: &OnThreadAction,
        reply: &TextDraft,
        ui: &Entity<ThreadUi>,
        focus: &FocusHandle,
        is_busy: bool,
    ) -> AnyElement {
        let thread_id = thread_id.to_string();
        let send = {
            let (ui, on_action, thread_id) = (ui.clone(), on_action.clone(), thread_id.clone());
            move |mode: SendMode| {
                let (ui, on_action, thread_id) = (ui.clone(), on_action.clone(), thread_id.clone());
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    send_reply(&ui, &on_action, &thread_id, mode, cx);
                }
            }
        };
        let on_key = {
            let (ui, on_action, thread_id) = (ui.clone(), on_action.clone(), thread_id.clone());
            move |event: &KeyDownEvent, _: &mut Window, cx: &mut App| {
                let keystroke = &event.keystroke;
                if ui.read(cx).is_busy {
                    return;
                }
                match keystroke.key.as_str() {
                    "escape" => ui.update(cx, |ui, cx| {
                        ui.reply = None;
                        cx.notify();
                    }),
                    "enter" if keystroke.modifiers.platform => {
                        send_reply(&ui, &on_action, &thread_id, SendMode::AddToReview, cx);
                    }
                    key => ui.update(cx, |ui, cx| {
                        let Some(text) = &mut ui.reply else { return };
                        match key {
                            "enter" => text.newline(),
                            _ if text.edit(keystroke) => {}
                            _ => return,
                        }
                        cx.notify();
                    }),
                }
            }
        };
        let id = thread_id;
        let buttons = (!is_busy).then(|| {
            div()
                .flex()
                .gap_2()
                .child(
                    small_button(
                        SharedString::from(format!("reply-add-{id}")),
                        "Add to review",
                    )
                    .on_click(send(SendMode::AddToReview)),
                )
                .children(can_reply_now.then(|| {
                    small_button(SharedString::from(format!("reply-now-{id}")), "Reply now")
                        .on_click(send(SendMode::CommentNow))
                }))
                .child(
                    small_button(SharedString::from(format!("reply-cancel-{id}")), "Cancel")
                        .on_click({
                            let ui = ui.clone();
                            move |_, _, cx| {
                                ui.update(cx, |ui, cx| {
                                    ui.reply = None;
                                    cx.notify();
                                })
                            }
                        }),
                )
        });
        div()
            .id(SharedString::from(format!("reply-box-{id}")))
            .key_context("CommentInput")
            .track_focus(focus)
            .on_key_down(on_key)
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(Palette::border())
            .bg(Palette::sidebar())
            .child(div().min_h(px(48.)).child(if is_busy {
                reply.text().to_string()
            } else {
                reply.shown()
            }))
            .children(buttons)
            .into_any_element()
    }
}

fn send_reply(
    ui: &Entity<ThreadUi>,
    on_action: &OnThreadAction,
    thread_id: &str,
    mode: SendMode,
    cx: &mut App,
) {
    let Some(body) = ui
        .read(cx)
        .reply
        .as_ref()
        .map(|draft| draft.text().to_string())
        .filter(|body| !body.trim().is_empty())
    else {
        return;
    };
    let action = ThreadAction::Reply {
        thread_id: thread_id.to_string(),
        body,
        mode,
    };
    run_action(action, ui, on_action, true, cx);
}

trait WhenElse: Sized {
    fn when_else(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if condition { then(self) } else { self }
    }
}

impl<E> WhenElse for E {}

impl RenderOnce for ThreadElement {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.view.thread.id.clone();
        let ui = window.use_keyed_state(
            SharedString::from(format!("thread-ui-{id}")),
            cx,
            |_, cx| ThreadUi::new(cx),
        );
        let reaction_on_action = self.on_action.clone();
        let comment_count = self.view.thread.comments.len();
        let first_author = self
            .view
            .thread
            .comments
            .first()
            .map(|comment| comment.author.clone())
            .unwrap_or_default();
        let summary = format!(
            "{} · {} · {first_author} · {comment_count} comment{}",
            self.kind(),
            self.where_it_is(),
            if comment_count == 1 { "" } else { "s" }
        );
        let toggle = div()
            .id(SharedString::from(format!("thread-toggle-{id}")))
            .flex_1()
            .cursor_pointer()
            .text_sm()
            .text_color(Palette::muted_text())
            .on_click(self.on_toggle)
            .child(format!(
                "{} {summary}",
                if self.view.is_folded { "▸" } else { "▾" }
            ));
        let conversation_link = self.view.thread.comments.first().map(|comment| {
            let url = comment.url.clone();
            div()
                .id(SharedString::from(format!("thread-link-{id}")))
                .flex_none()
                .cursor_pointer()
                .text_sm()
                .text_color(Palette::muted_text())
                .on_click(move |_, _, cx| cx.open_url(&url))
                .child("Open on GitHub")
        });
        let comments = (!self.view.is_folded).then(|| {
            div().flex().flex_col().gap_3().children(
                self.view
                    .thread
                    .comments
                    .iter()
                    .enumerate()
                    .map(|(index, comment)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().child(comment.author.clone()))
                            .child(
                                MarkdownView::new(
                                    format!("thread-{id}-{index}"),
                                    Markdown::parse(&comment.body),
                                )
                                .on_lines(self.view.commented_lines.clone()),
                            )
                            .child(Self::reactions(comment, &ui, &reaction_on_action))
                    }),
            )
        });
        let (reply, is_busy, problem, focus) = {
            let state = ui.read(cx);
            (
                state.reply.as_ref().map(|draft| draft.text().to_string()),
                state.is_busy,
                state.problem.clone(),
                state.focus.clone(),
            )
        };
        let (can_reply_now, on_action) = (self.can_reply_now, self.on_action.clone());
        let actions = (!self.view.is_folded).then(|| {
            let thread = &self.view.thread;
            let resolve_action = if thread.is_resolved {
                ThreadAction::Unresolve {
                    thread_id: id.clone(),
                }
            } else {
                ThreadAction::Resolve {
                    thread_id: id.clone(),
                }
            };
            let (resolve_ui, resolve_on_action) = (ui.clone(), on_action.clone());
            let reply_ui = ui.clone();
            let reply_focus = focus.clone();
            let buttons = div()
                .flex()
                .gap_2()
                .child(
                    small_button(SharedString::from(format!("reply-{id}")), "Reply").on_click(
                        move |_, window, cx| {
                            reply_ui.update(cx, |ui, cx| {
                                ui.reply.get_or_insert_with(TextDraft::default);
                                cx.notify();
                            });
                            window.focus(&reply_focus, cx);
                        },
                    ),
                )
                .child(
                    small_button(
                        SharedString::from(format!("resolve-{id}")),
                        if thread.is_resolved {
                            "Unresolve"
                        } else {
                            "Resolve"
                        },
                    )
                    .on_click(move |_, _, cx| {
                        run_action(
                            resolve_action.clone(),
                            &resolve_ui,
                            &resolve_on_action,
                            false,
                            cx,
                        );
                    }),
                );
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(reply.as_deref().map(|text| {
                    Self::reply_box(
                        &id,
                        can_reply_now,
                        &on_action,
                        &TextDraft::with_text(text),
                        &ui,
                        &focus,
                        is_busy,
                    )
                }))
                .child(buttons.when_else(is_busy, |row| row.opacity(0.5)))
                .children(is_busy.then(|| div().text_sm().child("Working...")))
                .children(problem.map(|problem| div().text_sm().text_color(red()).child(problem)))
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .m_2()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(Palette::border())
            .bg(Palette::background())
            .font_family(super::palette::TEXT_FONT)
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(toggle)
                    .children(conversation_link),
            )
            .children(comments)
            .children(actions)
    }
}
