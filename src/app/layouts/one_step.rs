use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, actions, div, px,
};

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::app::components::ask_panel::{AskPanel, AskSetup, ask_button};
use crate::app::components::diagram::{DiagramView, Diagrams};
use crate::app::components::diff_view::{DiffView, OnMoreContext, threads_by_piece};
use crate::app::components::fold_toggle::FoldToggle;
use crate::app::components::new_commits_banner::{NewCommitsBanner, TourUpdate};
use crate::app::components::palette::{Palette, TEXT_FONT};
use crate::app::components::safety_net_view::SafetyNetView;
use crate::app::components::step_notes::StepNotes;
use crate::app::components::thread_view::ThreadElement;
use crate::code_source::CodeSource;
use crate::viewer::{
    GitHubPort, LineLocation, MoreContext, Place, Review, Screen, StepId, StepPage, ThreadView,
    TourUpdater,
};

const PUSH_DEBOUNCE: Duration = Duration::from_secs(1);

actions!(one_step, [GoForward, GoBack]);

/// The one layout: step tree on the left, one screen on the right (the summary, or one step).
pub struct OneStepLayout {
    review: Review,
    github: Arc<dyn GitHubPort>,
    diagrams: Entity<Diagrams>,
    updater: Arc<TourUpdater>,
    tour_update: TourUpdate,
    focus: FocusHandle,
    ask: Entity<AskPanel>,
    pr_head_check: Option<Task<()>>,
    threads_pull: Option<Task<()>>,
    push_debounce: Option<Task<()>>,
    is_syncing: bool,
    sync_again: bool,
    _focus_subscription: Subscription,
}

impl OneStepLayout {
    pub const KEY_CONTEXT: &str = "OneStepLayout";

