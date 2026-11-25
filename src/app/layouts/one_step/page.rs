//! The rows of the page: steps, files, the summary, the overview, the toolbar and the screen's footer.

use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, Div, ElementId, Entity, FontWeight, InteractiveElement, IntoElement,
    ListOffset, ParentElement, Pixels, SharedString, Stateful, StatefulInteractiveElement, Styled,
    div, px,
};

use super::{OneStepLayout, ai_review_icon};
use crate::app::code_selection::TextSpot;
use crate::app::components::button::{
    ButtonKind, ButtonSize, button, button_icon, button_keys, icon_label_button, joined,
    toggle_button,
};
use crate::app::components::card::card;
use crate::app::components::chip::{callout, chip};
use crate::app::components::diagram::DiagramView;
use crate::app::components::diff_view::{
    CodeSelectionHooks, DiffRow, GapAsk, OnGapAsk, OnScrollX, PieceHeader, PieceMarker,
    binary_file_row, box_row, hung_row, hunk_gap, no_change_since_read_row, only_whitespace_row,
    piece_end, rendered_block_row, rendered_file_waiting_row,
};
use crate::app::components::finding_view::{FindingBoxKeys, finding_card};
use crate::app::components::fold_toggle::FoldToggle;
use crate::app::components::hidden_text_notice::hidden_text_notice;
use crate::app::components::icon::{Icon, sized_icon};
use crate::app::components::markdown_view::{
    InlineMarkdown, MarkdownView, PreparedMarkdown, UnderPicture, bullet, parsed_markdown,
};
use crate::app::components::motion::{grow_when_shown, pop_in, turn_to};
use crate::app::components::palette::{CODE_FONT, CONTROL_RADIUS, Colors, INNER_RADIUS, Tone};
use crate::app::components::safety_net_view::SafetyNetIntro;
use crate::app::components::sidebar_row::check_flag;
use crate::app::components::step_notes::{StepNotesCard, note_label};
use crate::app::components::tooltip::tooltip;
use crate::app::page_rows::DESCRIPTION_KEY;
use crate::app::review_session::ReviewSession;
use crate::local_facts::{LocalChange, LocalFacts};
use crate::viewer::{
    AskAt, BoxEdges, BoxFold, Command, CommentGesture, FinishAction, Intent, NextScreen,
    NoteSubject, NotesButton, OverviewPage, OverviewStats, PageRow, ReadSubject, ReadUnit,
    ReviewChange, ReviewerSummaryRow, RouteStopRow, Screen, ScreenFooter, SelectedRows,
    StepHeading, StepId, StepIntro, SummaryColumns, SummaryStatsRow, ThreadRowPlace, Toolbar,
    TourLead, pluralised,
};

impl OneStepLayout {
    pub(super) fn render_row(
        &mut self,
        index: usize,
        selected: Option<&SelectedRows>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.session.read(cx).rows().row(index).cloned() else {
            return div().into_any_element();
        };
        let edges = self.session.read(cx).rows().edges(index);
        self.render_page_row(row, index, edges, selected, cx)
    }

