use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, anchored,
    deferred, div, px,
};

use crate::app::code_links::open_link;
use crate::viewer::{
    Audience, CommentId, HandOffId, Reaction, ReactionChange, ReactionCount, SendMode,
    ThreadAction, ThreadActionResult, ThreadComment, ThreadId, ThreadView,
};

use super::button::{
    ButtonKind, ButtonSize, button, button_icon, icon_button, icon_label_button, joined,
};
use super::card::card;
use super::chip::chip;
use super::icon::{Icon, sized_icon};
use super::markdown_view::{MarkdownView, parsed_markdown};
use super::motion::{BOX_RISE, Edge, STATE, busy_dot, slide_in, turn_to};
use super::palette::{CARD_RADIUS, CODE_FONT, CONTROL_RADIUS, Colors, Tone};
use super::text_box::{Field, TextBox};
use super::text_input::Lines;
use super::tooltip::tooltip;

pub type OnToggle = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

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

/// Queues a failed hand-off again.
pub type OnTryHandOffAgain = Rc<dyn Fn(HandOffId, &mut App)>;

/// What happened to the edit of one comment; the core keeps its text, so it survives scrolling away.
pub type OnCommentEdit = Rc<dyn Fn(&CommentId, ReplyDraftChange, &mut App)>;

/// What the reviewer is doing in one thread: the action under way. The reply text is the core's.
struct ThreadUi {
    /// The comment whose reaction picker is open.
    picking_reaction: Option<CommentId>,
    is_busy: bool,
    /// The comment whose new text is on its way to GitHub.
    saving_edit: Option<CommentId>,
    problem: Option<String>,
    focus: FocusHandle,
}

impl ThreadUi {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            picking_reaction: None,
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
    on_try_hand_off_again: OnTryHandOffAgain,
}

/// What the review holds about one thread: what the reviewer may do and what they have typed.
pub struct ThreadDrafts {
    pub can_reply_now: bool,
    pub can_ask_agent: bool,
    pub reply: Option<String>,
    /// The text of each comment being edited, as typed so far.
    pub edits: BTreeMap<CommentId, String>,
}

/// What the reviewer's clicks and typing in a thread change.
pub struct ThreadChanges {
    pub on_reply_draft: OnReplyDraft,
    pub on_comment_edit: OnCommentEdit,
    pub on_toggle: OnToggle,
    pub on_action: OnThreadAction,
    pub on_try_hand_off_again: OnTryHandOffAgain,
}

/// What an outdated thread's lines were when it was written, since the code under it shows what they are now.
fn before_block(colors: &Colors, before: &[String]) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(colors.muted_text)
                .child("When commented, the lines were:"),
        )
        .child(
            div()
                .py_1()
                .px_2()
                .rounded(px(CONTROL_RADIUS))
                .bg(colors.removed_line)
                .font_family(CODE_FONT)
                .text_sm()
                .overflow_hidden()
                .children(before.iter().map(|line| {
                    div()
                        .whitespace_nowrap()
                        .child(SharedString::from(line.clone()))
                })),
        )
}

impl ThreadElement {
    pub fn new(view: Rc<ThreadView>, drafts: ThreadDrafts, changes: ThreadChanges) -> Self {
        let ThreadDrafts {
            can_reply_now,
            can_ask_agent,
            reply,
            edits,
        } = drafts;
        let ThreadChanges {
            on_reply_draft,
            on_comment_edit,
            on_toggle,
            on_action,
            on_try_hand_off_again,
        } = changes;
        Self {
            view,
            can_reply_now,
            can_ask_agent,
            reply,
            edits,
            on_reply_draft,
            on_comment_edit,
            on_toggle,
            on_action,
            on_try_hand_off_again,
        }
    }

