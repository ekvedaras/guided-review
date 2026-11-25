//! The sidebar under the stations: the offers (changed on disk, Move to PR, a clone), the file tree
//! or route, and the settings row.

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Context, Div, FontWeight, InteractiveElement, IntoElement, ParentElement,
    PathPromptOptions, SharedString, Stateful, StatefulInteractiveElement, Styled, div, px,
};

use super::OneStepLayout;
use crate::app::components::button::{
    ButtonKind, ButtonSize, button, button_icon, button_keys, icon_label_button,
};
use crate::app::components::chip::{build_chip, chip};
use crate::app::components::icon::{Icon, sized_icon};
use crate::app::components::markdown_view::InlineMarkdown;
use crate::app::components::motion::{glide_to_share, turn_to};
use crate::app::components::palette::{Colors, SIDEBAR_WIDTH, Tone, TrailStage};
use crate::app::components::safety_net_view::SafetyNetIntro;
use crate::app::components::sidebar_row::{
    RowMark, RowPlace, check_flag, count_badge, finding_badge, row_label, row_note, row_places,
    sidebar_row, skim_chip,
};
use crate::app::components::title_bar::{TitleBarArea, TitleBarEdge, title_inset};
use crate::app::components::tooltip::tooltip;
use crate::app::components::version_link::version_link;
use crate::app::review_session::ReviewSession;
use crate::app::sidebar_rows::{SidebarDiskNews, SidebarMoveOffer};
use crate::app::updater::{Updater, Updates};
use crate::repin::RepinStatus;
use crate::viewer::{
    CloneOffer, CloneOfferState, Command, DiskOffer, Intent, JobProgress, ReadProgress, ReadUnit,
    StepId, TreeRowKind,
};