    /// `index` tells the row's elements apart from other rows'; a row inside another uses a made-up one.
    fn render_page_row(
        &mut self,
        row: PageRow,
        index: usize,
        edges: BoxEdges,
        selected: Option<&SelectedRows>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = Colors::active(cx);
        match row {
            PageRow::SummaryColumns(columns) => self.summary_columns(&columns, index, cx),
            PageRow::SummaryTitle(title) => div()
                .pb_5()
                .flex()
                .flex_col()
                .items_start()
                .gap_2()
                .child(tour_place(&colors, "SUMMARY", TourStop::On))
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(InlineMarkdown::new("summary-title", &title)),
                )
                .into_any_element(),
            PageRow::SummaryStats(stats) => summary_stats(&colors, &stats).into_any_element(),
            PageRow::RouteHeading => note_label("THE TOUR")
                .pt_6()
                .pb_2()
                .text_color(colors.muted_text)
                .into_any_element(),
            PageRow::RouteStop(stop) => route_stop(&stop, cx).into_any_element(),
            PageRow::TourLead(lead) => div()
                .pt_5()
                .pb_2()
                .max_w(READING_WIDTH)
                .flex()
                .children(match lead.as_ref() {
                    TourLead::Finish { steps, finish } => Some(
                        finish_card(
                            *steps,
                            ReadUnit::Steps,
                            *finish,
                            ReviewAgain::Offered,
                            "summary-lead",
                            cx,
                        )
                        .into_any_element(),
                    ),
                    TourLead::FinishFiles { files, finish } => Some(
                        finish_card(
                            *files,
                            ReadUnit::Files,
                            *finish,
                            ReviewAgain::Offered,
                            "summary-lead",
                            cx,
                        )
                        .into_any_element(),
                    ),
                    other => lead_button(other, LeadLabel::Named, "summary-lead", cx),
                })
                .into_any_element(),
            PageRow::SummaryBullet(summary_bullet) => div()
                .pb_2()
                .flex()
                .items_start()
                .gap_3()
                .max_w(READING_WIDTH)
                .child(bullet(&colors, SUMMARY_LINE))
                .child(div().flex_1().child(InlineMarkdown::new(
                    format!("summary-bullet-{index}"),
                    &summary_bullet,
                )))
                .into_any_element(),
            PageRow::StepHeading(heading) => step_heading(&heading, cx).into_any_element(),
            PageRow::StepIntro(intro) => self.step_intro(&intro, index, cx).into_any_element(),
            PageRow::RepinNotes {
                gone_pieces,
                text_may_be_old,
            } => repin_notes(&colors, gone_pieces, text_may_be_old).into_any_element(),
            PageRow::Toolbar(toolbar) => self.toolbar(&toolbar, cx).into_any_element(),
            PageRow::ScreenFooter(footer) => screen_footer(&footer, cx).into_any_element(),
            PageRow::Overview(overview) => self.overview(&overview, cx).into_any_element(),
            PageRow::SafetyNetIntro => SafetyNetIntro.into_any_element(),
            PageRow::PieceHeader(heading) => {
                let open_code = heading.code_link.as_ref().map(|link| {
                    Self::open_code_button(format!("open-piece-{}", heading.piece), link, cx)
                });
                let ask = heading
                    .ask
                    .as_ref()
                    .map(|ask| self.ask_at(ask, ButtonKind::Ghost, cx));
                let view_switch = heading
                    .view
                    .as_ref()
                    .map(|switch| self.view_switch(switch, cx));
                let labels_toggle = heading
                    .labels
                    .as_ref()
                    .map(|switch| self.labels_toggle(switch, cx));
                let header = PieceHeader::new(
                    heading.label.clone(),
                    heading.repin_label,
                    labels_toggle
                        .into_iter()
                        .chain(view_switch)
                        .chain(open_code)
                        .chain(ask)
                        .collect(),
                )
                .closing_its_box(edges.closes);
                match heading.fold.clone() {
                    Some(fold) => header
                        .with_fold_toggle(Self::box_fold_toggle(fold, "fold-box", None, cx))
                        .into_any_element(),
                    None => header.into_any_element(),
                }
            }
            PageRow::PieceMarker(heading) => {
                let open_code = heading.code_link.as_ref().map(|link| {
                    Self::open_code_button(format!("open-marker-{}", heading.piece), link, cx)
                });
                let ask = heading
                    .ask
                    .as_ref()
                    .map(|ask| self.ask_at(ask, ButtonKind::Ghost, cx));
                PieceMarker::new(
                    heading.label.clone(),
                    heading.repin_label,
                    open_code.into_iter().chain(ask).collect(),
                )
                .with_edges(edges)
                .into_any_element()
            }
            PageRow::NoChangeSinceRead => {
                no_change_since_read_row(&colors, edges).into_any_element()
            }
            PageRow::BinaryFile(binary) => binary_file_row(
                &colors,
                edges,
                format!("binary-{}", binary.piece).into(),
                binary.link.clone(),
                cx,
            )
            .into_any_element(),
            PageRow::Image { path, .. } => box_row(&colors, edges)
                .child(self.image_row(&path, cx))
                .into_any_element(),
            PageRow::OnlyWhitespace => only_whitespace_row(&colors, edges).into_any_element(),
            PageRow::RenderedBlock(block) => rendered_block_row(
                &colors,
                edges,
                block.change,
                MarkdownView::new(block.key.to_string(), parsed_markdown(&block.source)),
            )
            .into_any_element(),
            PageRow::RenderedFileWaiting => {
                rendered_file_waiting_row(&colors, edges).into_any_element()
            }
            PageRow::HunkGap(gap) => hunk_gap(
                &colors,
                edges,
                SharedString::from(&gap.id),
                &gap.gap,
                SharedString::from(&gap.label),
                &self.on_gap_ask(cx),
            )
            .into_any_element(),
            PageRow::Line(line) => {
                let is_selected = selected.is_some_and(|rows| rows.contains(index, &line.location));
                let code_selection = self
                    .code_selection
                    .as_ref()
                    .filter(|selection| selection.is_on(self.session.read(cx).review().screen()))
                    .and_then(|selection| selection.range_on(index, line.code.len()));
                let on_code_gesture = Rc::clone(&self.on_code_gesture);
                let path = line.location.path.clone();
                let scroll_x = self
                    .code_scroll_x
                    .of(self.session.read(cx).review().screen(), &path);
                let layout_handle = cx.weak_entity();
                let on_scroll_x: OnScrollX = Rc::new(move |delta, window, cx| {
                    layout_handle
                        .update(cx, |this, cx| {
                            this.scroll_code_sideways(&path, delta, window, cx);
                        })
                        .ok();
                });
                DiffRow::new(line)
                    .with_edges(edges)
                    .scrolled_sideways(scroll_x, on_scroll_x)
                    .selected(is_selected)
                    .with_code_selection(CodeSelectionHooks {
                        selected: code_selection,
                        on_gesture: Rc::new(move |gesture, byte, window, cx| {
                            on_code_gesture(gesture, TextSpot { row: index, byte }, window, cx);
                        }),
                    })
                    .with_gestures(Rc::clone(&self.on_row_gesture))
                    .with_name_pointer(Rc::clone(&self.on_name_pointer))
                    .into_any_element()
            }
            PageRow::Thread { view, place, .. } => {
                let thread = self.thread_element(&view, cx);
                match place {
                    ThreadRowPlace::UnderLine => hung_row().child(thread).into_any_element(),
                    ThreadRowPlace::AfterCode => div().pb_3().child(thread).into_any_element(),
                }
            }
            PageRow::Finding(finding) => {
                let actions = self.finding_actions(&colors, &finding, cx);
                let question = self.finding_questions.view(
                    &colors,
                    &finding.id,
                    cx.entity(),
                    FindingBoxKeys {
                        on_type: Self::type_why_question,
                        on_send: Self::send_finding_box,
                        on_send_now: Self::send_finding_box_now,
                        on_cancel: Self::give_up_why_question,
                    },
                );
                let card = finding_card(&colors, &finding, question, actions);
                match finding.place {
                    ThreadRowPlace::UnderLine => hung_row().child(card).into_any_element(),
                    ThreadRowPlace::AfterCode => div().pb_3().child(card).into_any_element(),
                }
            }
            PageRow::QueuedComment(queued) => hung_row()
                .child(self.queued_comment(&queued, cx))
                .into_any_element(),
            PageRow::Composer { place, .. } => hung_row()
                .children(self.composer(&place, cx))
                .into_any_element(),
            PageRow::AskMarker(ask) => hung_row()
                .flex()
                .px_2()
                .py_1()
                .child(self.ask_at(&ask, ButtonKind::Ghost, cx))
                .into_any_element(),
            PageRow::PieceEnd => piece_end(),
        }
    }

    /// The summary keeps its reading width; the reviewer summary and the description sit beside it where
    /// the window has room, else under it.
    fn summary_columns(
        &mut self,
        columns: &SummaryColumns,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const INNER_ROWS: usize = 1_000_000;
        let summary: Vec<AnyElement> = columns
            .summary
            .iter()
            .enumerate()
            .map(|(inner, row)| {
                self.render_page_row(
                    row.clone(),
                    INNER_ROWS * (index + 1) + inner,
                    BoxEdges::default(),
                    None,
                    cx,
                )
            })
            .collect();
        let rows = self.session.read(cx).rows();
        let (prepared_summary, prepared_description) = (
            rows.prepared_reviewer_summary(),
            rows.prepared_description(),
        );
        let reviewer_summary = columns
            .reviewer_summary
            .as_ref()
            .zip(prepared_summary)
            .map(|(summary, prepared)| self.reviewer_summary(summary, prepared, cx));
        let description =
            prepared_description.map(|description| self.description_card(description, cx));
        let beside = (reviewer_summary.is_some() || description.is_some()).then(|| {
            div()
                .flex_1()
                .min_w(DESCRIPTION_MIN_WIDTH)
                .max_w(READING_WIDTH)
                .flex()
                .flex_col()
                .gap_6()
                .children(reviewer_summary)
                .children(description)
        });
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap_x_10()
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .max_w(READING_WIDTH)
                    .flex()
                    .flex_col()
                    .children(summary),
            )
            .children(beside)
            .into_any_element()
    }

    fn reviewer_summary(
        &self,
        summary: &ReviewerSummaryRow,
        prepared: Rc<PreparedMarkdown>,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = Colors::active(cx);
        let (session, buttons) = (self.session.clone(), Arc::clone(&summary.picture_buttons));
        let under_picture: UnderPicture = Rc::new(move |path, cx| {
            let state = buttons.get(path)?;
            Some(
                notes_button(
                    &Colors::active(cx),
                    SharedString::from(format!("picture-to-notes-{}", path.display())),
                    state,
                    NotesLabels::PICTURE,
                    session.clone(),
                    NoteSubject::SummaryPicture(path.to_owned()),
                )
                .into_any_element(),
            )
        });
        let markdown =
            MarkdownView::new("reviewer-summary", prepared).under_each_picture(under_picture);
        self.markdown_card("REVIEWER SUMMARY", markdown, cx)
            .children(summary.notes_button.as_ref().map(|state| {
                notes_button(
                    &colors,
                    "summary-to-notes",
                    state,
                    NotesLabels::SUMMARY,
                    self.session.clone(),
                    NoteSubject::ReviewerSummary,
                )
            }))
    }

    fn overview(&self, overview: &OverviewPage, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let description = self
            .session
            .read(cx)
            .rows()
            .prepared_description()
            .map(|description| self.description_card(description, cx));
        let local = overview
            .local
            .as_ref()
            .map(|local| local_overview_card(&colors, local));
        let files = overview.files.iter().map(|file| {
            let path = file.path.clone();
            let (folder, name) = match file.path.as_str().rsplit_once('/') {
                Some((folder, name)) => (Some(format!("{folder}/")), name),
                None => (None, file.path.as_str()),
            };
            div()
                .id(SharedString::from(format!("overview-file-{}", file.path)))
                .px_3()
                .py_1p5()
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| review.go_to_file(&path));
                }))
                .child(div().flex_none().w(px(14.)).flex().justify_center().child(
                    if file.is_read {
                        sized_icon(Icon::Check, colors.success, px(14.)).into_any_element()
                    } else {
                        div()
                            .size(px(8.))
                            .rounded_full()
                            .border_1()
                            .border_color(colors.muted_text)
                            .into_any_element()
                    },
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(*CODE_FONT)
                        .text_sm()
                        .children(folder.map(|folder| {
                            div()
                                .flex_none()
                                .text_color(colors.muted_text)
                                .child(folder)
                        }))
                        .child(div().min_w_0().truncate().child(name.to_owned())),
                )
                .children(file.change.map(|change| change_chip(&colors, change)))
                .child(line_counts(&colors, file.added_lines, file.removed_lines))
        });
        let write_guided_tour = Some(overview.write_guided_tour_refusal).map(|refusal| {
            let button = icon_label_button(
                &colors,
                "write-guided-tour",
                ButtonKind::Secondary,
                ButtonSize::Regular,
                Icon::Sparkles,
                "Write guided tour",
            );
            div()
                .pt_6()
                .flex()
                .flex_col()
                .items_start()
                .gap_2()
                .child(if refusal.is_none() {
                    button
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.act(Intent::WriteGuidedTour, window, cx);
                        }))
                        .into_any_element()
                } else {
                    button.opacity(0.5).cursor_not_allowed().into_any_element()
                })
                .children(refusal.map(|refusal| {
                    div()
                        .text_xs()
                        .text_color(colors.muted_text)
                        .child(refusal.reason())
                }))
        });
        div()
            .pb_8()
            .w_full()
            .max_w(READING_WIDTH)
            .flex()
            .flex_col()
            .child(
                div()
                    .pb_5()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .child(tour_place(&colors, "OVERVIEW", TourStop::On))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(InlineMarkdown::new("overview-title", &overview.title)),
                    ),
            )
            .child(overview_stats(&colors, overview.stats))
            .children(description)
            .children(local)
            .child(
                div()
                    .pb_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(note_label("CHANGED FILES").text_color(colors.muted_text))
                    .child(card(&colors).py_1().flex().flex_col().children(files)),
            )
            .children(write_guided_tour)
    }

    /// The PR description's card, with a warning under it when the description hides text.
    fn description_card(&self, description: Rc<PreparedMarkdown>, cx: &mut Context<Self>) -> Div {
        let colors = Colors::active(cx);
        let hidden_notice = self
            .session
            .read(cx)
            .rows()
            .description_hidden_text()
            .and_then(|hidden| hidden_text_notice(&colors, "description-hidden-text", hidden));
        self.markdown_card(
            "PR DESCRIPTION",
            MarkdownView::new(DESCRIPTION_KEY, description)
                .with_diagrams(self.description_diagrams.clone()),
            cx,
        )
        .children(hidden_notice)
    }

    fn markdown_card(
        &self,
        label: &'static str,
        markdown: MarkdownView,
        cx: &mut Context<Self>,
    ) -> Div {
        let colors = Colors::active(cx);
        div()
            .pb_4()
            .w_full()
            .max_w(READING_WIDTH)
            .flex()
            .flex_col()
            .gap_2()
            .child(note_label(label).text_color(colors.muted_text))
            .child(card(&colors).px_5().py_4().text_sm().child(markdown))
    }

    fn toolbar(&self, toolbar: &Toolbar, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let fold = toolbar.fold.as_ref().map(|fold| {
            let step = fold.step.clone();
            FoldToggle::new(
                fold.is_folded,
                fold.piece_count,
                cx.listener(move |this, _, _, cx| {
                    this.change_review(cx, |review| review.toggle_fold(&step));
                }),
            )
            .keys(button_keys(
                &colors,
                if fold.is_folded {
                    ButtonKind::Secondary
                } else {
                    ButtonKind::Ghost
                },
                Command::ToggleFold,
                cx,
            ))
        });
        let full_diff = toolbar.full_diff.as_ref().map(|control| {
            let subject = control.subject.clone();
            toggle_button(
                &colors,
                "full-diff-toggle",
                control.is_showing_full_diff,
                ButtonSize::Small,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| review.toggle_full_diff_of(&subject));
            }))
            .child("Full diff, not only the change since your read")
        });
        let whitespace = toolbar.ignores_whitespace.map(|is_on| {
            toolbar_toggle(
                &colors,
                "ignore-whitespace",
                is_on,
                Icon::Space,
                "Hide whitespace",
                Command::HideWhitespace,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_review(cx, |review| {
                    review.display_options_mut().toggle_ignore_whitespace();
                });
            }))
        });
        let wrap = toolbar.wraps_lines.map(|is_on| {
            toolbar_toggle(
                &colors,
                "wrap-lines",
                is_on,
                Icon::WrapText,
                "Wrap lines",
                Command::WrapLines,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_review(cx, |review| {
                    review.display_options_mut().toggle_wrap_lines();
                });
            }))
        });
        let toggles: Vec<Stateful<Div>> = whitespace.into_iter().chain(wrap).collect();
        let toggles = (!toggles.is_empty()).then(|| joined(&colors, toggles));
        let ask = toolbar.ask.as_ref().map(|ask| {
            self.ask_at(ask, ButtonKind::Tinted, cx).child(button_keys(
                &colors,
                ButtonKind::Tinted,
                Command::AskAboutStep,
                cx,
            ))
        });
        let run_ai_review = toolbar.run_ai_review.map(|action| {
            icon_label_button(
                &colors,
                "run-ai-review",
                ButtonKind::Secondary,
                ButtonSize::Small,
                ai_review_icon(action),
                action.label(),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.act(Intent::RunAiReview, window, cx);
            }))
        });
        let comment_on_file = toolbar.comment_on_file.as_ref().map(|path| {
            let comment_path = path.clone();
            icon_label_button(
                &colors,
                ElementId::from(SharedString::from(format!("comment-on-file-{path}"))),
                ButtonKind::Secondary,
                ButtonSize::Small,
                Icon::MessageSquare,
                "Comment on this file",
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_comment_box(CommentGesture::OnFile(&comment_path), window, cx);
            }))
        });
        div()
            .w_full()
            .pb_3()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .children(toolbar.change.map(|change| change_chip(&colors, change)))
            .children(fold)
            .children(full_diff)
            .child(div().flex_1())
            .children(run_ai_review)
            .children(comment_on_file)
            .children(toggles)
            .children(ask.map(|ask| div().pl_1().child(ask)))
    }

    /// The text keeps its reading width; the diagram sits beside it where the window has room, else under it.
    fn step_intro(
        &self,
        intro: &StepIntro,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = Colors::active(cx);
        let text = div()
            .flex_none()
            .w_full()
            .max_w(READING_WIDTH)
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_color(colors.muted_text).child(MarkdownView::new(
                format!("step-why-{index}"),
                parsed_markdown(&intro.why),
            )))
            .children(intro.notes.clone().map(StepNotesCard));
        let diagram = intro.diagram.as_ref().map(|diagram| {
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .items_start()
                .gap_2()
                .child(DiagramView::new(self.diagrams.clone()))
                .children(diagram.notes_button.as_ref().map(|state| {
                    notes_button(
                        &Colors::active(cx),
                        "diagram-to-notes",
                        state,
                        NotesLabels::DIAGRAM,
                        self.session.clone(),
                        NoteSubject::StepDiagram(diagram.step.clone()),
                    )
                }))
        });
        div()
            .pb_4()
            .flex()
            .flex_wrap()
            .items_start()
            .gap_x_8()
            .gap_y_4()
            .child(text)
            .children(diagram)
    }

    fn ask_at(&self, ask: &AskAt, kind: ButtonKind, cx: &mut Context<Self>) -> Stateful<Div> {
        self.ask_button(
            ask.id.to_string(),
            ask.place.clone(),
            ask.label.clone(),
            kind,
            cx,
        )
    }

    fn on_gap_ask(&self, cx: &mut Context<Self>) -> OnGapAsk {
        let layout = cx.weak_entity();
        Rc::new(move |ask, _, cx| {
            layout
                .update(cx, |this, cx| {
                    this.change_review(cx, |review| match ask {
                        GapAsk::More(more) => review.display_options_mut().show_more_context(more),
                        GapAsk::All(gap) => review.display_options_mut().show_hidden_lines(gap),
                    });
                })
                .ok();
        })
    }

    fn toggle_tick(&mut self, id: &StepId, cx: &mut Context<Self>) {
        let change = ReviewChange::ToggleTick(ReadSubject::Step(id.clone()));
        self.session
            .update(cx, |session, cx| session.apply(change, cx));
    }

    /// The chevron that opens or folds a file box. Folded from the pinned header, the box's own
    /// header is scrolled to, so the reader stays at the box they folded.
    pub(super) fn box_fold_toggle(
        fold: BoxFold,
        id: &str,
        header_row: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let colors = Colors::active(cx);
        div()
            .id(SharedString::from(format!("{id}-{}", fold.path)))
            .flex_none()
            .w(px(14.))
            .h(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(INNER_RADIUS))
            .cursor_pointer()
            .hover(|chevron| chevron.bg(colors.hover))
            .active(|chevron| chevron.bg(colors.pressed))
            .child(turn_to(
                sized_icon(Icon::ChevronRight, colors.muted_text, px(12.)),
                SharedString::from(format!("{id}-chevron")),
                !fold.is_folded,
                1.,
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| {
                    review.toggle_box_fold(&fold.screen, &fold.path);
                });
                if let Some(row) = header_row {
                    // From the row above, so the step bar does not cover the header.
                    this.screen_list.scroll_to(ListOffset {
                        item_ix: row.saturating_sub(1),
                        offset_in_item: Pixels::ZERO,
                    });
                }
            }))
    }
}

