//! The comment boxes and threads on the page and the finish dialog, wired to the session.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Window,
};

use super::OneStepLayout;
use crate::app::components::button::{ButtonKind, ButtonSize, button, button_icon};
use crate::app::components::comment_composer::{
    CommentCapabilities, CommentComposer, ComposerActions, OnCompose, QueuedCommentNote,
};
use crate::app::components::finish_review::{FinishReviewDialog, OnBodyEdit, OnFinish, OnMarkDone};
use crate::app::components::icon::Icon;
use crate::app::components::palette::Colors;
use crate::app::components::thread_view::{
    DeleteQuestion, OnCommentEdit, OnDeleteQuestion, OnHandOffButton, OnHandToAgent, OnReplyDraft,
    OnThreadAction, ReplyDraftChange, ThreadChanges, ThreadDrafts, ThreadElement,
};
use crate::app::review_session::ReviewSession;
use crate::app::sidebar_rows::SidebarRows;
use crate::viewer::{
    Audience, CommentBox, CommentPlace, Intent, QueuedComment, SendMode, TextBox, ThreadId,
    ThreadView,
};

impl OneStepLayout {
    fn on_compose(
        &self,
        place: &CommentPlace,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, CommentPlace, &mut Window, &mut Context<Self>) + 'static,
    ) -> OnCompose {
        let (layout, place) = (cx.entity(), place.clone());
        Rc::new(move |window, cx| {
            layout.update(cx, |this, cx| act(this, place.clone(), window, cx));
        })
    }

    pub(super) fn queued_comment(
        &self,
        queued: &QueuedComment,
        cx: &mut Context<Self>,
    ) -> QueuedCommentNote {
        let on_discard = self.on_compose(&queued.draft.place, cx, |this, place, _, cx| {
            this.session.update(cx, |session, cx| {
                session.discard(&TextBox::Draft(place), cx);
            });
        });
        QueuedCommentNote::new(queued.clone(), on_discard)
    }

    /// The draft is read here, so typing redraws the box without new rows.
    pub(super) fn composer(
        &self,
        place: &CommentPlace,
        cx: &mut Context<Self>,
    ) -> Option<CommentComposer> {
        let session = self.session.read(cx);
        let review = session.review();
        let CommentBox {
            draft,
            state,
            problem,
        } = review.comment_box(place)?;
        let draft = draft.clone();
        let capabilities = CommentCapabilities {
            comment_now: review.can_comment_now(),
            suggest_change: review.can_suggest_change(place),
            ask_agent: session.agent_link_is_on(),
            sending: review.sending(),
        };
        let on_type = {
            let (layout, draft) = (cx.entity(), TextBox::Draft(place.clone()));
            Rc::new(move |body: String, _: &mut Window, cx: &mut App| {
                layout.update(cx, |this, cx| {
                    this.session
                        .update(cx, |session, cx| session.type_in(&draft, &body, cx));
                });
            })
        };
        let actions = ComposerActions {
            on_focus: self.on_compose(place, cx, |this, place, window, cx| {
                let takes_keys = this.session.update(cx, |session, cx| {
                    session.compose(place.clone(), cx);
                    session.review().composing() == Some(&place)
                });
                if takes_keys {
                    window.focus(&this.composer_focus, cx);
                }
            }),
            on_type,
            on_stop: self.on_compose(place, cx, |this, place, window, cx| {
                this.session
                    .update(cx, |session, cx| session.leave(&TextBox::Draft(place), cx));
                window.focus(&this.focus, cx);
            }),
            on_add_to_review: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(&place, SendMode::AddToReview, Audience::Author, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_comment_now: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(&place, SendMode::CommentNow, Audience::Author, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_ask_agent: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(&place, SendMode::AddToReview, Audience::Agent, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_ask_agent_now: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.send_comment(&place, SendMode::CommentNow, Audience::Agent, cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_suggest_change: self.on_compose(place, cx, |this, place, window, cx| {
                // A refused or failed save is one of the review's problems, which the sidebar shows.
                this.change_review(cx, |review| review.suggest_change(&place))
                    .ok();
                window.focus(&this.composer_focus, cx);
            }),
            on_discard: self.on_compose(place, cx, |this, place, window, cx| {
                this.session.update(cx, |session, cx| {
                    session.discard(&TextBox::Draft(place), cx);
                });
                window.focus(&this.focus, cx);
            }),
            on_send_task: {
                let (layout, place) = (cx.entity(), place.clone());
                Rc::new(move |mode, window: &mut Window, cx: &mut App| {
                    layout.update(cx, |this, cx| {
                        this.session.update(cx, |session, cx| {
                            session.send_comment(&place, mode, Audience::Agent, cx);
                        });
                        window.focus(&this.focus, cx);
                    });
                })
            },
        };
        Some(CommentComposer::new(
            draft,
            self.composer_focus.clone(),
            state,
            capabilities,
            problem,
            actions,
        ))
    }

    fn on_thread_action(&self) -> OnThreadAction {
        let session = self.session.clone();
        Rc::new(move |action, on_done, cx| {
            session.update(cx, |session, cx| session.act_on_thread(action, on_done, cx));
        })
    }

    fn on_hand_off_button(&self) -> OnHandOffButton {
        let session = self.session.clone();
        Rc::new(move |button, cx| {
            session.update(cx, |session, cx| session.press_hand_off_button(button, cx));
        })
    }

    fn on_hand_to_agent(&self) -> OnHandToAgent {
        let session = self.session.clone();
        Rc::new(move |comment, cx| {
            session.update(cx, |session, cx| session.hand_to_agent(comment, cx));
        })
    }

    /// Reply text goes to the core, which saves it, so it survives scrolling and restarts.
    fn on_reply_draft(&self, thread_id: &ThreadId) -> OnReplyDraft {
        let (session, reply) = (self.session.clone(), TextBox::Reply(thread_id.clone()));
        Rc::new(move |change, cx| {
            session.update(cx, |session, cx| change_box(session, &reply, change, cx));
        })
    }

    /// The text of an edit goes to the core, which keeps it until it is sent or given up.
    fn on_comment_edit(&self) -> OnCommentEdit {
        let session = self.session.clone();
        Rc::new(move |comment_id, change, cx| {
            let edit = TextBox::Edit(comment_id.clone());
            session.update(cx, |session, cx| change_box(session, &edit, change, cx));
        })
    }

    fn on_delete_question(&self) -> OnDeleteQuestion {
        let session = self.session.clone();
        Rc::new(move |comment_id, question, cx| {
            session.update(cx, |session, cx| {
                session.change(cx, |review| match question {
                    DeleteQuestion::Ask => review.ask_to_delete(comment_id),
                    DeleteQuestion::Withdraw => review.withdraw_delete(),
                });
            });
        })
    }

    fn on_finish_body_edit(&self) -> OnBodyEdit {
        let session = self.session.clone();
        Rc::new(move |text, cx| {
            session.update(cx, |session, cx| {
                session.type_in(&TextBox::FinishBody, &text, cx);
            });
        })
    }

    pub(super) fn finish_review_dialog(
        &self,
        sidebar: &SidebarRows,
        is_everything_read: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        FinishReviewDialog::new(
            is_everything_read,
            sidebar.finishing,
            sidebar.approve_warning,
            sidebar.finish,
            self.session
                .read(cx)
                .review()
                .text_of(&TextBox::FinishBody)
                .unwrap_or_default()
                .to_owned(),
            self.on_finish_body_edit(),
            self.on_finish_review(),
        )
        .on_mark_done(Self::on_mark_done(cx))
        .opened_by(self.finish_form_request)
        .into_any_element()
    }

    /// A local review's finish: no verdict and no body, only its queued tasks to send.
    pub(super) fn send_tasks_button(
        colors: &Colors,
        queued: usize,
        is_everything_read: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let kind = if is_everything_read {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        };
        button(colors, "send-tasks", kind, ButtonSize::Regular)
            .child(button_icon(colors, kind, ButtonSize::Regular, Icon::Send))
            .child(SharedString::from(format!("Send tasks ({queued})")))
            .aria_label(format!("Send tasks ({queued})"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.act(Intent::SendTasks, window, cx);
            }))
    }

    fn on_mark_done(cx: &Context<Self>) -> OnMarkDone {
        let layout = cx.entity().downgrade();
        Rc::new(move |window, cx| {
            // A layout already gone has no review left to mark.
            layout
                .update(cx, |layout, cx| layout.act(Intent::MarkDone, window, cx))
                .ok();
        })
    }

    fn on_finish_review(&self) -> OnFinish {
        let session = self.session.clone();
        Rc::new(move |verdict, body, on_done, cx| {
            session.update(cx, |session, cx| {
                session.finish_review(verdict, &body, on_done, cx);
            });
        })
    }

    pub(super) fn thread_element(
        &self,
        view: &Arc<ThreadView>,
        cx: &mut Context<Self>,
    ) -> ThreadElement {
        let id = view.thread.id.clone();
        let review = self.session.read(cx).review();
        let can_comment_now = review.can_comment_now();
        let can_ask_agent = self.session.read(cx).agent_link_is_on();
        let reply = review
            .text_of(&TextBox::Reply(view.thread.id.clone()))
            .map(str::to_owned);
        let edits = view
            .thread
            .comments
            .iter()
            .filter_map(|comment| {
                let text = review.text_of(&TextBox::Edit(comment.id.clone()))?;
                Some((comment.id.clone(), text.to_owned()))
            })
            .collect();
        ThreadElement::new(
            Arc::clone(view),
            ThreadDrafts {
                can_reply_now: can_comment_now,
                can_ask_agent,
                can_react: review.can_react_to_comments(),
                sending: review.sending(),
                reply,
                edits,
            },
            ThreadChanges {
                on_reply_draft: self.on_reply_draft(&view.thread.id),
                on_comment_edit: self.on_comment_edit(),
                on_delete_question: self.on_delete_question(),
                on_toggle: Box::new(cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| review.toggle_thread_fold(&id));
                })),
                on_action: self.on_thread_action(),
                on_hand_off_button: self.on_hand_off_button(),
                on_hand_to_agent: self.on_hand_to_agent(),
            },
        )
    }
}

fn change_box(
    session: &mut ReviewSession,
    text_box: &TextBox,
    change: ReplyDraftChange,
    cx: &mut Context<ReviewSession>,
) {
    match change {
        ReplyDraftChange::Edit(text) => session.type_in(text_box, &text, cx),
        ReplyDraftChange::Open => {
            session.change(cx, |review| review.open_box(text_box)).ok();
        }
        ReplyDraftChange::Discard => session.discard(text_box, cx),
    }
}
