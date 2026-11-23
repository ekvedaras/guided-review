//! A finding of the AI review on the page: a card marked as AI, with its severity, under the line it is about,
//! and marked outdated with what its lines said once new commits changed them.

use std::collections::HashMap;

use gpui::{
    AnyElement, App, Context, Div, FocusHandle, IntoElement, ParentElement, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};

use crate::viewer::{KeyCap, Modifier};

use super::card::card;
use super::chip::chip;
use super::icon::Icon;
use super::key_cap::{KeyLabel, key_hint};
use super::markdown_view::{MarkdownView, parsed_markdown};
use super::palette::{Colors, Tone};
use super::text_box::{Field, TextBox};
use super::text_input::Lines;
use super::thread_view::before_block;
use crate::findings::{FindingId, Severity};
use crate::viewer::{FindingRow, WhyAuthor, WhyMessage};

pub fn tone_of(severity: Severity) -> Tone {
    match severity {
        Severity::MustFix => Tone::Danger,
        Severity::ShouldFix => Tone::Warning,
        Severity::Nit => Tone::Neutral,
    }
}

/// The marks of a finding, on its card and on the thread or queued comment made from it.
pub fn finding_chips(colors: &Colors, severity: Severity) -> [Div; 2] {
    [
        chip(colors, Tone::Accent).child("AI"),
        chip(colors, tone_of(severity)).child(severity.to_string()),
    ]
}

/// Ask why and the fix requests on a finding, oldest first, each under who said it.
pub fn talk<'a>(
    colors: &'a Colors,
    id: &'a FindingId,
    said: &'a [WhyMessage],
) -> impl Iterator<Item = Div> + 'a {
    said.iter().enumerate().map(move |(at, said)| {
        let who = match said.author {
            WhyAuthor::Reviewer => "You",
            WhyAuthor::ReviewAgent => "Review agent",
            WhyAuthor::TourAgent => "Tour agent",
        };
        div()
            .pl_3()
            .border_l_2()
            .border_color(colors.border)
            .flex()
            .flex_col()
            .gap_0p5()
            .child(div().text_xs().text_color(colors.muted_text).child(who))
            .child(
                MarkdownView::new(
                    format!("finding-{id}-why-{at}"),
                    parsed_markdown(&said.body),
                )
                .into_element(),
            )
    })
}

/// `actions` are the buttons that decide the finding, which the layout makes because their clicks are its.
/// `question` is the box the reviewer types an Ask why in, when one is open on this finding.
pub fn finding_card(
    colors: &Colors,
    row: &FindingRow,
    question: Option<AnyElement>,
    actions: impl IntoElement,
) -> Div {
    card(colors)
        .m_2()
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .children(finding_chips(colors, row.severity))
                .children(
                    row.outdated
                        .as_ref()
                        .map(|_| chip(colors, Tone::Warning).child("Outdated")),
                ),
        )
        .child(
            MarkdownView::new(format!("finding-{}", row.id), parsed_markdown(&row.body))
                .into_element(),
        )
        .children(
            row.outdated
                .as_ref()
                .map(|before| before_block(colors, "When found, the lines were:", before)),
        )
        .children(row.fix.as_ref().map(|line| {
            div()
                .text_sm()
                .text_color(colors.muted_text)
                .child(line.to_string())
        }))
        .children(talk(colors, &row.id, &row.thread))
        .children(row.asked.as_ref().map(|line| {
            div()
                .text_sm()
                .text_color(colors.muted_text)
                .child(line.to_string())
        }))
        .children(question)
        .child(actions)
}

/// The Ask why boxes open on findings: what is typed in each, and the focus it takes keys with. A box
/// goes when its question is sent or given up.
#[derive(Default)]
pub struct FindingQuestions(HashMap<FindingId, OpenQuestion>);

struct OpenQuestion {
    text: String,
    purpose: FindingBox,
    focus: FocusHandle,
}

/// What the owner of the boxes does on each key.
pub struct FindingBoxKeys<T> {
    pub on_type: fn(&mut T, &FindingId, String),
    pub on_send: fn(&mut T, &FindingId, &mut Window, &mut Context<T>),
    /// Only a fix note's box has it.
    pub on_send_now: fn(&mut T, &FindingId, &mut Window, &mut Context<T>),
    pub on_cancel: fn(&mut T, &FindingId, &mut Window, &mut Context<T>),
}

