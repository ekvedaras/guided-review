use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
    red,
};

use crate::app::code_links::open_link;
use crate::viewer::{
    Audience, CommentId, ReactionChange, ReactionCount, SendMode, ThreadAction,
    ThreadActionResult,
    ThreadComment, ThreadId, ThreadView,
};

use super::markdown_view::{MarkdownView, parsed_markdown};
use super::palette::Colors;
use super::text_box::TextBox;
use super::text_input::Lines;

type OnToggle = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Runs a thread action and calls back with how it went. The layout owns the review and the network.
pub type OnThreadAction =
    Rc<dyn Fn(ThreadAction, Box<dyn FnOnce(&ThreadActionResult, &mut App)>, &mut App)>;

/// What happened to the reply being written; the core saves it, so it survives scrolling away.
pub enum ReplyDraftChange {
    Open,
    Edit(String),
    Discard,
}

pub type OnReplyDraft = Rc<dyn Fn(ReplyDraftChange, &mut App)>;

/// What happened to the edit of one comment; the core keeps its text, so it survives scrolling away.
pub type OnCommentEdit = Rc<dyn Fn(&CommentId, ReplyDraftChange, &mut App)>;

/// What the reviewer is doing in one thread: the action under way. The reply text is the core's.
struct ThreadUi {
    is_busy: bool,
    /// The comment whose new text is on its way to GitHub.
    saving_edit: Option<CommentId>,
    problem: Option<String>,
    focus: FocusHandle,
}

impl ThreadUi {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            is_busy: false,
            saving_edit: None,
            problem: None,
            focus: cx.focus_handle(),
        }
    }
}

struct ReplyBox<'a> {
    colors: Colors,
    thread_id: &'a ThreadId,
    can_reply_now: bool,
    can_ask_agent: bool,
    on_action: &'a OnThreadAction,
    on_reply_draft: &'a OnReplyDraft,
    ui: &'a Entity<ThreadUi>,
    focus: &'a FocusHandle,
    is_busy: bool,
}

/// One review thread as the reviewer sees it under a line, or after the code of a step.
/// Folded threads (resolved, outdated, by a bot) show one line and a toggle.
#[derive(IntoElement)]
pub struct ThreadElement {
    view: Rc<ThreadView>,
    can_reply_now: bool,
    can_ask_agent: bool,
    reply: Option<String>,
    /// The text of each comment being edited, as typed so far.
    edits: BTreeMap<CommentId, String>,
    on_reply_draft: OnReplyDraft,
    on_comment_edit: OnCommentEdit,
    on_toggle: OnToggle,
    on_action: OnThreadAction,
}

impl ThreadElement {
    #[expect(
        clippy::too_many_arguments,
        reason = "the element is built from the texts the review holds and the callbacks that change them"
    )]
    pub fn new(
        view: Rc<ThreadView>,
        can_reply_now: bool,
        can_ask_agent: bool,
        reply: Option<String>,
        edits: BTreeMap<CommentId, String>,
        on_reply_draft: OnReplyDraft,
        on_comment_edit: OnCommentEdit,
        on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_action: OnThreadAction,
    ) -> Self {
        Self {
            view,
            can_reply_now,
            can_ask_agent,
            reply,
            edits,
            on_reply_draft,
            on_comment_edit,
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
            "Thread".to_owned()
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
            (true, _) | (false, None) => thread.path.to_string(),
            (false, Some(lines)) => format!("{}, {lines}", thread.path),
        }
    }
}

fn small_button(colors: &Colors, id: SharedString, label: &str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .cursor_pointer()
        .text_sm()
        .child(label.to_owned())
}