/// Keeps prose at a readable line length on a wide window; code still takes the full width.
const READING_WIDTH: Pixels = px(760.);
/// Narrower than this beside the summary, the description goes under it instead.
const DESCRIPTION_MIN_WIDTH: Pixels = px(420.);

/// The line height of the summary's bullets, which their dots centre on.
const SUMMARY_LINE: Pixels = px(26.);
fn step_heading(heading: &StepHeading, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
    let title = div()
        .id("step-title")
        .text_2xl()
        .font_weight(FontWeight::SEMIBOLD)
        .child(InlineMarkdown::new("step-title", &heading.title));
    let title = if heading.folds_on_title_click {
        let id = heading.id.clone();
        title
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_review(cx, |review| review.toggle_fold(&id));
            }))
    } else {
        title
    };
    div()
        .w_full()
        .pb_2()
        .flex()
        .items_end()
        .gap_4()
        .when(heading.is_gone, |row| row.text_color(colors.muted_text))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .items_start()
                .gap_2()
                .child(step_place(&colors, heading))
                .child(title),
        )
        .child(tick_button(
            heading.is_ticked,
            heading.id.clone(),
            ButtonSize::Regular,
            cx,
        ))
}

/// The step's place on the tour, over its heading; a gone step's has no trail dot.
pub(super) fn step_place(colors: &Colors, heading: &StepHeading) -> impl IntoElement {
    let text = match (heading.number, heading.is_gone) {
        (_, true) => "Gone since the last version".to_owned(),
        (Some((number, count)), false) => format!("Step {number} of {count}"),
        (None, false) => "Step".to_owned(),
    };
    let stop = if heading.is_gone {
        TourStop::Off
    } else {
        TourStop::On
    };
    tour_place(colors, text.to_uppercase(), stop)
}

