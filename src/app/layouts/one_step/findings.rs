//! The AI review's findings on the page: their buttons, the questions to the agent and the fixes.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Div, Entity, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    div,
};

use super::OneStepLayout;
use crate::app::components::button::{ButtonKind, ButtonSize, icon_label_button};
use crate::app::components::finding_view::FindingBox;
use crate::app::components::findings_sheet::{FindingsEvent, FindingsSheet};
use crate::app::components::icon::Icon;
use crate::app::components::palette::Colors;
use crate::app::components::toast_stack::post_toast;
use crate::app::review_session::ReviewSession;
use crate::findings::FindingId;
use crate::viewer::{CommentPlace, ToastTone};
use crate::viewer::{
    DEFAULT_WHY_QUESTION, FindingRefusal, FindingRow, FixNote, FixTiming, FixesOffer, WhyAsked,
};

impl OneStepLayout {
    /// Use, Use as is and Dismiss, which decide one finding; Fix now and Add to fixes, which hand it to the tour agent.
    pub(super) fn finding_actions(
        &self,
        colors: &Colors,
        finding: &FindingRow,
        cx: &mut Context<Self>,
    ) -> Div {
        let small_button = |name: &str, kind, icon, label| {
            icon_label_button(
                colors,
                SharedString::from(format!("finding-{name}-{}", finding.id)),
                kind,
                ButtonSize::Small,
                icon,
                label,
            )
        };
        let (use_id, as_is_id, dismiss_id, fix_id, why_id, answer_id, fixes_id, note_id, fixed_id) = (
            finding.id.clone(),
            finding.id.clone(),
            finding.id.clone(),
            finding.id.clone(),
            finding.id.clone(),
            finding.id.clone(),
            finding.id.clone(),
            finding.id.clone(),
            finding.id.clone(),
        );
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .when(finding.can_mark_fixed, |actions| {
                actions.child(
                    small_button("mark-fixed", ButtonKind::Primary, Icon::Check, "Mark fixed")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let marked = this
                                .session
                                .update(cx, |session, cx| session.mark_fixed(&fixed_id, cx));
                            this.finding_refused(marked, cx);
                        })),
                )
            })
            .when(finding.can_decide, |actions| {
                actions
                    .child(
                        small_button("use", ButtonKind::Secondary, Icon::Pencil, "Use").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.use_finding(&use_id, window, cx);
                            }),
                        ),
                    )
                    .child(
                        small_button("use-as-is", ButtonKind::Ghost, Icon::Send, "Use as is")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.use_finding_as_is(&as_is_id, window, cx);
                            })),
                    )
            })
            .when(finding.can_fix, |actions| {
                actions.child(
                    small_button("fix", ButtonKind::Ghost, Icon::Zap, "Fix now").on_click(
                        cx.listener(move |this, _, _, cx| {
                            let refused = this
                                .session
                                .update(cx, |session, cx| {
                                    session.fix_findings(std::slice::from_ref(&fix_id), cx)
                                })
                                .into_iter()
                                .next();
                            this.finding_refused(
                                refused.map_or(Ok(()), |(_, refusal)| Err(refusal)),
                                cx,
                            );
                        }),
                    ),
                )
            })
            .when_some(
                Self::fixes_offer_button(finding.fixes_offer),
                |actions, (kind, icon, label)| {
                    let offer = finding.fixes_offer;
                    actions.child(
                        small_button("fixes", kind, icon, label).on_click(cx.listener(
                            move |this, _, _, cx| this.answer_fixes_offer(offer, &fixes_id, cx),
                        )),
                    )
                },
            )
            .when(finding.can_fix, |actions| {
                actions.child(
                    small_button(
                        "fix-note",
                        ButtonKind::Ghost,
                        Icon::FilePen,
                        "Fix with note",
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_fix_note(&note_id, window, cx);
                    })),
                )
            })
            .when(finding.can_answer_fix, |actions| {
                actions.child(
                    small_button("answer-fix", ButtonKind::Primary, Icon::Send, "Answer").on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.open_fix_answer(&answer_id, window, cx);
                        }),
                    ),
                )
            })
            .when(finding.can_ask_why, |actions| {
                actions.child(
                    small_button("ask-why", ButtonKind::Ghost, Icon::Sparkles, "Ask why").on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.open_why_question(&why_id, window, cx);
                        }),
                    ),
                )
            })
            .when(finding.can_decide, |actions| {
                actions.child(
                    small_button("dismiss", ButtonKind::Ghost, Icon::Close, "Dismiss").on_click(
                        cx.listener(move |this, _, _, cx| {
                            let dismissed = this
                                .session
                                .update(cx, |session, cx| session.dismiss_finding(&dismiss_id, cx));
                            this.finding_refused(dismissed, cx);
                        }),
                    ),
                )
            })
    }

    fn fixes_offer_button(offer: FixesOffer) -> Option<(ButtonKind, Icon, &'static str)> {
        match offer {
            FixesOffer::Add => Some((ButtonKind::Secondary, Icon::Plus, "Add to fixes")),
            FixesOffer::Remove => Some((ButtonKind::Ghost, Icon::Minus, "Remove from fixes")),
            FixesOffer::None => None,
        }
    }

    fn answer_fixes_offer(&mut self, offer: FixesOffer, id: &FindingId, cx: &mut Context<Self>) {
        let refused = match offer {
            FixesOffer::Add => self
                .session
                .update(cx, |session, cx| {
                    session.add_to_fixes(std::slice::from_ref(id), cx)
                })
                .into_iter()
                .next()
                .map_or(Ok(()), |(_, refusal)| Err(refusal)),
            FixesOffer::Remove => self
                .session
                .update(cx, |session, cx| session.remove_from_fixes(id, cx)),
            FixesOffer::None => Ok(()),
        };
        self.finding_refused(refused, cx);
    }

    /// What the findings list asked: a batch is done by the session, and the list shows what is left.
    pub(super) fn findings_asked(
        &mut self,
        sheet: &Entity<FindingsSheet>,
        event: &FindingsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let refused = match event {
            FindingsEvent::Dismiss(ids) => self
                .session
                .update(cx, |session, cx| session.dismiss_findings(ids, cx)),
            FindingsEvent::Restore(ids) => self
                .session
                .update(cx, |session, cx| session.restore_findings(ids, cx)),
            FindingsEvent::Comment(ids) => self
                .session
                .update(cx, |session, cx| session.comment_findings(ids, cx)),
            FindingsEvent::Fix(ids) => self
                .session
                .update(cx, |session, cx| session.fix_findings(ids, cx)),
            FindingsEvent::GoTo(id) => {
                self.session.update(cx, |session, cx| {
                    session.change(cx, |review| review.go_to_finding(id));
                });
                self.scroll_to_finding(id, cx);
                return;
            }
        };
        if let Some((_, refusal)) = refused.first() {
            let others = refused.len().saturating_sub(1);
            let note = if others == 0 {
                refusal.to_string()
            } else {
                format!("{refusal}, and {others} more were left open")
            };
            post_toast(cx, ToastTone::Neutral, note);
        }
        let list = self.session.read(cx).review().findings_list();
        sheet.update(cx, |sheet, cx| sheet.refresh(&list, window, cx));
        cx.notify();
    }

    fn use_finding(&mut self, id: &FindingId, window: &mut Window, cx: &mut Context<Self>) {
        let used = self.session.update(cx, |session, cx| {
            session.change(cx, |review| review.use_finding(id))
        });
        match used {
            Ok(place) => self.go_to_comment_box(place, window, cx),
            Err(refusal) => self.finding_refused(Err(refusal), cx),
        }
    }

    /// Use as is refused for a box open on the finding's lines: the reviewer is taken to that box.
    fn use_finding_as_is(&mut self, id: &FindingId, window: &mut Window, cx: &mut Context<Self>) {
        let sent = self
            .session
            .update(cx, |session, cx| session.use_finding_as_is(id, cx));
        if let Err(FindingRefusal::DraftInTheWay(place)) = &sent {
            self.go_to_comment_box(place.clone(), window, cx);
        }
        self.finding_refused(sent, cx);
    }

    /// The box takes the keys and the page scrolls until it shows.
    fn go_to_comment_box(
        &mut self,
        place: CommentPlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self.session.read(cx).rows().row_of_comment_box(&place);
        self.session
            .update(cx, |session, cx| session.compose(place, cx));
        window.focus(&self.composer_focus, cx);
        if let Some(row) = row {
            self.screen_list.scroll_to_reveal_item(row);
        }
        cx.notify();
    }

    /// Ask why opens a box under the finding, with the usual question in it to ask as is or to edit.
    fn open_why_question(&mut self, id: &FindingId, window: &mut Window, cx: &mut Context<Self>) {
        let focus =
            self.finding_questions
                .open(id, DEFAULT_WHY_QUESTION, FindingBox::WhyQuestion, cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Answer opens the same box under the finding, empty, for what the tour agent asked before fixing.
    fn open_fix_answer(&mut self, id: &FindingId, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self
            .finding_questions
            .open(id, "", FindingBox::FixAnswer, cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Fix with note opens the box under the finding, empty, for what the reviewer wants of the fix.
    fn open_fix_note(&mut self, id: &FindingId, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.finding_questions.open(id, "", FindingBox::FixNote, cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn type_why_question(&mut self, id: &FindingId, text: String) {
        self.finding_questions.type_in(id, text);
    }

    pub(super) fn give_up_why_question(
        &mut self,
        id: &FindingId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finding_questions.close(id);
        window.focus(&self.focus, cx);
    }

    /// The typed question goes to the review agent on the finding's thread. A question that was
    /// refused stays in its box.
    pub(super) fn send_finding_box(
        &mut self,
        id: &FindingId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.finding_questions.purpose_of(id) {
            Some(FindingBox::WhyQuestion) => self.send_why_question(id, window, cx),
            Some(FindingBox::FixAnswer) => self.send_fix_answer(id, window, cx),
            Some(FindingBox::FixNote) => {
                self.send_fix_note(id, FixTiming::WithSendFixes, window, cx);
            }
            None => {}
        }
    }

    pub(super) fn send_finding_box_now(
        &mut self,
        id: &FindingId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.finding_questions.purpose_of(id) == Some(FindingBox::FixNote) {
            self.send_fix_note(id, FixTiming::Now, window, cx);
        }
    }

    /// The note goes with the fix, now or with Send fixes. A blank note sends nothing; a refused one stays in its box.
    fn send_fix_note(
        &mut self,
        id: &FindingId,
        timing: FixTiming,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(note) = FixNote::new(self.finding_questions.text_of(id).unwrap_or_default())
        else {
            return;
        };
        let sent = self.session.update(cx, |session, cx| {
            session.fix_with_note(id, note, timing, cx)
        });
        if sent.is_ok() {
            self.finding_questions.close(id);
            window.focus(&self.focus, cx);
        }
        self.finding_refused(sent, cx);
    }

    fn send_why_question(&mut self, id: &FindingId, window: &mut Window, cx: &mut Context<Self>) {
        let question = self
            .finding_questions
            .text_of(id)
            .unwrap_or_default()
            .to_owned();
        let asked = self
            .session
            .update(cx, |session, cx| session.ask_why(id, &question, cx));
        if asked.is_ok() {
            self.finding_questions.close(id);
            if asked == Ok(WhyAsked::ToReviewAgent) {
                window.focus(&self.focus, cx);
            }
        }
        if asked == Ok(WhyAsked::ToReviewAgent) {
            self.session.update(cx, ReviewSession::deliver_questions);
        }
        self.finding_refused(asked.map(|_| ()), cx);
    }

    /// The typed answer goes to the tour agent when it is idle. An answer that was refused stays in its box.
    fn send_fix_answer(&mut self, id: &FindingId, window: &mut Window, cx: &mut Context<Self>) {
        let answer = self
            .finding_questions
            .text_of(id)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if answer.is_empty() {
            return;
        }
        let sent = self.session.update(cx, |session, cx| {
            session.answer_fix_question(id, &answer, cx)
        });
        if sent.is_ok() {
            self.finding_questions.close(id);
            window.focus(&self.focus, cx);
        }
        self.finding_refused(sent, cx);
    }

    fn finding_refused(&mut self, done: Result<(), FindingRefusal>, cx: &mut Context<Self>) {
        if let Err(refusal) = done {
            post_toast(cx, ToastTone::Neutral, refusal.to_string());
        }
        cx.notify();
    }
}