impl OneStepLayout {
    /// A local review's folder is not its snapshot any more: the line, with Update or Drop review when
    /// the reviewer can do something about it.
    fn disk_news_block(
        colors: &Colors,
        news: &SidebarDiskNews,
        repin_progress: &JobProgress,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let is_updating = matches!(repin_progress, JobProgress::Running { .. });
        let action = news.offer.map(|offer| match offer {
            DiskOffer::Update => icon_label_button(
                colors,
                "changed-on-disk-update",
                ButtonKind::Secondary,
                ButtonSize::Small,
                Icon::Refresh,
                match repin_progress {
                    JobProgress::Idle => "Update",
                    JobProgress::Running { .. } => "Updating…",
                    JobProgress::Failed(_) => "Try again",
                },
            )
            .when(!is_updating, |button| {
                button.on_click(cx.listener(|this, _, _, cx| {
                    this.session.update(cx, ReviewSession::repin);
                }))
            }),
            DiskOffer::DropReview => icon_label_button(
                colors,
                "branch-gone-drop-review",
                ButtonKind::Secondary,
                ButtonSize::Small,
                Icon::Trash,
                "Drop review",
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.session.update(cx, ReviewSession::drop_review);
            })),
        });
        let failure = match repin_progress {
            JobProgress::Failed(reason) => Some(
                div()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(SharedString::from(reason.to_string())),
            ),
            JobProgress::Idle | JobProgress::Running { .. } => None,
        };
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.warning)
                    .child(news.line.clone()),
            )
            .children(failure)
            .children(action)
    }

    /// A local review's branch has an open PR: the line, Move to <pr> and Keep local review.
    fn move_offer_block(
        colors: &Colors,
        offer: &SidebarMoveOffer,
        is_moving: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.accent)
                    .child(offer.heading.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        icon_label_button(
                            colors,
                            "move-to-pr",
                            ButtonKind::Secondary,
                            ButtonSize::Small,
                            Icon::ArrowRight,
                            if is_moving {
                                "Moving…".to_owned()
                            } else {
                                offer.button.to_string()
                            },
                        )
                        .when(!is_moving, |button| {
                            button.on_click(cx.listener(|this, _, _, cx| {
                                this.session.update(cx, ReviewSession::move_to_pr);
                            }))
                        }),
                    )
                    .when(!is_moving, |row| {
                        row.child(
                            icon_label_button(
                                colors,
                                "decline-move-to-pr",
                                ButtonKind::Secondary,
                                ButtonSize::Small,
                                Icon::Close,
                                offer.keep,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.session.update(cx, ReviewSession::decline_move);
                            })),
                        )
                    }),
            )
    }

    /// The offer to clone the repo whose code comes from GitHub, with its folder picker.
    fn clone_offer_block(
        colors: &Colors,
        offer: &CloneOffer,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let is_cloning = offer.state() == &CloneOfferState::Cloning;
        let failure = match offer.state() {
            CloneOfferState::Failed(reason) => Some(
                div()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(format!("Not cloned: {reason}")),
            ),
            CloneOfferState::Offered | CloneOfferState::Cloning => None,
        };
        let clone = icon_label_button(
            colors,
            "clone-repo",
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::FolderGit,
            match offer.state() {
                CloneOfferState::Offered => "Clone",
                CloneOfferState::Cloning => "Cloning…",
                CloneOfferState::Failed(_) => "Try again",
            },
        )
        .when(!is_cloning, |button| {
            button.on_click(cx.listener(|this, _, _, cx| {
                this.session.update(cx, ReviewSession::start_clone);
            }))
        });
        let other_folder = (!is_cloning).then(|| {
            icon_label_button(
                colors,
                "clone-repo-folder",
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::FolderOpen,
                "Other folder…",
            )
            .on_click(cx.listener(|this, _, _, cx| this.pick_clone_folder(cx)))
        });
        let not_now = (!is_cloning).then(|| {
            icon_label_button(
                colors,
                "clone-repo-not-now",
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::Close,
                "Not now",
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.session.update(cx, ReviewSession::decline_clone);
            }))
        });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.warning)
                    .child(offer.line()),
            )
            .children(failure)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(clone)
                    .children(other_folder)
                    .children(not_now),
            )
    }

    /// The picker is the system's, so the answer comes back later.
    fn pick_clone_folder(&mut self, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        let session = self.session.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = answer.await
                && let Some(folder) = paths.into_iter().next()
            {
                session.update(cx, |session, cx| session.pick_clone_folder(folder, cx));
            }
        })
        .detach();
    }

    /// A row touching the current one tells the layout while the mouse is on it.
    fn touching_hover(
        &self,
        row: Stateful<Div>,
        index: usize,
        place: RowPlace,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        if !matches!(
            place,
            RowPlace::JustAboveCurrent | RowPlace::JustBelowCurrent
        ) {
            return row;
        }
        row.on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
            let hovered = is_hovered.then_some((index, place));
            if this.hovered_touching_row != hovered {
                this.hovered_touching_row = hovered;
                cx.notify();
            }
        }))
    }

    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let session = self.session.read(cx);
        let sidebar = session.sidebar();
        let progress = sidebar.progress;
        let tree_rows = sidebar.tree.as_ref().map_or(&[][..], |tree| &tree.rows[..]);
        let is_first_current = sidebar
            .tree
            .as_ref()
            .map_or(sidebar.is_summary_current, |tree| tree.is_overview_current);
        let places = row_places(
            &std::iter::once(is_first_current)
                .chain(sidebar.steps.iter().map(|step| step.is_current))
                .chain(tree_rows.iter().map(|row| {
                    matches!(
                        row.kind,
                        TreeRowKind::File {
                            is_current: true,
                            ..
                        }
                    )
                }))
                .chain(
                    sidebar
                        .safety_net
                        .iter()
                        .map(|safety_net| safety_net.is_current),
                )
                .collect::<Vec<_>>(),
        );
        // By its index and place then: once the current row moves, a row kept from before no longer counts.
        let hovered_touching = self
            .hovered_touching_row
            .filter(|(index, place)| places.get(*index) == Some(place))
            .map(|(_, place)| place);
        let last_index = places.len().saturating_sub(1);
        let mut places = places.into_iter();
        let summary_place = places.next().unwrap_or(RowPlace::Apart);
        let step_places: Vec<RowPlace> = places.by_ref().take(sidebar.steps.len()).collect();
        let tree_places: Vec<RowPlace> = places.by_ref().take(tree_rows.len()).collect();
        let safety_net_place = places.next().unwrap_or(RowPlace::Apart);
        let is_basic = sidebar.tree.is_some();
        let summary_row = self
            .touching_hover(
                sidebar_row(
                    &colors,
                    "summary",
                    0,
                    summary_place,
                    RowMark::Page,
                    hovered_touching,
                ),
                0,
                summary_place,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| {
                    if is_basic {
                        review.go_to_overview();
                    } else {
                        review.go_to_summary();
                    }
                });
            }))
            .child(row_label(if is_basic { "Overview" } else { "Summary" }));
        let tree_row_elements =
            tree_rows
                .iter()
                .zip(tree_places.into_iter().enumerate())
                .map(|(row, (offset, place))| {
                    let index = offset + 1 + sidebar.steps.len();
                    match &row.kind {
                        TreeRowKind::Folder {
                            path,
                            label,
                            is_folded,
                            is_read,
                        } => {
                            let folder = path.clone();
                            self.touching_hover(
                                sidebar_row(
                                    &colors,
                                    SharedString::from(format!("folder-{path}")),
                                    row.depth,
                                    place,
                                    if *is_read {
                                        RowMark::Read
                                    } else {
                                        RowMark::Page
                                    },
                                    hovered_touching,
                                ),
                                index,
                                place,
                                cx,
                            )
                            .text_color(colors.muted_text)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.change_review(cx, |review| review.toggle_folder(&folder));
                            }))
                            .child(div().flex_none().h(px(22.)).flex().items_center().child(
                                turn_to(
                                    sized_icon(Icon::ChevronRight, colors.muted_text, px(12.)),
                                    SharedString::from(format!("folder-chevron-{path}")),
                                    !is_folded,
                                    1.,
                                ),
                            ))
                            .child(row_label(SharedString::from(label.clone())))
                        }
                        TreeRowKind::File {
                            path,
                            label,
                            is_read,
                            repin,
                            ..
                        } => {
                            let file = path.clone();
                            let repin_note = match repin {
                                Some(RepinStatus::New) => Some("new"),
                                Some(RepinStatus::Changed) => Some("changed"),
                                _ => None,
                            };
                            self.touching_hover(
                                sidebar_row(
                                    &colors,
                                    SharedString::from(format!("file-{path}")),
                                    row.depth,
                                    place,
                                    if *is_read {
                                        RowMark::Read
                                    } else {
                                        RowMark::Unread
                                    },
                                    hovered_touching,
                                ),
                                index,
                                place,
                                cx,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.change_review(cx, |review| review.go_to_file(&file));
                            }))
                            .child(row_label(SharedString::from(label.clone())))
                            .children(repin_note.map(|note| {
                                div().text_xs().text_color(colors.muted_text).child(note)
                            }))
                        }
                    }
                });
        let repin_progress = session.repin_progress();
        let disk_news = sidebar
            .disk_news
            .as_ref()
            .map(|news| Self::disk_news_block(&colors, news, &repin_progress, cx));
        let move_offer = sidebar
            .move_offer
            .as_ref()
            .map(|offer| Self::move_offer_block(&colors, offer, session.is_moving(), cx));
        let clone_offer = session
            .clone_offer()
            .map(|offer| Self::clone_offer_block(&colors, offer, cx));
        let is_everything_read = progress.total > 0 && progress.read == progress.total;
        let step_rows = sidebar
            .steps
            .iter()
            .zip(step_places.into_iter().enumerate())
            .map(|(step, (offset, place))| {
                let id: StepId = step.id.clone();
                let mark = if step.is_gone {
                    RowMark::Gone
                } else if step.is_ticked {
                    RowMark::Read
                } else {
                    RowMark::Unread
                };
                self.touching_hover(
                    sidebar_row(
                        &colors,
                        SharedString::from(format!("step-{id}")),
                        step.depth,
                        place,
                        mark,
                        hovered_touching,
                    ),
                    offset + 1,
                    place,
                    cx,
                )
                .when(step.is_skim && !step.is_current, |row| {
                    row.text_color(colors.muted_text)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| review.go_to(&id));
                }))
                .child(row_label(InlineMarkdown::new(
                    format!("sidebar-{}", step.id),
                    &step.label,
                )))
                .children(step.has_check.then(|| check_flag(&colors)))
                .children(step.is_skim.then(|| skim_chip(&colors)))
                .children(count_badge(&colors, step.thread_count))
                .children(finding_badge(&colors, step.finding_count))
            });
        let safety_net_row = sidebar.safety_net.as_ref().map(|safety_net| {
            self.touching_hover(
                sidebar_row(
                    &colors,
                    "safety-net",
                    0,
                    safety_net_place,
                    RowMark::Page,
                    hovered_touching,
                ),
                last_index,
                safety_net_place,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_review(cx, |review| review.go_to_safety_net());
            }))
            .child(div().flex_1().child(SafetyNetIntro::TITLE))
            .child(row_note(
                &colors,
                match safety_net.pieces {
                    1 => "1 piece".to_owned(),
                    pieces => format!("{pieces} pieces"),
                },
            ))
            .children(count_badge(&colors, safety_net.thread_count))
            .children(finding_badge(&colors, safety_net.finding_count))
        });
        let header = div()
            .flex_none()
            .px_4()
            .pt_4()
            .pb_3()
            .flex()
            .flex_col()
            .gap_3()
            .children(
                sidebar
                    .local_heading
                    .clone()
                    .map(|heading| div().text_xs().text_color(colors.muted_text).child(heading)),
            )
            .children(disk_news)
            .children(move_offer)
            .child(match sidebar.basic_mode_label {
                Some(label) => div()
                    .flex()
                    .child(chip(&colors, Tone::Accent).child(label))
                    .into_any_element(),
                None => progress_meter(&colors, progress, sidebar.progress_unit).into_any_element(),
            })
            .child(self.stations_section(&colors, sidebar, is_everything_read, cx))
            .children(clone_offer);
        div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .text_sm()
            .child(
                TitleBarArea::new("sidebar-title-bar", gpui::transparent_black())
                    .edge(TitleBarEdge::Open)
                    .child(
                        div()
                            .pl(title_inset())
                            .flex()
                            .items_center()
                            .gap_2()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.muted_text)
                            .child("Guided review")
                            .children(build_chip(&colors)),
                    ),
            )
            .child(header)
            .child(
                div()
                    .id("sidebar-steps")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_px()
                    .child(summary_row)
                    .children(step_rows)
                    .children(tree_row_elements)
                    .children(safety_net_row),
            )
            .children(update_button(&colors, cx))
            .child(self.settings_row(&colors, cx))
    }

    /// Settings and the shortcuts sheet at the foot of the sidebar, under a quiet line with the version
    /// this app runs, so the Settings row stays at the very bottom.
    fn settings_row(&self, colors: &Colors, cx: &mut Context<Self>) -> impl IntoElement {
        let shortcuts = button(
            colors,
            "sidebar-shortcuts",
            ButtonKind::Ghost,
            ButtonSize::Regular,
        )
        .px_2()
        .gap_1p5()
        .tooltip(tooltip(|_| "Keyboard shortcuts".into()))
        .child(button_icon(
            colors,
            ButtonKind::Ghost,
            ButtonSize::Regular,
            Icon::Keyboard,
        ))
        .child(button_keys(
            colors,
            ButtonKind::Ghost,
            Command::Shortcuts,
            cx,
        ))
        .on_click(cx.listener(|this, _, window, cx| {
            this.act(Intent::ToggleShortcuts, window, cx);
        }));
        let row = div()
            .px_2()
            .pb_2()
            .flex()
            .items_center()
            .gap_1()
            .child(
                button(
                    colors,
                    "sidebar-settings",
                    ButtonKind::Ghost,
                    ButtonSize::Regular,
                )
                .px_2()
                .child(button_icon(
                    colors,
                    ButtonKind::Ghost,
                    ButtonSize::Regular,
                    Icon::Settings,
                ))
                .child("Settings")
                .child(button_keys(
                    colors,
                    ButtonKind::Ghost,
                    Command::Settings,
                    cx,
                ))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.act(Intent::ToggleSettings, window, cx);
                })),
            )
            .child(div().flex_1())
            .child(shortcuts);
        let version = div().px_4().pt_2().pb_1().flex().child(
            version_link(colors, "sidebar-version", colors.faint_text())
                .text_xs()
                .line_height(px(12.)),
        );
        div().flex_none().child(version).child(row)
    }
}