/// Whether a screen is a stop on the trail, or a step no longer on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TourStop {
    On,
    Off,
}

/// A screen's place on the tour, over its heading: a label like every other label on the page,
/// led by the trail's dot when the screen is on the trail.
fn tour_place(colors: &Colors, text: impl Into<SharedString>, stop: TourStop) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .text_color(colors.muted_text)
        .when(stop == TourStop::On, |place| {
            place.child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(colors.trail.left_right()),
            )
        })
        .child(note_label(text))
}

fn repin_notes(colors: &Colors, gone_pieces: usize, text_may_be_old: bool) -> impl IntoElement {
    let note = |text: String| chip(colors, Tone::Warning).py_0p5().child(text);
    div()
        .pb_3()
        .flex()
        .gap_2()
        .text_xs()
        .when(gone_pieces > 0, |row| {
            row.child(note(match gone_pieces {
                1 => "1 piece gone since last version".to_owned(),
                count => format!("{count} pieces gone since last version"),
            }))
        })
        .when(text_may_be_old, |row| {
            row.child(note("The text may describe an older version".to_owned()))
        })
}

/// What the last screen's footer says: the end, or what was passed without being read.
fn the_end_note(footer: &ScreenFooter) -> SharedString {
    let unit = if footer.file_place.is_some() {
        ReadUnit::Files
    } else {
        ReadUnit::Steps
    };
    match (footer.ticks.is_some(), footer.unread_left, unit) {
        (true, _, _) => "This is the last step.".into(),
        (false, 0, ReadUnit::Files) => "That was the last file.".into(),
        (false, 0, ReadUnit::Steps) => {
            "That was the last step. Finish the review in the sidebar.".into()
        }
        (false, 1, ReadUnit::Files) => "1 file is not read yet.".into(),
        (false, 1, ReadUnit::Steps) => "1 step is not read yet.".into(),
        (false, left, unit) => format!("{left} {} are not read yet.", unit.plural()).into(),
    }
}