/// Starts an action and shows it as under way until GitHub answers.
fn run_action(
    action: ThreadAction,
    ui: &Entity<ThreadUi>,
    on_action: &OnThreadAction,
    cx: &mut App,
) {
    let saving_edit = match &action {
        ThreadAction::EditComment { comment_id } => Some(comment_id.clone()),
        _ => None,
    };
    ui.update(cx, |ui, cx| {
        ui.is_busy = true;
        ui.saving_edit = saving_edit;
        ui.problem = None;
        cx.notify();
    });
    let done_ui = ui.clone();
    on_action(
        action,
        Box::new(move |result, cx| {
            done_ui.update(cx, |ui, cx| {
                ui.is_busy = false;
                ui.saving_edit = None;
                match result {
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
        colors: &Colors,
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
                        (ui.clone(), Rc::clone(on_action), comment.id.clone());
                    let label = if count > 0 {
                        format!("{} {count}", reaction.emoji())
                    } else {
                        reaction.emoji().to_owned()
                    };
                    let button = small_button(
                        colors,
                        SharedString::from(format!(
                            "react-{}-{}",
                            comment.id,
                            reaction.github_name()
                        )),
                        &label,
                    )
                    .when_else(viewer_has_reacted, |b| b.bg(colors.current))
                    .when_else(count == 0, |b| b.opacity(0.5));
                    button.on_click(move |_, _, cx| {
                        let action = ThreadAction::SetReaction {
                            comment_id: comment_id.clone(),
                            reaction,
                            change: if viewer_has_reacted {
                                ReactionChange::Remove
                            } else {
                                ReactionChange::Add
                            },
                        };
                        run_action(action, &ui, &on_action, cx);
                    })
                },
            ))
            .into_any_element()
    }

    fn reply_box(
        ReplyBox {
            colors,
            thread_id,
            can_reply_now,
            can_ask_agent,
            on_action,
            on_reply_draft,
            ui,
            focus,
            is_busy,
        }: ReplyBox,
        reply: &str,
    ) -> AnyElement {
        let thread_id = thread_id.clone();
        let send = {
            let (ui, on_action, thread_id) = (ui.clone(), Rc::clone(on_action), thread_id.clone());
            move |mode: SendMode, audience: Audience| {
                let (ui, on_action, thread_id) =
                    (ui.clone(), Rc::clone(&on_action), thread_id.clone());
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    send_reply(&ui, &on_action, &thread_id, mode, audience, cx);
                }
            }
        };
        let text = if is_busy {
            div().child(reply.to_owned()).into_any_element()
        } else {
            let (ui, on_action, submitted_thread) =
                (ui.clone(), Rc::clone(on_action), thread_id.clone());
            let (ask_ui, ask_action, asked_thread) =
                (ui.clone(), Rc::clone(&on_action), thread_id.clone());
            let (on_edit, on_cancel) = (Rc::clone(on_reply_draft), Rc::clone(on_reply_draft));
            TextBox::new(
                SharedString::from(format!("reply-text-{thread_id}")),
                focus,
                reply,
                Lines::Multi,
            )
            .on_edit(move |text, _, cx| on_edit(ReplyDraftChange::Edit(text), cx))
            .on_submit(move |_, cx| {
                send_reply(
                    &ui,
                    &on_action,
                    &submitted_thread,
                    SendMode::AddToReview,
                    Audience::Author,
                    cx,
                );
            })
            .on_submit_to_agent(can_ask_agent, move |_, cx| {
                send_reply(
                    &ask_ui,
                    &ask_action,
                    &asked_thread,
                    SendMode::AddToReview,
                    Audience::Agent,
                    cx,
                );
            })
            .on_cancel(move |_, cx| on_cancel(ReplyDraftChange::Discard, cx))
            .into_any_element()
        };
        let id = thread_id;
        let buttons = (!is_busy).then(|| {
            div()
                .flex()
                .gap_2()
                .child(
                    small_button(
                        &colors,
                        SharedString::from(format!("reply-add-{id}")),
                        "Add to review",
                    )
                    .on_click(send(SendMode::AddToReview, Audience::Author)),
                )
                .children(can_ask_agent.then(|| {
                    small_button(
                        &colors,
                        SharedString::from(format!("reply-ask-{id}")),
                        "Ask agent",
                    )
                    .on_click(send(SendMode::AddToReview, Audience::Agent))
                }))
                .children((can_ask_agent && can_reply_now).then(|| {
                    small_button(
                        &colors,
                        SharedString::from(format!("reply-ask-now-{id}")),
                        "Ask agent now",
                    )
                    .on_click(send(SendMode::CommentNow, Audience::Agent))
                }))
                .children(can_reply_now.then(|| {
                    small_button(
                        &colors,
                        SharedString::from(format!("reply-now-{id}")),
                        "Reply now",
                    )
                    .on_click(send(SendMode::CommentNow, Audience::Author))
                }))
                .child(
                    small_button(
                        &colors,
                        SharedString::from(format!("reply-cancel-{id}")),
                        "Cancel",
                    )
                    .on_click({
                        let on_reply_draft = Rc::clone(on_reply_draft);
                        move |_, _, cx| on_reply_draft(ReplyDraftChange::Discard, cx)
                    }),
                )
        });
        div()
            .id(SharedString::from(format!("reply-box-{id}")))
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .child(div().min_h(px(48.)).child(text))
            .children(buttons)
            .into_any_element()
    }
}

/// Where a comment's new text is written, in place of the comment.
struct EditBox<'a> {
    colors: Colors,
    comment_id: &'a CommentId,
    on_action: &'a OnThreadAction,
    on_comment_edit: &'a OnCommentEdit,
    ui: &'a Entity<ThreadUi>,
    focus: &'a FocusHandle,
    is_saving: bool,
}