    pub fn new(
        review: Review,
        source: Arc<dyn CodeSource>,
        ask: AskSetup,
        github: Arc<dyn GitHubPort>,
        updater: Arc<TourUpdater>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_subscription = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.check_pr_head(cx);
                this.sync(cx);
                this.pull_threads(cx);
            }
        });
        let diagrams = Self::new_diagrams(cx);
        let focus = cx.focus_handle();
        let ask = cx.new(|cx| AskPanel::new(ask, focus.clone(), cx));
        let mut layout = Self {
            review,
            github,
            diagrams,
            updater,
            tour_update: TourUpdate::Idle,
            focus,
            ask,
            pr_head_check: None,
            threads_pull: None,
            push_debounce: None,
            is_syncing: false,
            sync_again: false,
            _focus_subscription: focus_subscription,
        };
        layout.start_background_work(source, cx);
        layout
    }

    fn new_diagrams(cx: &mut Context<Self>) -> Entity<Diagrams> {
        let diagrams = cx.new(|_| Diagrams::default());
        cx.observe(&diagrams, |_, _, cx| cx.notify()).detach();
        diagrams
    }

    /// What a freshly opened review needs: the step shows plain code at once, the colours and
    /// the context lines follow from the background, then the PR head, viewed marks and threads.
    fn start_background_work(&mut self, source: Arc<dyn CodeSource>, cx: &mut Context<Self>) {
        let request = self.review.highlight_request();
        let files_at_head_request = self.review.files_at_head_request();
        cx.spawn(async move |this, cx| {
            let (files_at_head, highlights) = cx
                .background_executor()
                .spawn(async move {
                    (
                        files_at_head_request.read(source.as_ref()),
                        request.compute(source.as_ref()),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.review.set_files_at_head(files_at_head);
                this.review.set_highlights(highlights);
                cx.notify();
            })
            .ok();
        })
        .detach();
        self.check_pr_head(cx);
        self.sync(cx);
        self.pull_threads(cx);
    }

    /// Runs only on the button. The old `Review` is replaced, so its ticks cannot overwrite the new state.
    fn update_tour(&mut self, cx: &mut Context<Self>) {
        if self.tour_update == TourUpdate::Running {
            return;
        }
        self.tour_update = TourUpdate::Running;
        let (updater, github) = (self.updater.clone(), self.github.clone());
        cx.spawn(async move |this, cx| {
            let updated = cx
                .background_executor()
                .spawn(async move { updater.run(github.as_ref()) })
                .await;
            this.update(cx, |this, cx| {
                match updated {
                    Ok(updated) => {
                        let screen = this.review.screen().clone();
                        this.review = updated.review;
                        if let Screen::Step(id) = screen {
                            this.review.go_to(&id);
                        }
                        this.tour_update = TourUpdate::Idle;
                        this.diagrams = Self::new_diagrams(cx);
                        this.start_background_work(updated.source, cx);
                    }
                    Err(error) => this.tour_update = TourUpdate::Failed(format!("{error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn ask_button(
        &self,
        key: String,
        place: Place,
        label: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let count = self.ask.read(cx).count_at(&place);
        ask_button(SharedString::from(key), count).on_click(cx.listener(
            move |this, _, window, cx| {
                this.ask.update(cx, |panel, cx| {
                    panel.open(&this.review, place.clone(), label.clone(), window, cx)
                });
            },
        ))
    }

    fn check_pr_head(&mut self, cx: &mut Context<Self>) {
        let pr = self.review.pr().clone();
        let github = self.github.clone();
        self.pr_head_check = Some(cx.spawn(async move |this, cx| {
            let tips = cx
                .background_executor()
                .spawn(async move { github.pull_request_tips(&pr) })
                .await;
            match tips {
                Ok(tips) => {
                    let _ = this.update(cx, |this, cx| {
                        this.review.pr_head_is(tips.head);
                        cx.notify();
                    });
                }
                Err(error) => eprintln!("warning: could not check the PR's head: {error:#}"),
            }
        }));
    }

    /// Pushes what is queued and pulls what changed on GitHub, off the UI thread.
    fn sync(&mut self, cx: &mut Context<Self>) {
        if self.is_syncing {
            self.sync_again = true;
            return;
        }
        self.is_syncing = true;
        let request = self.review.sync_request();
        let (github, pr) = (self.github.clone(), self.review.pr().clone());
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { request.run(github.as_ref(), &pr) })
                .await;
            this.update(cx, |this, cx| {
                if let Err(error) = this.review.apply_sync(outcome) {
                    eprintln!("could not sync with GitHub: {error:#}");
                }
                this.is_syncing = false;
                if std::mem::take(&mut this.sync_again) {
                    this.sync(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn sync_after_debounce(&mut self, cx: &mut Context<Self>) {
        self.push_debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PUSH_DEBOUNCE).await;
            this.update(cx, |this, cx| this.sync(cx)).ok();
        }));
    }

    fn pull_threads(&mut self, cx: &mut Context<Self>) {
        let pr = self.review.pr().clone();
        let github = self.github.clone();
        self.threads_pull = Some(cx.spawn(async move |this, cx| {
            let threads = cx
                .background_executor()
                .spawn(async move { github.review_threads(&pr) })
                .await;
            match threads {
                Ok(threads) => {
                    let _ = this.update(cx, |this, cx| {
                        this.review.set_threads(&threads);
                        cx.notify();
                    });
                }
                Err(error) => eprintln!("warning: could not read the review threads: {error:#}"),
            }
        }));
    }

    /// Threads under a line come with their line, the rest go after the code.
    fn thread_elements(
        &self,
        views: &[ThreadView],
        cx: &mut Context<Self>,
    ) -> (Vec<(LineLocation, AnyElement)>, Vec<AnyElement>) {
        let mut under_lines = Vec::new();
        let mut after_code = Vec::new();
        for view in views {
            let id = view.thread.id.clone();
            let element = ThreadElement::new(
                view.clone(),
                cx.listener(move |this, _, _, cx| {
                    this.review.toggle_thread_fold(&id);
                    cx.notify();
                }),
            )
            .into_any_element();
            match &view.anchor {
                Some(location) => under_lines.push((location.clone(), element)),
                None => after_code.push(element),
            }
        }
        (under_lines, after_code)
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.review.go_forward() {
            cx.notify();
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.review.go_back() {
            cx.notify();
        }
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let summary_row = div()
            .id("summary")
            .px_3()
            .py_1()
            .cursor_pointer()
            .when_current(self.review.screen() == &Screen::Summary)
            .on_click(cx.listener(|this, _, _, cx| {
                this.review.go_to_summary();
                cx.notify();
            }))
            .child("Summary");
        let progress = self.review.progress();
        let progress_row = div()
            .px_3()
            .pb_2()
            .text_color(Palette::muted_text())
            .child(format!("Read {} of {}", progress.read, progress.total));
        let sync_row = self.review.sync_banner().map(|reason| {
            div()
                .px_3()
                .pb_2()
                .text_color(Palette::muted_text())
                .child(format!("Not syncing with GitHub: {reason}"))
        });
        let not_synced_row = self.review.not_synced().then(|| {
            div()
                .px_3()
                .pb_2()
                .text_color(Palette::muted_text())
                .child("● not synced with GitHub yet")
        });
        let step_rows = self.review.sidebar().into_iter().map(|entry| {
            let id: StepId = entry.id.clone();
            div()
                .id(SharedString::from(format!("step-{id}")))
                .pl(px(12. + 16. * entry.depth as f32))
                .pr_3()
                .py_1()
                .cursor_pointer()
                .when_current(entry.is_current)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.review.go_to(&id);
                    cx.notify();
                }))
                .when_ticked(entry.is_ticked || entry.is_gone)
                .child(match entry.thread_count {
                    0 => entry.title.to_string(),
                    count => format!("{} ({count} 💬)", entry.title),
                })
        });
        div()
            .id("sidebar")
            .w(px(280.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .py_2()
            .bg(Palette::sidebar())
            .border_r_1()
            .border_color(Palette::border())
            .text_sm()
            .child(progress_row)
            .children(sync_row)
            .children(not_synced_row)
            .child(summary_row)
            .children(step_rows)
            .children(self.safety_net_row(cx))
    }

    fn safety_net_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let page = self.review.safety_net()?;
        Some(
            div()
                .id("safety-net")
                .px_3()
                .py_1()
                .cursor_pointer()
                .when_current(self.review.screen() == &Screen::SafetyNet)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.review.go_to_safety_net();
                    cx.notify();
                }))
                .child(match page.threads.len() {
                    0 => format!("{} ({})", SafetyNetView::TITLE, page.pieces.len()),
                    threads => format!(
                        "{} ({}, {threads} 💬)",
                        SafetyNetView::TITLE,
                        page.pieces.len()
                    ),
                }),
        )
    }

    fn screen(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.show_diagram(window, cx);
        match self.review.screen() {
            Screen::Summary => {
                let button =
                    self.ask_button("ask-tour".into(), Place::Tour, "the whole tour".into(), cx);
                summary_screen(self.review.title(), self.review.summary(), button)
                    .into_any_element()
            }
            Screen::Step(_) => match self.review.current_step() {
                Some(page) => self.step_screen(page, cx).into_any_element(),
                None => div().into_any_element(),
            },
            Screen::SafetyNet => match self.review.safety_net() {
                Some(page) => {
                    let (under_lines, after_code) = self.thread_elements(&page.threads, cx);
                    SafetyNetView::new(page, self.on_more_context(cx))
                        .with_threads(under_lines, after_code)
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
        }
    }

    fn show_diagram(&self, window: &mut Window, cx: &mut Context<Self>) {
        let page = match self.review.screen() {
            Screen::Step(_) => self.review.current_step(),
            _ => None,
        };
        let diagram = page
            .as_ref()
            .filter(|page| !page.is_folded)
            .and_then(|page| page.step.diagram.as_deref());
        self.diagrams
            .update(cx, |diagrams, cx| diagrams.show(diagram, window, cx));
    }

    fn step_screen(&self, page: StepPage<'_>, cx: &mut Context<Self>) -> impl IntoElement {
        let step_id = StepId::from(page.step.id.as_str());
        let step_button = self.ask_button(
            format!("ask-step-{step_id}"),
            Place::Step(step_id.clone()),
            page.step.title.clone(),
            cx,
        );
        let on_more_context = self.on_more_context(cx);
        let (under_lines, after_code) = self.thread_elements(&page.threads, cx);
        let mut threads_of_piece = threads_by_piece(&page.pieces, under_lines).into_iter();
        let shows_change_since_read = page
            .since_read
            .as_ref()
            .is_some_and(|since| !since.showing_full_diff);
        let pieces: Vec<_> = page
            .pieces
            .iter()
            .map(|piece| {
                let threads = threads_of_piece.next().unwrap_or_default();
                let place = Place::Piece {
                    step: step_id.clone(),
                    piece: piece.id.to_string(),
                };
                let button = self.ask_button(
                    format!("ask-piece-{step_id}-{}", piece.id),
                    place,
                    piece.id.to_string(),
                    cx,
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().flex().justify_end().child(button))
                    .child(
                        DiffView::new(piece.clone(), on_more_context.clone())
                            .with_threads(threads)
                            .in_change_since_read(shows_change_since_read),
                    )
            })
            .collect();
        let diagram = (!page.is_folded && page.step.diagram.is_some())
            .then(|| DiagramView::new(self.diagrams.clone()));
        step_screen(
            page,
            step_button,
            diagram,
            pieces,
            self.review.ignores_whitespace(),
            after_code,
            cx,
        )
    }

    fn on_more_context(&self, cx: &mut Context<Self>) -> OnMoreContext {
        Rc::new(cx.listener(|this, more: &MoreContext, _, cx| {
            this.review.show_more_context(more);
            cx.notify();
        }))
    }

    fn toggle_tick(&mut self, id: &StepId, cx: &mut Context<Self>) {
        let saved = if self.review.is_ticked(id) {
            self.review.untick(id)
        } else {
            self.review.tick(id)
        };
        if let Err(error) = saved {
            eprintln!("could not save the read tick: {error:#}");
        }
        self.sync_after_debounce(cx);
        cx.notify();
    }
}

trait TickedRow {
    fn when_ticked(self, is_ticked: bool) -> Self;
}

impl<E: Styled> TickedRow for E {
    fn when_ticked(self, is_ticked: bool) -> Self {
        if is_ticked {
            self.text_color(Palette::muted_text())
        } else {
            self
        }
    }
}

trait CurrentRow {
    fn when_current(self, is_current: bool) -> Self;
}

impl<E: Styled> CurrentRow for E {
    fn when_current(self, is_current: bool) -> Self {
        if is_current {
            self.bg(Palette::current())
        } else {
            self
        }
    }
}

fn summary_screen(title: &str, summary: &[String], ask: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().text_2xl().child(title.to_string()))
        .child(div().flex().child(ask))
        .children(
            summary
                .iter()
                .map(|bullet| div().child(format!("• {bullet}"))),
        )
}

fn step_screen(
    page: StepPage<'_>,
    ask: impl IntoElement,
    diagram: Option<DiagramView>,
    pieces: Vec<impl IntoElement>,
    ignores_whitespace: bool,
    after_code: Vec<AnyElement>,
    cx: &mut Context<OneStepLayout>,
) -> impl IntoElement {
    let step = page.step;
    let id = StepId::from(step.id.as_str());
    let fold_toggle = (page.is_folded || step.skim || page.is_ticked).then(|| {
        FoldToggle::new(
            page.is_folded,
            page.pieces.len(),
            cx.listener(move |this, _, _, cx| {
                this.review.toggle_fold(&id);
                cx.notify();
            }),
        )
    });
    let (pieces, after_code) = if page.is_folded {
        (Vec::new(), Vec::new())
    } else {
        (pieces, after_code)
    };
    let full_diff_toggle = page.since_read.as_ref().map(|since| {
        let id = StepId::from(step.id.as_str());
        let label = if since.showing_full_diff {
            "Show only the change since your read"
        } else {
            "Show the full diff"
        };
        div()
            .id("full-diff-toggle")
            .text_sm()
            .cursor_pointer()
            .text_color(Palette::muted_text())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.review.toggle_full_diff(&id);
                cx.notify();
            }))
            .child(label)
    });
    let repin_notes = (page.gone_pieces > 0 || page.text_may_be_old).then(|| {
        div()
            .flex()
            .gap_3()
            .text_sm()
            .text_color(Palette::muted_text())
            .when(page.gone_pieces > 0, |row| {
                row.child(match page.gone_pieces {
                    1 => "1 piece gone since last version".to_string(),
                    count => format!("{count} pieces gone since last version"),
                })
            })
            .when(page.text_may_be_old, |row| row.child("Text may be old"))
    });
    div()
        .flex()
        .flex_col()
        .gap_3()
        .when(page.is_gone, |column| {
            column.text_color(Palette::muted_text())
        })
        .child(step_header(&page, cx))
        .child(
            div()
                .text_color(Palette::muted_text())
                .child(step.why.clone()),
        )
        .children(diagram)
        .child(div().flex().child(ask))
        .child(StepNotes::of(step))
        .child(whitespace_toggle(ignores_whitespace, cx))
        .children(repin_notes)
        .children(full_diff_toggle)
        .children(fold_toggle)
        .children(pieces)
        .children(after_code)
}

fn step_header(page: &StepPage<'_>, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    let id = StepId::from(page.step.id.as_str());
    let title = div()
        .id("step-title")
        .flex_1()
        .text_xl()
        .child(page.step.title.clone());
    let title = if page.is_ticked || page.step.skim {
        let id = id.clone();
        title
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.review.toggle_fold(&id);
                cx.notify();
            }))
    } else {
        title
    };
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(title)
        .child(tick_button(page.is_ticked, id, cx))
}