fn screen_footer(footer: &ScreenFooter, cx: &mut Context<OneStepLayout>) -> AnyElement {
    let colors = Colors::active(cx);
    let previous = footer.has_previous.then(|| {
        button(
            &colors,
            "footer-previous",
            ButtonKind::Ghost,
            ButtonSize::Regular,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_review(cx, |review| review.go_back());
        }))
        .child("Previous")
        .child(button_keys(
            &colors,
            ButtonKind::Ghost,
            Command::PreviousScreen,
            cx,
        ))
    });
    let next_label = footer.next.as_ref().map(|next| match next {
        NextScreen::Step { title } => title.clone(),
        NextScreen::File { path } => path.to_string(),
        NextScreen::SafetyNet => SafetyNetIntro::TITLE.to_owned(),
    });
    if let Some((count, unit, finish)) = footer.lead.as_ref().and_then(|lead| match lead {
        TourLead::Finish { steps, finish } => Some((*steps, ReadUnit::Steps, *finish)),
        TourLead::FinishFiles { files, finish } => Some((*files, ReadUnit::Files, *finish)),
        _ => None,
    }) {
        return div()
            .pt_8()
            .pb_10()
            .flex()
            .items_center()
            .gap_3()
            .children(previous)
            .child(finish_card(
                count,
                unit,
                finish,
                ReviewAgain::NotOffered,
                "footer",
                cx,
            ))
            .into_any_element();
    }
    let action = match (&footer.ticks, footer.next.is_some()) {
        (Some(_), true) => Some(("Mark as read and go on", Onward::Goes)),
        (Some(_), false) => Some(("Mark as read", Onward::Stays)),
        (None, true) => Some(("Next", Onward::Goes)),
        (None, false) => None,
    };
    let lead_label = if footer.next.is_none() {
        LeadLabel::Named
    } else {
        LeadLabel::Short
    };
    let lead = footer
        .lead
        .as_ref()
        .and_then(|lead| lead_button(lead, lead_label, "footer-lead", cx));
    let primary = action.map(|(label, onward)| {
        button(
            &colors,
            "footer-next",
            ButtonKind::Primary,
            ButtonSize::Large,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.act(Intent::MarkReadAndGoOn, window, cx);
        }))
        .child(label)
        .child(button_keys(
            &colors,
            ButtonKind::Primary,
            match onward {
                Onward::Goes => Command::NextScreen,
                Onward::Stays => Command::ToggleRead,
            },
            cx,
        ))
    });
    let up_next = next_label.map(|title| {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                note_label("Up next:")
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.muted_text),
            )
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text)
                    .truncate()
                    .child(InlineMarkdown::new("footer-up-next", &title)),
            )
    });
    let file_place = footer.file_place.map(|(number, count)| {
        div()
            .flex_none()
            .text_sm()
            .text_color(colors.muted_text)
            .child(format!("File {number} of {count}"))
    });
    let the_end = (footer.next.is_none()).then(|| {
        div()
            .flex_1()
            .min_w_0()
            .text_sm()
            .text_color(colors.muted_text)
            .child(the_end_note(footer))
    });
    div()
        .pt_8()
        .pb_10()
        .flex()
        .items_center()
        .gap_3()
        .children(previous)
        .child(
            card(&colors)
                .flex_1()
                .min_w_0()
                .pl_5()
                .pr_3()
                .py_3()
                .flex()
                .items_center()
                .gap_4()
                .children(file_place)
                .children(up_next)
                .children(the_end)
                .children(lead)
                .children(primary),
        )
        .into_any_element()
}