impl ThreadElement {
    fn edit_box(
        EditBox {
            colors,
            comment_id,
            on_action,
            on_comment_edit,
            ui,
            focus,
            is_saving,
        }: EditBox,
        text: &str,
    ) -> AnyElement {
        let save = {
            let (ui, on_action, comment_id) =
                (ui.clone(), Rc::clone(on_action), comment_id.clone());
            move |cx: &mut App| {
                let action = ThreadAction::EditComment {
                    comment_id: comment_id.clone(),
                };
                run_action(action, &ui, &on_action, cx);
            }
        };
        let body = if is_saving {
            div().child(text.to_owned()).into_any_element()
        } else {
            let (on_edit, on_cancel, submit) = (
                Rc::clone(on_comment_edit),
                Rc::clone(on_comment_edit),
                save.clone(),
            );
            let (edited_id, cancelled_id) = (comment_id.clone(), comment_id.clone());
            TextBox::new(
                SharedString::from(format!("edit-text-{comment_id}")),
                focus,
                text,
                Lines::Multi,
            )
            .on_edit(move |text, _, cx| on_edit(&edited_id, ReplyDraftChange::Edit(text), cx))
            .on_submit(move |_, cx| submit(cx))
            .on_cancel(move |_, cx| on_cancel(&cancelled_id, ReplyDraftChange::Discard, cx))
            .into_any_element()
        };
        let buttons = (!is_saving).then(|| {
            let on_comment_edit = Rc::clone(on_comment_edit);
            let cancelled_id = comment_id.clone();
            div()
                .flex()
                .gap_2()
                .child(
                    small_button(
                        &colors,
                        SharedString::from(format!("edit-save-{comment_id}")),
                        "Save",
                    )
                    .on_click(move |_, _, cx| save(cx)),
                )
                .child(
                    small_button(
                        &colors,
                        SharedString::from(format!("edit-cancel-{comment_id}")),
                        "Cancel",
                    )
                    .on_click(move |_, _, cx| {
                        on_comment_edit(&cancelled_id, ReplyDraftChange::Discard, cx);
                    }),
                )
        });
        div()
            .id(SharedString::from(format!("edit-box-{comment_id}")))
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .child(div().min_h(px(48.)).child(body))
            .children(is_saving.then(|| div().text_sm().child("Saving…")))
            .children(buttons)
            .into_any_element()
    }
}

fn send_reply(
    ui: &Entity<ThreadUi>,
    on_action: &OnThreadAction,
    thread_id: &ThreadId,
    mode: SendMode,
    audience: Audience,
    cx: &mut App,
) {
    let action = ThreadAction::Reply {
        thread_id: thread_id.clone(),
        mode,
        audience,
    };
    run_action(action, ui, on_action, cx);
}

trait WhenElse: Sized {
    fn when_else(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if condition { then(self) } else { self }
    }
}

impl<E> WhenElse for E {}

