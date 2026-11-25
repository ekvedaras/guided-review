//! The Pull requests page: a comment box's send actions and the merge method.

use gpui::{Context, IntoElement, ParentElement, Styled, div, px};

use super::super::palette::Colors;
use super::super::segmented::{Segment, SegmentLayout, Segmented};
use super::{Editing, SettingsPanel, setting_row};
use crate::viewer::{Command, MergeMethod, SendAction, SendRole};

impl SettingsPanel {
    /// The Comments section: the two send actions that hold the main keys.
    pub(super) fn comment_actions(
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::send_action_row(
                editing,
                SendRole::Primary,
                colors,
                cx,
            ))
            .child(Self::send_action_row(
                editing,
                SendRole::Secondary,
                colors,
                cx,
            ))
    }

    fn send_action_row(
        editing: &Editing,
        role: SendRole,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let settings = &editing.settings;
        let (name, id, chosen, other, about) = match role {
            SendRole::Primary => (
                "Primary",
                "primary-comment-action",
                settings.primary_comment_action(),
                settings.secondary_comment_action(),
                "The accent button, first in the row",
            ),
            SendRole::Secondary | SendRole::Other => (
                "Secondary",
                "secondary-comment-action",
                settings.secondary_comment_action(),
                settings.primary_comment_action(),
                "The second button in the row",
            ),
        };
        let segments = SendAction::ALL
            .into_iter()
            .map(|action| {
                Segment::new(
                    action.config_text(),
                    action.label(),
                    cx.listener(move |this, _, _, cx| {
                        this.edit(cx, |editing| match role {
                            SendRole::Primary => {
                                editing.settings.set_primary_comment_action(action);
                            }
                            SendRole::Secondary | SendRole::Other => {
                                editing.settings.set_secondary_comment_action(action);
                            }
                        });
                    }),
                )
            })
            .collect();
        let key = Command::from(chosen)
            .default_chords_for(settings.send_choice())
            .first()
            .map_or_default(|chord| chord.spelled());
        let note = div().text_xs().text_color(colors.muted_text).child(format!(
            "{about}, with {key}. Picking {} here swaps the two. A key set under Keys wins.",
            other.label()
        ));
        setting_row(
            name,
            note,
            Segmented::new(
                id,
                SegmentLayout::Row {
                    segment_width: px(116.),
                },
                segments,
            )
            .picked(SendAction::ALL.iter().position(|action| *action == chosen)),
        )
    }

    pub(super) fn merge_method(
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let chosen = editing.settings.merge_method();
        let segments = MergeMethod::ALL
            .into_iter()
            .map(|method| {
                Segment::new(
                    method.config_text(),
                    method.label(),
                    cx.listener(move |this, _, _, cx| {
                        this.edit(cx, |editing| editing.settings.set_merge_method(method));
                    }),
                )
            })
            .collect();
        let note = div()
            .text_xs()
            .text_color(colors.muted_text)
            .child("Where a repo does not allow it, its first allowed method. [merge_methods] in config.toml sets one per repo.");
        setting_row(
            "Method",
            note,
            Segmented::new(
                "merge-method",
                SegmentLayout::Row {
                    segment_width: px(110.),
                },
                segments,
            )
            .picked(MergeMethod::ALL.iter().position(|method| *method == chosen)),
        )
    }
}