/// How big the PR is, in the numbers the review truly has.
fn summary_stats(colors: &Colors, stats: &SummaryStatsRow) -> impl IntoElement {
    let item = |icon_name: Icon, text: SharedString| {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(sized_icon(icon_name, colors.muted_text, px(14.)))
            .child(text)
    };
    let dot = || div().size(px(3.)).rounded_full().bg(colors.border);
    div()
        .pb_5()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_3()
        .text_sm()
        .text_color(colors.muted_text)
        .child(item(Icon::Milestone, SharedString::from(&stats.steps)))
        .child(dot())
        .child(item(Icon::FileText, SharedString::from(&stats.files)))
        .child(dot())
        .child(
            div()
                .flex()
                .gap_2()
                .font_family(*CODE_FONT)
                .child(
                    div()
                        .text_color(colors.success)
                        .child(SharedString::from(&stats.added)),
                )
                .child(
                    div()
                        .text_color(colors.danger)
                        .child(SharedString::from(&stats.removed)),
                ),
        )
        .children(stats.open_threads.clone().map(|threads| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(dot())
                .child(item(Icon::MessageSquare, SharedString::from(threads)))
        }))
        .children(stats.started_read.clone().map(|started_read| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(dot())
                .child(item(Icon::Check, SharedString::from(started_read)))
        }))
}

/// Where a file's change in a local review comes from, as a chip.
fn change_chip(colors: &Colors, change: LocalChange) -> impl IntoElement {
    chip(
        colors,
        match change {
            LocalChange::Committed => Tone::Neutral,
            LocalChange::Uncommitted => Tone::Warning,
            LocalChange::Both => Tone::Accent,
        },
    )
    .child(change.label())
}

/// One line of the local overview card: a quiet label and its value.
fn local_fact(colors: &Colors, label: &'static str, value: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_baseline()
        .gap_3()
        .child(
            div()
                .flex_none()
                .w(px(96.))
                .text_xs()
                .text_color(colors.muted_text)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// What a local review is of, in place of the PR description: where it is, which branch against
/// which, the branch's commits and how many files are not committed.
fn local_overview_card(colors: &Colors, local: &LocalFacts) -> impl IntoElement {
    let commits = if local.commit_subjects.is_empty() {
        vec![
            div()
                .text_color(colors.muted_text)
                .child(format!("None beyond {}", local.default_branch))
                .into_any_element(),
        ]
    } else {
        local
            .commit_subjects
            .iter()
            .map(|subject| {
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(bullet(colors, SUMMARY_LINE))
                    .child(div().flex_1().min_w_0().child(subject.clone()))
                    .into_any_element()
            })
            .collect()
    };
    div()
        .pb_4()
        .w_full()
        .max_w(READING_WIDTH)
        .flex()
        .flex_col()
        .gap_2()
        .child(note_label("LOCAL CHANGES").text_color(colors.muted_text))
        .child(
            card(colors)
                .px_5()
                .py_4()
                .flex()
                .flex_col()
                .gap_2()
                .text_sm()
                .children(
                    local
                        .origin
                        .as_ref()
                        .map(|origin| local_fact(colors, "Repo", origin.repo.to_string())),
                )
                .child(local_fact(
                    colors,
                    "Folder",
                    local.folder.as_path().display().to_string(),
                ))
                .child(local_fact(
                    colors,
                    "Branch",
                    format!(
                        "{} (compared with {})",
                        local.branch.as_str(),
                        local.default_branch
                    ),
                ))
                .child(local_fact(
                    colors,
                    "Commits",
                    div().flex().flex_col().children(commits),
                ))
                .child(local_fact(
                    colors,
                    "Working tree",
                    local.uncommitted_label(),
                )),
        )
}

/// How big a basic review's PR is, in the numbers the review truly has.
fn overview_stats(colors: &Colors, stats: OverviewStats) -> impl IntoElement {
    let item = |icon_name: Icon, text: String| {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(sized_icon(icon_name, colors.muted_text, px(14.)))
            .child(text)
    };
    let dot = || div().size(px(3.)).rounded_full().bg(colors.border);
    div()
        .pb_5()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_3()
        .text_sm()
        .text_color(colors.muted_text)
        .child(item(
            Icon::FileText,
            pluralised(stats.files, "file changed", "files changed"),
        ))
        .child(dot())
        .child(line_counts(colors, stats.added_lines, stats.removed_lines))
        .children((stats.open_threads > 0).then(|| {
            div().flex().items_center().gap_3().child(dot()).child(item(
                Icon::MessageSquare,
                pluralised(stats.open_threads, "open thread", "open threads"),
            ))
        }))
}

/// Added lines in green, removed in red.
fn line_counts(colors: &Colors, added: usize, removed: usize) -> impl IntoElement {
    div()
        .flex_none()
        .flex()
        .gap_2()
        .text_sm()
        .font_family(*CODE_FONT)
        .child(div().text_color(colors.success).child(format!("+{added}")))
        .child(
            div()
                .text_color(colors.danger)
                .child(format!("\u{2212}{removed}")),
        )
}

/// One top-level step on the route: its number on the route line, its title and why, and whether
/// it is read or has something to check. A click goes there.
fn route_stop(stop: &RouteStopRow, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let colors = Colors::active(cx);
    let disc = div()
        .size(px(24.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .border_1()
        .map(|disc| {
            if stop.is_read {
                disc.bg(colors.success_background)
                    .border_color(colors.success_border)
                    .child(sized_icon(Icon::Check, colors.success, px(13.)))
            } else {
                disc.bg(colors.background)
                    .border_color(colors.current_border)
                    .text_color(colors.accent)
                    .child(SharedString::from(&stop.number))
            }
        });
    let line = |is_shown: bool| {
        div()
            .w(px(2.))
            .flex_1()
            .when(is_shown, |line| line.bg(colors.current_border))
    };
    let id = stop.id.clone();
    div()
        .id(SharedString::from(format!("route-{}", stop.id)))
        .max_w(READING_WIDTH)
        .flex()
        .gap_3()
        .px_2()
        .rounded(px(CONTROL_RADIUS))
        .cursor_pointer()
        .hover(|row| row.bg(colors.hover))
        .active(|row| row.bg(colors.pressed))
        .on_click(cx.listener(move |this, _, _, cx| {
            let id = id.clone();
            this.change_review(cx, move |review| review.go_to(&id));
        }))
        .child(
            div()
                .flex_none()
                .w(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .h(px(10.))
                        .w(px(2.))
                        .when(!stop.is_first, |line| line.bg(colors.current_border)),
                )
                .child(disc)
                .child(line(!stop.is_last)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .py_2p5()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if stop.is_read {
                                    colors.muted_text
                                } else {
                                    colors.text
                                })
                                .child(InlineMarkdown::new(
                                    format!("route-title-{}", stop.id),
                                    &stop.title,
                                )),
                        )
                        .children(stop.has_check.then(|| check_flag(&colors)))
                        .children(stop.details.clone().map(|details| {
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(colors.muted_text)
                                .child(SharedString::from(details))
                        })),
                )
                .children((!stop.why.is_empty()).then(|| {
                    div()
                        .text_sm()
                        .text_color(colors.muted_text)
                        .line_clamp(2)
                        .child(InlineMarkdown::new(
                            format!("route-why-{}", stop.id),
                            &stop.why,
                        ))
                })),
        )
}

/// Whether the finish card also offers to read the tour again from its first step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewAgain {
    Offered,
    NotOffered,
}

/// The way into the tour: its start, or the first step not read yet. Finishing has its own card.
/// Whether the lead names the step it goes to, or a label beside it already does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadLabel {
    Named,
    Short,
}