    /// What sets the thread apart, each in its state's tone: outdated, resolved, by a bot.
    fn marks(&self, colors: &Colors) -> Vec<gpui::Div> {
        let thread = &self.view.thread;
        let is_bot = thread
            .comments
            .first()
            .is_some_and(|comment| comment.is_bot);
        [
            (thread.is_outdated, Tone::Warning, "Outdated"),
            (thread.is_resolved, Tone::Success, "Resolved"),
            (is_bot, Tone::Neutral, "Bot"),
        ]
        .into_iter()
        .filter(|(is_on, _, _)| *is_on)
        .map(|(_, tone, label)| chip(colors, tone).child(label))
        .collect()
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

fn small_button(
    colors: &Colors,
    id: SharedString,
    kind: ButtonKind,
    icon: Icon,
    label: &'static str,
) -> gpui::Stateful<gpui::Div> {
    icon_label_button(colors, id, kind, ButtonSize::Small, icon, label)
}

fn plain_button(
    colors: &Colors,
    id: SharedString,
    kind: ButtonKind,
    label: &'static str,
) -> gpui::Stateful<gpui::Div> {
    button(colors, id, kind, ButtonSize::Small).child(label)
}

/// A reaction: just the emoji until someone reacts, then a pill with the count, on `current`
/// when the reviewer is one of them.
fn reaction_pill(
    colors: &Colors,
    id: SharedString,
    label: String,
    count: usize,
    viewer_has_reacted: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .h(px(24.))
        .min_w(px(28.))
        .px_1p5()
        .justify_center()
        .rounded_full()
        .border_1()
        .cursor_pointer()
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .map(|pill| {
            if viewer_has_reacted {
                pill.bg(colors.current)
                    .border_color(colors.current_border)
                    .text_color(colors.accent)
                    .hover(|pill| pill.border_color(colors.accent_border))
            } else if count > 0 {
                pill.bg(colors.surface)
                    .border_color(colors.subtle_border)
                    .text_color(colors.muted_text)
                    .hover(|pill| pill.bg(colors.hover).border_color(colors.border))
            } else {
                pill.border_color(gpui::transparent_black())
                    .hover(|pill| pill.bg(colors.hover))
            }
        })
        .active(|pill| pill.bg(colors.pressed))
        .child(label)
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
    /// The reactions someone gave, each with its count, then one button that opens the picker of all
    /// eight.
    fn reactions(
        colors: &Colors,
        comment: &ThreadComment,
        ui: &Entity<ThreadUi>,
        on_action: &OnThreadAction,
        cx: &App,
    ) -> AnyElement {
        let summary = comment.reaction_summary();
        let react = |reaction: Reaction, viewer_has_reacted: bool| {
            let (ui, on_action, comment_id) =
                (ui.clone(), Rc::clone(on_action), comment.id.clone());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                ui.update(cx, |ui, cx| {
                    ui.picking_reaction = None;
                    cx.notify();
                });
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
            }
        };
        let counted = summary.iter().filter(|count| count.count > 0).map(
            |&ReactionCount {
                 reaction,
                 count,
                 viewer_has_reacted,
             }| {
                reaction_pill(
                    colors,
                    SharedString::from(format!("react-{}-{}", comment.id, reaction.github_name())),
                    format!("{} {count}", reaction.emoji()),
                    count,
                    viewer_has_reacted,
                )
                .on_click(react(reaction, viewer_has_reacted))
            },
        );
        let is_picking = ui.read(cx).picking_reaction.as_ref() == Some(&comment.id);
        let toggle_picker = {
            let (ui, comment_id) = (ui.clone(), comment.id.clone());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                ui.update(cx, |ui, cx| {
                    ui.picking_reaction = if ui.picking_reaction.as_ref() == Some(&comment_id) {
                        None
                    } else {
                        Some(comment_id.clone())
                    };
                    cx.notify();
                });
            }
        };
        let add = icon_button(
            colors,
            SharedString::from(format!("react-add-{}", comment.id)),
            if is_picking {
                ButtonKind::Pressed
            } else {
                ButtonKind::Ghost
            },
            ButtonSize::Small,
            Icon::SmilePlus,
            "Add a reaction",
        )
        .h(px(24.))
        .w(px(28.))
        .on_click(toggle_picker);
        let picker = is_picking.then(|| {
            let close_ui = ui.clone();
            deferred(
                anchored().snap_to_window_with_margin(px(8.)).child(
                    div()
                        .id(SharedString::from(format!("react-picker-{}", comment.id)))
                        .mt(px(28.))
                        .p_1()
                        .flex()
                        .gap_0p5()
                        .bg(colors.overlay)
                        .border_1()
                        .border_color(colors.border)
                        .rounded(px(CARD_RADIUS))
                        .shadow(colors.overlay_shadow())
                        .on_mouse_down_out(move |_, _, cx| {
                            close_ui.update(cx, |ui, cx| {
                                ui.picking_reaction = None;
                                cx.notify();
                            });
                        })
                        .children(summary.iter().map(|count| {
                            reaction_pill(
                                colors,
                                SharedString::from(format!(
                                    "react-pick-{}-{}",
                                    comment.id,
                                    count.reaction.github_name()
                                )),
                                count.reaction.emoji().to_owned(),
                                0,
                                count.viewer_has_reacted,
                            )
                            .h(px(28.))
                            .min_w(px(32.))
                            .on_click(react(count.reaction, count.viewer_has_reacted))
                        })),
                ),
            )
            .with_priority(1)
        });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_0p5()
            .children(counted)
            .child(div().relative().child(add).children(picker))
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
        let cancel = (!is_busy).then(|| {
            icon_button(
                &colors,
                SharedString::from(format!("reply-cancel-{id}")),
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::Close,
                "Cancel the reply",
            )
            .on_click({
                let on_reply_draft = Rc::clone(on_reply_draft);
                move |_, _, cx| on_reply_draft(ReplyDraftChange::Discard, cx)
            })
        });
        let buttons = (!is_busy).then(|| {
            let ask_agent = can_ask_agent.then(|| {
                let mut parts = vec![
                    icon_label_button(
                        &colors,
                        SharedString::from(format!("reply-ask-{id}")),
                        ButtonKind::Joined,
                        ButtonSize::Small,
                        Icon::Sparkles,
                        "Ask agent",
                    )
                    .tooltip(tooltip(|_| {
                        "Ask agent: adds the reply to the review for your agent".into()
                    }))
                    .on_click(send(SendMode::AddToReview, Audience::Agent)),
                ];
                if can_reply_now {
                    parts.push(
                        button(
                            &colors,
                            SharedString::from(format!("reply-ask-now-{id}")),
                            ButtonKind::Joined,
                            ButtonSize::Small,
                        )
                        .child(button_icon(
                            &colors,
                            ButtonKind::Joined,
                            ButtonSize::Small,
                            Icon::Zap,
                        ))
                        .child("now")
                        .tooltip(tooltip(|_| "Ask agent now: posts the reply at once".into()))
                        .on_click(send(SendMode::CommentNow, Audience::Agent)),
                    );
                }
                joined(&colors, parts)
            });
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    button(
                        &colors,
                        SharedString::from(format!("reply-add-{id}")),
                        ButtonKind::Primary,
                        ButtonSize::Small,
                    )
                    .child("Add to review")
                    .on_click(send(SendMode::AddToReview, Audience::Author)),
                )
                .children(can_reply_now.then(|| {
                    plain_button(
                        &colors,
                        SharedString::from(format!("reply-now-{id}")),
                        ButtonKind::Secondary,
                        "Reply now",
                    )
                    .on_click(send(SendMode::CommentNow, Audience::Author))
                }))
                .children(ask_agent)
        });
        slide_in(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .relative()
                        .child(
                            Field::new(focus)
                                .pl_2()
                                .pr_8()
                                .py_1p5()
                                .min_h(px(56.))
                                .child(text),
                        )
                        .children(cancel.map(|cancel| {
                            div().absolute().top(px(4.)).right(px(4.)).child(cancel)
                        })),
                )
                .children(buttons),
            SharedString::from(format!("reply-box-{id}")),
            STATE,
            Edge::Bottom,
            BOX_RISE,
        )
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
                .flex_wrap()
                .gap_2()
                .child(
                    button(
                        &colors,
                        SharedString::from(format!("edit-save-{comment_id}")),
                        ButtonKind::Primary,
                        ButtonSize::Small,
                    )
                    .child("Save")
                    .on_click(move |_, _, cx| save(cx)),
                )
                .child(
                    button(
                        &colors,
                        SharedString::from(format!("edit-cancel-{comment_id}")),
                        ButtonKind::Ghost,
                        ButtonSize::Small,
                    )
                    .child("Cancel")
                    .on_click(move |_, _, cx| {
                        on_comment_edit(&cancelled_id, ReplyDraftChange::Discard, cx);
                    }),
                )
        });
        slide_in(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(Field::new(focus).px_2().py_1p5().min_h(px(56.)).child(body))
                .children(is_saving.then(|| busy_line(&colors, "Saving…")))
                .children(buttons),
            SharedString::from(format!("edit-box-{comment_id}")),
            STATE,
            Edge::Bottom,
            BOX_RISE,
        )
        .into_any_element()
    }
}