impl RenderOnce for ThreadElement {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let id = self.view.thread.id.clone();
        let ui = window.use_keyed_state(
            SharedString::from(format!("thread-ui-{id}")),
            cx,
            |_, cx| ThreadUi::new(cx),
        );
        let reaction_on_action = Rc::clone(&self.on_action);
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
            .text_color(colors.muted_text)
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
                .text_color(colors.muted_text)
                .on_click(move |_, _, cx| open_link(&url, cx))
                .child("Open on GitHub")
        });
        let (is_busy, saving_edit, problem, focus) = {
            let state = ui.read(cx);
            (
                state.is_busy,
                state.saving_edit.clone(),
                state.problem.clone(),
                state.focus.clone(),
            )
        };
        let comments = (!self.view.is_folded).then(|| {
            div().flex().flex_col().gap_3().children(
                self.view
                    .thread
                    .comments
                    .iter()
                    .enumerate()
                    .map(|(index, comment)| {
                        let being_edited = self.edits.get(&comment.id);
                        let header = div()
                            .flex()
                            .gap_2()
                            .child(div().text_sm().child(comment.author.clone()))
                            .children((comment.by_viewer && being_edited.is_none()).then(|| {
                                let (on_comment_edit, comment_id) =
                                    (Rc::clone(&self.on_comment_edit), comment.id.clone());
                                let edit_focus = window.use_keyed_state(
                                    SharedString::from(format!("edit-focus-{}", comment.id)),
                                    cx,
                                    |_, cx| cx.focus_handle(),
                                );
                                small_button(
                                    &colors,
                                    SharedString::from(format!("edit-{}", comment.id)),
                                    "Edit",
                                )
                                .on_click(move |_, window, cx| {
                                    on_comment_edit(&comment_id, ReplyDraftChange::Open, cx);
                                    let handle = edit_focus.read(cx).clone();
                                    window.focus(&handle, cx);
                                })
                            }));
                        let content = match being_edited {
                            Some(text) => {
                                let edit_focus = window
                                    .use_keyed_state(
                                        SharedString::from(format!("edit-focus-{}", comment.id)),
                                        cx,
                                        |_, cx| cx.focus_handle(),
                                    )
                                    .read(cx)
                                    .clone();
                                Self::edit_box(
                                    EditBox {
                                        colors,
                                        comment_id: &comment.id,
                                        on_action: &self.on_action,
                                        on_comment_edit: &self.on_comment_edit,
                                        ui: &ui,
                                        focus: &edit_focus,
                                        is_saving: saving_edit.as_ref() == Some(&comment.id),
                                    },
                                    text,
                                )
                            }
                            None => MarkdownView::new(
                                format!("thread-{id}-{index}"),
                                parsed_markdown(&comment.body),
                            )
                            .on_lines(Arc::clone(&self.view.commented_lines))
                            .into_any_element(),
                        };
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(header)
                            .child(content)
                            .child(Self::reactions(&colors, comment, &ui, &reaction_on_action))
                    }),
            )
        });
        let reply = self.reply.clone();
        let on_reply_draft = Rc::clone(&self.on_reply_draft);
        let (can_reply_now, can_ask_agent, on_action) = (
            self.can_reply_now,
            self.can_ask_agent,
            Rc::clone(&self.on_action),
        );
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
            let (resolve_ui, resolve_on_action) = (ui.clone(), Rc::clone(&on_action));
            let open_reply = Rc::clone(&on_reply_draft);
            let reply_focus = focus.clone();
            let buttons = div()
                .flex()
                .gap_2()
                .child(
                    small_button(&colors, SharedString::from(format!("reply-{id}")), "Reply")
                        .on_click(move |_, window, cx| {
                            open_reply(ReplyDraftChange::Open, cx);
                            window.focus(&reply_focus, cx);
                        }),
                )
                .child(
                    small_button(
                        &colors,
                        SharedString::from(format!("resolve-{id}")),
                        if thread.is_resolved {
                            "Unresolve"
                        } else {
                            "Resolve"
                        },
                    )
                    .on_click(move |_, _, cx| {
                        run_action(resolve_action.clone(), &resolve_ui, &resolve_on_action, cx);
                    }),
                );
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(reply.as_deref().map(|text| {
                    Self::reply_box(
                        ReplyBox {
                            colors,
                            thread_id: &id,
                            can_reply_now,
                            can_ask_agent,
                            on_action: &on_action,
                            on_reply_draft: &on_reply_draft,
                            ui: &ui,
                            focus: &focus,
                            is_busy,
                        },
                        text,
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
            .border_color(colors.border)
            .bg(colors.background)
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