/// The file a lead names.
fn next_file_screen(lead: &TourLead) -> Screen {
    match lead {
        TourLead::StartFile(file) | TourLead::ContinueFile(file) => Screen::File(file.path.clone()),
        _ => Screen::Overview,
    }
}

fn lead_button(
    lead: &TourLead,
    label_kind: LeadLabel,
    id: &'static str,
    cx: &mut Context<OneStepLayout>,
) -> Option<AnyElement> {
    let colors = Colors::active(cx);
    let (place, label, command) = match lead {
        TourLead::Start(first) => (
            Screen::Step(first.id.clone()),
            div().child("Start the tour").into_any_element(),
            Command::NextScreen,
        ),
        TourLead::StartFile(first) => (
            Screen::File(first.path.clone()),
            div().child("Start reading").into_any_element(),
            Command::NextScreen,
        ),
        TourLead::Continue(next) if label_kind == LeadLabel::Short => (
            Screen::Step(next.id.clone()),
            div().child("Continue").into_any_element(),
            Command::NextUnreadStep,
        ),
        TourLead::ContinueFile(_) if label_kind == LeadLabel::Short => (
            next_file_screen(lead),
            div().child("Continue").into_any_element(),
            Command::NextUnreadStep,
        ),
        TourLead::ContinueFile(next) => (
            Screen::File(next.path.clone()),
            div()
                .flex()
                .min_w_0()
                .gap_1()
                .child(div().flex_none().child("Continue at"))
                .child(div().min_w_0().truncate().child(next.path.to_string()))
                .into_any_element(),
            Command::NextUnreadStep,
        ),
        TourLead::Continue(next) => (
            Screen::Step(next.id.clone()),
            div()
                .flex()
                .min_w_0()
                .gap_1()
                .child(div().flex_none().child("Continue at"))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(InlineMarkdown::new(format!("{id}-title"), &next.title)),
                )
                .into_any_element(),
            Command::NextUnreadStep,
        ),
        TourLead::Finish { .. } | TourLead::FinishFiles { .. } | TourLead::Nowhere => return None,
    };
    Some(
        button(&colors, id, ButtonKind::Primary, ButtonSize::Large)
            .flex_shrink(1.)
            .min_w_0()
            .max_w(px(460.))
            .on_click(cx.listener(move |this, _, _, cx| {
                let place = place.clone();
                this.change_review(cx, move |review| match &place {
                    Screen::Step(id) => review.go_to(id),
                    Screen::File(path) => review.go_to_file(path),
                    Screen::Summary | Screen::Overview | Screen::SafetyNet => {}
                });
            }))
            .child(label)
            .child(button_keys(&colors, ButtonKind::Primary, command, cx))
            .into_any_element(),
    )
}