impl<T> Clone for FindingBoxKeys<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for FindingBoxKeys<T> {}

/// What a box under a finding is for, which says what its keys do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingBox {
    WhyQuestion,
    FixAnswer,
    /// Enter adds the finding to fixes with the note; the fix-now key sends it at once.
    FixNote,
}

impl FindingBox {
    const fn send_hint(self) -> &'static str {
        match self {
            Self::WhyQuestion => "to ask",
            Self::FixAnswer => "to answer",
            Self::FixNote => "to add to fixes",
        }
    }
}

impl FindingQuestions {
    /// Opens the box on the finding with `text` in it, or finds the one open; returns its focus.
    pub fn open(
        &mut self,
        id: &FindingId,
        text: &str,
        purpose: FindingBox,
        cx: &mut App,
    ) -> FocusHandle {
        self.0
            .entry(id.clone())
            .or_insert_with(|| OpenQuestion {
                text: text.to_owned(),
                purpose,
                focus: cx.focus_handle(),
            })
            .focus
            .clone()
    }

    pub fn purpose_of(&self, id: &FindingId) -> Option<FindingBox> {
        self.0.get(id).map(|open| open.purpose)
    }

    pub fn type_in(&mut self, id: &FindingId, text: String) {
        if let Some(open) = self.0.get_mut(id) {
            open.text = text;
        }
    }

    pub fn text_of(&self, id: &FindingId) -> Option<&str> {
        self.0.get(id).map(|open| open.text.as_str())
    }

    pub fn close(&mut self, id: &FindingId) {
        self.0.remove(id);
    }

    /// The box and its hints, when one is open on the finding. The owner says what its keys do.
    pub fn view<T: 'static>(
        &self,
        colors: &Colors,
        id: &FindingId,
        layout: gpui::Entity<T>,
        keys: FindingBoxKeys<T>,
    ) -> Option<AnyElement> {
        let FindingBoxKeys {
            on_type,
            on_send,
            on_send_now,
            on_cancel,
        } = keys;
        let open = self.0.get(id)?;
        let has_send_now = open.purpose == FindingBox::FixNote;
        let (typed, sent, sent_now, given_up) = (
            (layout.clone(), id.clone()),
            (layout.clone(), id.clone()),
            (layout.clone(), id.clone()),
            (layout, id.clone()),
        );
        let text_box = TextBox::new(
            SharedString::from(format!("finding-question-{id}")),
            &open.focus,
            &open.text,
            Lines::Single,
        )
        .on_edit(move |text, _, cx| {
            let (layout, id) = &typed;
            layout.update(cx, |this, cx| {
                on_type(this, id, text);
                cx.notify();
            });
        })
        .on_submit(move |window, cx| {
            let (layout, id) = &sent;
            layout.update(cx, |this, cx| on_send(this, id, window, cx));
        })
        .on_submit_now(has_send_now, move |window, cx| {
            let (layout, id) = &sent_now;
            layout.update(cx, |this, cx| on_send_now(this, id, window, cx));
        })
        .on_cancel(move |window, cx| {
            let (layout, id) = &given_up;
            layout.update(cx, |this, cx| on_cancel(this, id, window, cx));
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    Field::new(&open.focus)
                        .px_3()
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .child(text_box),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child(key_hint(
                            colors,
                            &[KeyLabel::Icon(Icon::Enter)],
                            open.purpose.send_hint(),
                        ))
                        .when(has_send_now, |hints| {
                            hints.child(key_hint(
                                colors,
                                &[
                                    KeyLabel::of(KeyCap::Modifier(Modifier::Primary)),
                                    KeyLabel::of(KeyCap::Modifier(Modifier::Alt)),
                                    KeyLabel::Icon(Icon::Enter),
                                ],
                                "to fix now",
                            ))
                        })
                        .child(key_hint(colors, &[KeyLabel::Text("esc")], "to close")),
                )
                .into_any_element(),
        )
    }
}