fn whitespace_toggle(is_on: bool, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    div()
        .id("ignore-whitespace")
        .self_start()
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .cursor_pointer()
        .text_sm()
        .when_current(is_on)
        .on_click(cx.listener(|this, _, _, cx| {
            this.review.toggle_ignore_whitespace();
            cx.notify();
        }))
        .child("Hide whitespace")
}

fn tick_button(is_ticked: bool, id: StepId, cx: &mut Context<OneStepLayout>) -> impl IntoElement {
    div()
        .id("tick")
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(Palette::border())
        .cursor_pointer()
        .text_sm()
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tick(&id, cx)))
        .child(if is_ticked {
            "✓ Read"
        } else {
            "Mark as read"
        })
}

impl Focusable for OneStepLayout {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for OneStepLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::go_back))
            .size_full()
            .flex()
            .bg(Palette::background())
            .font_family(TEXT_FONT)
            .text_color(Palette::text())
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .children(self.review.old_head().map(|old_head| {
                        NewCommitsBanner::new(
                            old_head,
                            self.tour_update.clone(),
                            cx.listener(|this, _, _, cx| this.update_tour(cx)),
                        )
                    }))
                    .child(
                        div()
                            .id("screen")
                            .flex_1()
                            .overflow_y_scroll()
                            .p_6()
                            .child(self.screen(window, cx)),
                    ),
            )
            .when(self.ask.read(cx).is_open(), |row| {
                row.child(self.ask.clone())
            })
    }
}