/// The tour's end once every step is read: a calm success card whose check pops in once, with
/// Finish review when the PR takes a review from the reader, or Send tasks when a local review
/// has tasks queued.
fn finish_card(
    count: usize,
    unit: ReadUnit,
    finish: Option<FinishAction>,
    review_again: ReviewAgain,
    id: &'static str,
    cx: &mut Context<OneStepLayout>,
) -> impl IntoElement {
    let colors = Colors::active(cx);
    let title = match (count, unit) {
        (1, ReadUnit::Steps) => "The one step is read".to_owned(),
        (1, ReadUnit::Files) => "The one file is read".to_owned(),
        (count, unit) => format!("All {count} {} read", unit.plural()),
    };
    let finish_button = finish.map(|finish| {
        let (icon, label, intent) = match finish {
            FinishAction::FinishReview => (Icon::ListChecks, "Finish review", Intent::FinishReview),
            FinishAction::SendTasks => (Icon::Send, "Send tasks", Intent::SendTasks),
            FinishAction::MarkDone => (Icon::CircleCheck, "Mark done", Intent::MarkDone),
        };
        icon_label_button(
            &colors,
            SharedString::from(format!("{id}-finish")),
            ButtonKind::Primary,
            ButtonSize::Large,
            icon,
            label,
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            this.act(intent, window, cx);
        }))
    });
    let again = (review_again == ReviewAgain::Offered).then(|| {
        icon_label_button(
            &colors,
            SharedString::from(format!("{id}-again")),
            ButtonKind::Ghost,
            ButtonSize::Regular,
            Icon::RotateCcw,
            "Review again",
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_review(cx, |review| {
                review.go_to_summary();
                review.go_forward();
            });
        }))
    });
    callout(&colors, Tone::Success)
        .flex_1()
        .min_w_0()
        .pl_4()
        .pr_3()
        .flex()
        .items_center()
        .gap_4()
        .child(
            div()
                .flex_none()
                .size(px(36.))
                .rounded_full()
                .bg(colors.trail_done.diagonal())
                .flex()
                .items_center()
                .justify_center()
                .child(pop_in(
                    sized_icon(Icon::Check, colors.on_accent_fill, px(20.)),
                    SharedString::from(format!("{id}-check")),
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.text)
                        .child(title),
                )
                .child(div().text_sm().text_color(colors.muted_text).child(
                    match (unit, finish) {
                        (ReadUnit::Steps, Some(FinishAction::FinishReview)) => {
                            "Every step of the tour is read. Send your review when you are ready."
                        }
                        (ReadUnit::Steps, Some(FinishAction::SendTasks)) => {
                            "Every step of the tour is read. Send your tasks when you are ready."
                        }
                        (ReadUnit::Steps, Some(FinishAction::MarkDone)) => {
                            "Every step of the tour is read. Nothing new to send? Mark it done."
                        }
                        (ReadUnit::Steps, None) => "Every step of the tour is read.",
                        (ReadUnit::Files, Some(FinishAction::FinishReview)) => {
                            "Every changed file is read. Send your review when you are ready."
                        }
                        (ReadUnit::Files, Some(FinishAction::SendTasks)) => {
                            "Every changed file is read. Send your tasks when you are ready."
                        }
                        (ReadUnit::Files, Some(FinishAction::MarkDone)) => {
                            "Every changed file is read. Nothing new to send? Mark it done."
                        }
                        (ReadUnit::Files, None) => "Every changed file is read.",
                    },
                )),
        )
        .children(again)
        .children(finish_button)
}

/// Whether the footer's primary action moves on to another screen, which its key says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Onward {
    Goes,
    Stays,
}

fn toolbar_toggle(
    colors: &Colors,
    id: &'static str,
    is_on: bool,
    icon: Icon,
    label: &'static str,
    command: Command,
    cx: &App,
) -> Stateful<Div> {
    let kind = if is_on {
        ButtonKind::JoinedOn
    } else {
        ButtonKind::Joined
    };
    icon_label_button(colors, id, kind, ButtonSize::Small, icon, label)
        .child(button_keys(colors, kind, command, cx))
        .when(is_on, |toggle| {
            toggle.child(sized_icon(Icon::Check, colors.accent, px(12.)))
        })
}

pub(super) fn tick_button(
    is_ticked: bool,
    id: StepId,
    size: ButtonSize,
    cx: &mut Context<OneStepLayout>,
) -> impl IntoElement {
    let colors = Colors::active(cx);
    let kind = if is_ticked {
        ButtonKind::Done
    } else {
        ButtonKind::Secondary
    };
    button(&colors, "tick", kind, size)
        .gap_0()
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tick(&id, cx)))
        .child(grow_when_shown(
            sized_icon(Icon::Check, colors.success, px(14.)),
            "tick-check",
            is_ticked,
            px(14.),
            px(6.),
        ))
        .child(if is_ticked { "Read" } else { "Mark as read" })
        .child(button_keys(&colors, kind, Command::ToggleRead, cx).ml_2())
}

/// The words of a button that puts something under Notes in the PR description.
#[derive(Clone, Copy)]
struct NotesLabels {
    add: &'static str,
    adding: &'static str,
    done: &'static str,
    tooltip: &'static str,
}

impl NotesLabels {
    const DIAGRAM: Self = Self {
        add: "Add to PR notes",
        adding: "Adding to PR notes…",
        done: "In PR notes",
        tooltip: "Put this diagram under Notes in the PR description, folded",
    };
    const PICTURE: Self = Self {
        add: "Add to PR notes",
        adding: "Uploading to PR notes…",
        done: "In PR notes",
        tooltip: "Upload this picture to GitHub and put it under Notes in the PR description, folded",
    };
    const SUMMARY: Self = Self {
        add: "Add summary to PR notes",
        adding: "Adding summary to PR notes…",
        done: "Summary in PR notes",
        tooltip: "Put this summary under Notes in the PR description, folded, its pictures uploaded to GitHub",
    };
}

fn notes_button(
    colors: &Colors,
    id: impl Into<ElementId>,
    state: &NotesButton,
    labels: NotesLabels,
    session: Entity<ReviewSession>,
    subject: NoteSubject,
) -> Div {
    let id = id.into();
    let small = |kind, icon| {
        button(colors, id.clone(), kind, ButtonSize::Small).child(button_icon(
            colors,
            kind,
            ButtonSize::Small,
            icon,
        ))
    };
    let add = small(ButtonKind::Ghost, Icon::FilePlus)
        .tooltip(tooltip(move |_| labels.tooltip.into()))
        .on_click(move |_, _, cx| {
            session.update(cx, |session, cx| session.add_to_notes(&subject, cx));
        })
        .child(labels.add);
    let (shown, failure) = match state {
        NotesButton::Add => (add, None),
        NotesButton::Adding => (
            small(ButtonKind::Ghost, Icon::FilePlus).child(labels.adding),
            None,
        ),
        NotesButton::InNotes => (
            small(ButtonKind::Done, Icon::Check).child(labels.done),
            None,
        ),
        NotesButton::Failed(reason) => (
            add,
            Some(
                div()
                    .flex()
                    .gap_1()
                    .child("Could not add it:")
                    .child(SharedString::from(Arc::clone(reason))),
            ),
        ),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(colors.muted_text)
        .child(shown)
        .children(failure)
}