/// A line that says the thread is waiting on GitHub, with the dot that breathes while it waits.
fn busy_line(colors: &Colors, text: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_sm()
        .text_color(colors.muted_text)
        .child(busy_dot(colors, "thread-busy-dot"))
        .child(text)
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
        let marks = self.marks(&colors);
        let summary = format!(
            "{}{} · {first_author} · {comment_count} comment{}",
            if marks.is_empty() { "Thread · " } else { "" },
            self.where_it_is(),
            if comment_count == 1 { "" } else { "s" }
        );
        let toggle = div()
            .id(SharedString::from(format!("thread-toggle-{id}")))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .text_sm()
            .text_color(colors.muted_text)
            .hover(|toggle| toggle.text_color(colors.text))
            .on_click(self.on_toggle)
            .child(turn_to(
                sized_icon(Icon::ChevronRight, colors.muted_text, px(12.)),
                "thread-chevron",
                !self.view.is_folded,
                1.,
            ))
            .children(marks)
            .child(div().min_w_0().truncate().child(summary));
        let conversation_link = self.view.thread.comments.first().map(|comment| {
            let url = comment.url.clone();
            icon_button(
                &colors,
                SharedString::from(format!("thread-link-{id}")),
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::ExternalLink,
                "Open on GitHub",
            )
            .on_click(move |_, _, cx| open_link(&url, cx))
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
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(comment.author.clone()),
                            )
                            .children((comment.by_viewer && being_edited.is_none()).then(|| {
                                let (on_comment_edit, comment_id) =
                                    (Rc::clone(&self.on_comment_edit), comment.id.clone());
                                let edit_focus = window.use_keyed_state(
                                    SharedString::from(format!("edit-focus-{}", comment.id)),
                                    cx,
                                    |_, cx| cx.focus_handle(),
                                );
                                icon_button(
                                    &colors,
                                    SharedString::from(format!("edit-{}", comment.id)),
                                    ButtonKind::Ghost,
                                    ButtonSize::Small,
                                    Icon::Pencil,
                                    "Edit your comment",
                                )
                                .on_click(move |_, window, cx| {
                                    on_comment_edit(&comment_id, ReplyDraftChange::Open, cx);
                                    let handle = edit_focus.read(cx).clone();
                                    window.focus(&handle, cx);
                                })
                            }))
                            .child(Self::reactions(
                                &colors,
                                comment,
                                &ui,
                                &reaction_on_action,
                                cx,
                            ));
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
                            .child(div().pl_4().child(content))
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
        let resolve = (!self.view.is_folded).then(|| {
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
            let (resolve_icon, resolve_label) = if thread.is_resolved {
                (Icon::RotateCcw, "Unresolve")
            } else {
                (Icon::CircleCheck, "Resolve")
            };
            let (resolve_ui, resolve_on_action) = (ui.clone(), Rc::clone(&on_action));
            small_button(
                &colors,
                SharedString::from(format!("resolve-{id}")),
                ButtonKind::Ghost,
                resolve_icon,
                resolve_label,
            )
            .when(is_busy, |button| button.opacity(0.5))
            .on_click(move |_, _, cx| {
                run_action(resolve_action.clone(), &resolve_ui, &resolve_on_action, cx);
            })
        });
        let actions = (!self.view.is_folded).then(|| {
            let open_reply = Rc::clone(&on_reply_draft);
            let reply_focus = focus.clone();
            let is_replying = reply.is_some();
            let reply_button = (!is_replying).then(|| {
                small_button(
                    &colors,
                    SharedString::from(format!("reply-{id}")),
                    ButtonKind::Secondary,
                    Icon::Reply,
                    "Reply",
                )
                .when(is_busy, |button| button.opacity(0.5))
                .on_click(move |_, window, cx| {
                    open_reply(ReplyDraftChange::Open, cx);
                    window.focus(&reply_focus, cx);
                })
            });
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
                .children(reply_button.map(|button| div().flex().child(button)))
                .children(is_busy.then(|| busy_line(&colors, "Working...")))
                .children(
                    problem.map(|problem| div().text_sm().text_color(colors.danger).child(problem)),
                )
        });
        card(&colors)
            .flex()
            .flex_col()
            .gap_3()
            .m_2()
            .px_3()
            .py_2()
            .font_family(super::palette::TEXT_FONT)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(toggle)
                    .children(resolve)
                    .children(conversation_link),
            )
            .children(
                (!self.view.is_folded)
                    .then(|| self.view.followed.as_ref())
                    .flatten()
                    .filter(|followed| followed.were_changed)
                    .map(|followed| before_block(&colors, &followed.before)),
            )
            .children(comments)
            .children(self.view.hand_off_line.as_ref().map(|line| {
                let try_again = self.view.failed_hand_off.map(|hand_off| {
                    let on_try_again = Rc::clone(&self.on_try_hand_off_again);
                    small_button(
                        &colors,
                        SharedString::from(format!("try-hand-off-again-{id}")),
                        ButtonKind::Secondary,
                        Icon::Refresh,
                        "Try again",
                    )
                    .on_click(move |_, _, cx| on_try_again(hand_off, cx))
                });
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(if try_again.is_some() {
                        colors.danger
                    } else {
                        colors.muted_text
                    })
                    .child(SharedString::from(Arc::clone(line)))
                    .children(try_again)
            }))
            .children(actions)
    }
}