/// Above Settings, only while a newer release is offered, installing, or installed and waiting for a
/// restart.
fn update_button(colors: &Colors, cx: &App) -> Option<impl IntoElement + use<>> {
    let Updates(updater) = cx.try_global::<Updates>()?.clone();
    let (action, is_installing) = {
        let read = updater.read(cx);
        (read.action()?.clone(), read.is_installing())
    };
    let kind = if is_installing {
        ButtonKind::Secondary
    } else {
        ButtonKind::Tinted
    };
    Some(
        div().flex_none().px_4().pt_2().child(
            icon_label_button(
                colors,
                "update-app",
                kind,
                ButtonSize::Regular,
                Icon::ArrowUp,
                action,
            )
            .w_full()
            .when(!is_installing, |button| {
                button.on_click(move |_, _, cx| updater.update(cx, Updater::act))
            }),
        ),
    )
}

fn progress_meter(colors: &Colors, progress: ReadProgress, unit: ReadUnit) -> impl IntoElement {
    let share = progress.share();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .justify_between()
                .child(div().font_weight(FontWeight::MEDIUM).child(format!(
                    "{} of {} {} read",
                    progress.read,
                    progress.total,
                    unit.plural()
                )))
                .child(
                    div()
                        .text_color(colors.muted_text)
                        .child(format!("{}%", (share * 100.).round())),
                ),
        )
        .child(
            div()
                .h(px(6.))
                .w_full()
                .rounded_full()
                .bg(colors.subtle_border)
                .child(glide_to_share(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(colors.trail_fill(TrailStage::of(progress))),
                    "progress-fill",
                    share,
                )),
        )
}
