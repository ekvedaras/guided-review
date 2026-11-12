use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClipboardItem, Context, Div, ElementId, Entity, EventEmitter, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::app::review_session::ReviewSession;
use crate::config::AskSettings;
use crate::viewer::{
    AgentLaunch, AgentLauncher, Ask, AskDesk, AskId, AskProcessEvent, AskProcesses, AskTarget,
    Author, NotDelivered, Place, Presence, Question, Review, Shown, TurnStopper,
};

use super::button::{ButtonKind, ButtonSize, icon_button, icon_label_button};
use super::card::card;
use super::icon::{Icon, icon, sized_icon};
use super::key_cap::{KeyLabel, key_hint};
use super::markdown_view::{MarkdownView, parsed_markdown};
use super::motion::{OVERLAY, PANEL_SLIDE, along, busy_dot, panel_level};
use super::overlay::{close_presence, input_blocker};
use super::palette::{CARD_RADIUS, Colors, TEXT_FONT};
use super::text_box::{Field, TextBox};
use super::text_input::Lines;

/// Everything the panel needs to start the AI, built once at startup.
pub struct AskSetup {
    pub launcher: Arc<dyn AgentLauncher>,
    pub settings: AskSettings,
}

/// How often the panel looks for AI processes idle long enough to stop.
const IDLE_CHECK: Duration = Duration::from_secs(30);

/// One live `claude` process, driven from its own thread so the window never waits on it.
/// Dropping it ends the thread, and the process with it.
struct Worker {
    questions: mpsc::Sender<String>,
    /// Set by the thread once the process runs.
    stopper: Arc<Mutex<Option<Arc<dyn TurnStopper>>>>,
}

impl Worker {
    fn spawn(
        launcher: Arc<dyn AgentLauncher>,
        launch: AgentLaunch,
        id: AskId,
        into: UnboundedSender<(AskId, AskProcessEvent)>,
    ) -> Self {
        let (questions, incoming) = mpsc::channel::<String>();
        let stopper = Arc::new(Mutex::new(None));
        let stopper_of_thread = Arc::clone(&stopper);
        std::thread::spawn(move || {
            // A closed channel means the window is gone, so nobody is left to tell.
            let tell = |event: AskProcessEvent| {
                into.unbounded_send((id, event)).ok();
            };
            let mut agent = match launcher.start(&launch) {
                Ok(agent) => agent,
                Err(error) => {
                    tell(AskProcessEvent::Failed(format!("{error:#}")));
                    return;
                }
            };
            if let Ok(mut slot) = stopper_of_thread.lock() {
                *slot = Some(agent.turn_stopper());
            }
            while let Ok(message) = incoming.recv() {
                let result = agent.ask(&message, &mut |line| {
                    tell(AskProcessEvent::Line(line.to_owned()));
                });
                match result {
                    Ok(()) => {
                        tell(AskProcessEvent::TurnEnded);
                    }
                    Err(error) => {
                        tell(AskProcessEvent::Failed(format!("{error:#}")));
                        return;
                    }
                }
            }
        });
        Self { questions, stopper }
    }
}

/// The Asks' processes, each on a thread of its own, reporting into the panel.
struct WorkerThreads {
    launcher: Arc<dyn AgentLauncher>,
    workers: HashMap<AskId, Worker>,
    into: UnboundedSender<(AskId, AskProcessEvent)>,
}

impl AskProcesses for WorkerThreads {
    fn start(&mut self, ask: AskId, launch: AgentLaunch) {
        let worker = Worker::spawn(Arc::clone(&self.launcher), launch, ask, self.into.clone());
        self.workers.insert(ask, worker);
    }

    fn stop(&mut self, ask: AskId) {
        self.workers.remove(&ask);
    }

    fn is_running(&self, ask: AskId) -> bool {
        self.workers.contains_key(&ask)
    }

    fn deliver(&mut self, ask: AskId, message: String) -> Result<(), NotDelivered> {
        let worker = self.workers.get(&ask).ok_or(NotDelivered)?;
        // The send error only hands the message back: the thread, and so the process, is gone.
        worker
            .questions
            .send(message)
            .map_err(|_returned_message| NotDelivered)
    }

    fn turn_stopper(&self, ask: AskId) -> Option<Arc<dyn TurnStopper>> {
        self.workers.get(&ask)?.stopper.lock().ok()?.clone()
    }
}

/// Asks the layout to put an answer into the comment box; only the review can place it.
pub struct UseAsComment(pub Ask);

/// Asks the layout to type the Ask into the reviewer's own tour agent; only the session knows the agent.
pub struct HandToAgent {
    pub ask: Ask,
    pub place: String,
}

/// The right side panel: Asks about one place, newest first, and the box to ask the next one.
/// The review's `AskDesk` decides; the panel draws it, takes the question and runs the processes.
pub struct AskPanel {
    session: Entity<ReviewSession>,
    settings: AskSettings,
    question: String,
    processes: WorkerThreads,
    /// The panel's, so escape closes it when no question can be typed.
    focus: FocusHandle,
    question_focus: FocusHandle,
    layout_focus: FocusHandle,
    /// Whether the panel is leaving; the desk knows whether it is open.
    presence: Presence,
    /// What the desk was open on as the panel began to leave, drawn until it is gone.
    leaving_target: Option<AskTarget>,
}

/// How wide the panel is once in.
const PANEL_WIDTH: f32 = 380.;

impl AskPanel {
    pub const KEY_CONTEXT: &str = "AskInput";

    pub fn new(
        setup: AskSetup,
        session: Entity<ReviewSession>,
        layout_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let (from_workers, incoming): (_, UnboundedReceiver<(AskId, AskProcessEvent)>) =
            unbounded();
        cx.spawn(async move |this, cx| {
            let mut incoming = incoming;
            while let Some((id, event)) = incoming.next().await {
                if this
                    .update(cx, |panel, cx| panel.on_process_event(id, event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(IDLE_CHECK).await;
                if this.update(cx, Self::stop_idle).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            session,
            settings: setup.settings,
            question: String::new(),
            processes: WorkerThreads {
                launcher: setup.launcher,
                workers: HashMap::new(),
                into: from_workers,
            },
            focus: cx.focus_handle(),
            question_focus: cx.focus_handle(),
            layout_focus,
            presence: Presence::lasting(OVERLAY),
            leaving_target: None,
        }
    }

    pub fn is_open(&self, cx: &App) -> bool {
        self.desk(cx).is_open()
    }

    /// Drawn: open, or on its way out.
    pub fn is_shown(&self, cx: &App) -> bool {
        self.is_open(cx) || self.presence.shown().is_some()
    }

    fn desk<'a>(&self, cx: &'a App) -> &'a AskDesk {
        self.session.read(cx).review().asks()
    }

    fn change<T>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Review, &mut WorkerThreads) -> T,
    ) -> T {
        let processes = &mut self.processes;
        self.session.update(cx, |session, cx| {
            session.change_asks(cx, |review| change(review, processes))
        })
    }

    /// Shows the Asks of a place and gets the AI ready: its process starts now, not at the first question.
    pub fn open(
        &mut self,
        place: Place,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.settings.clone();
        self.change(cx, |review, processes| {
            review.open_ask(place, label, &settings, Instant::now(), processes);
        });
        self.presence.open(Instant::now());
        self.leaving_target = None;
        match self.desk(cx).target() {
            Some(AskTarget::Ready { .. }) => {
                self.question.clear();
                window.focus(&self.question_focus, cx);
            }
            _ => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open(cx) {
            return;
        }
        self.leaving_target = self.desk(cx).target().cloned();
        close_presence(self, cx, |this| &mut this.presence);
        self.change(cx, |review, _| review.change_asks(AskDesk::close));
        window.focus(&self.layout_focus, cx);
        cx.notify();
    }

    fn stop_idle(&mut self, cx: &mut Context<Self>) {
        self.change(cx, |review, processes| {
            review.change_asks(|desk| desk.stop_idle(Instant::now(), processes));
        });
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let question = self.question.clone();
        let sent = self.change(cx, |review, processes| {
            review.change_asks(|desk| desk.send(&question, Instant::now(), processes))
        });
        if sent == Question::Taken {
            self.question.clear();
        }
        cx.notify();
    }

    fn follow_up(&mut self, id: AskId, cx: &mut Context<Self>) {
        self.question.clear();
        self.change(cx, |review, processes| {
            review.change_asks(|desk| desk.follow_up(id, Instant::now(), processes));
        });
        cx.notify();
    }

    fn stop_turn(&mut self, id: AskId, cx: &mut Context<Self>) {
        self.change(cx, |review, processes| {
            review.change_asks(|desk| desk.stop_turn(id, processes));
        });
        cx.notify();
    }

    fn on_process_event(&mut self, id: AskId, event: AskProcessEvent, cx: &mut Context<Self>) {
        self.change(cx, |review, processes| {
            review.change_asks(|desk| desk.on_process_event(id, event, Instant::now(), processes));
        });
        cx.notify();
    }

    fn hand_to_agent(&mut self, ask: &Ask, place: &str, cx: &mut Context<Self>) {
        let id = ask.id;
        self.change(cx, |review, _| {
            review.change_asks(|desk| desk.hand_off_started(id));
        });
        cx.emit(HandToAgent {
            ask: ask.clone(),
            place: place.to_owned(),
        });
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.close(window, cx);
        }
    }

    fn question_box(&self, cx: &Context<Self>) -> TextBox {
        let (on_edit, on_submit, on_cancel) = (cx.entity(), cx.entity(), cx.entity());
        TextBox::new(
            "ask-question",
            &self.question_focus,
            &self.question,
            Lines::Single,
        )
        .on_edit(move |question, _, cx| {
            on_edit.update(cx, |this, cx| {
                this.question = question;
                cx.notify();
            });
        })
        .on_submit(move |_, cx| on_submit.update(cx, Self::send))
        .on_cancel(move |window, cx| on_cancel.update(cx, |this, cx| this.close(window, cx)))
    }

    fn ask_block(
        &self,
        desk: &AskDesk,
        ask: &Ask,
        place: &str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let colors = Colors::active(cx);
        let id = ask.id;
        let is_selected = desk.selected() == Some(id);
        let messages = ask.messages.iter().enumerate().map(|(index, message)| {
            let text = MarkdownView::new(
                format!("ask-{}-{index}", id_label(id)),
                parsed_markdown(&message.text),
            );
            match message.author {
                Author::Reviewer => div().font_weight(gpui::FontWeight::BOLD).child(text),
                Author::Ai => div().child(text),
            }
        });
        let mut block = card(&colors)
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2p5()
            .when(is_selected, |block| block.border_color(colors.accent))
            .children(messages);
        if let Some(activity) = &ask.activity {
            block = block.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(colors.muted_text)
                    .child(busy_dot(
                        &colors,
                        SharedString::from(format!("ask-activity-{}", id_label(id))),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(format!("{activity}...")),
                    ),
            );
        }
        if let Some(failure) = &ask.failure {
            block = block.child(div().text_color(colors.danger).child(failure.clone()));
        }
        if desk.code_changed_since(id) {
            block = block.child(
                div()
                    .text_color(colors.warning)
                    .child("Code changed since: this answer may be old"),
            );
        }
        let is_answering = desk.is_answering(id);
        let action = |name: String, icon: Icon, label: &'static str| {
            icon_label_button(
                &colors,
                SharedString::from(format!("{name}-{}", id_label(id))),
                ButtonKind::Ghost,
                ButtonSize::Small,
                icon,
                label,
            )
        };
        let mut actions = div().flex().flex_wrap().gap_1().ml(px(-8.));
        if is_answering {
            actions = actions.child(
                action("stop-turn".to_owned(), Icon::Square, "Stop")
                    .on_click(cx.listener(move |this, _, _, cx| this.stop_turn(id, cx))),
            );
        }
        if let Some(answer) = ask.latest_answer().filter(|_| !is_answering) {
            let answer = answer.to_owned();
            actions = actions.child(
                action("copy-answer".to_owned(), Icon::Copy, "Copy answer").on_click(
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(answer.clone()));
                    },
                ),
            );
        }
        let has_lines = matches!(ask.place, Place::Lines { .. } | Place::Piece { .. });
        if has_lines && !is_answering && ask.latest_answer().is_some() {
            let ask_to_use = ask.clone();
            actions = actions.child(
                action(
                    "use-as-comment".to_owned(),
                    Icon::MessageSquare,
                    "Use as comment",
                )
                .on_click(
                    cx.listener(move |_, _, _, cx| cx.emit(UseAsComment(ask_to_use.clone()))),
                ),
            );
        }
        if !is_selected {
            actions = actions.child(
                action("follow-up".to_owned(), Icon::Reply, "Follow up")
                    .on_click(cx.listener(move |this, _, _, cx| this.follow_up(id, cx))),
            );
        }
        if !is_answering && !ask.messages.is_empty() {
            let (ask_to_hand, place) = (ask.clone(), place.to_owned());
            actions = actions.child(
                action(
                    "hand-to-agent".to_owned(),
                    Icon::Terminal,
                    "Hand to your agent",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.hand_to_agent(&ask_to_hand, &place, cx);
                })),
            );
        }
        let hand_off = desk.hand_off(id).map(|line| {
            div()
                .text_color(colors.muted_text)
                .child(SharedString::from(Arc::clone(line)))
        });
        block.child(actions).children(hand_off)
    }
}

impl EventEmitter<UseAsComment> for AskPanel {}
impl EventEmitter<HandToAgent> for AskPanel {}

fn id_label(id: AskId) -> String {
    serde_json::to_string(&id).unwrap_or_default()
}

impl Render for AskPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let desk = self.desk(cx);
        let Some(target) = desk.target().or(self.leaving_target.as_ref()) else {
            return div().into_any_element();
        };
        let place = target.label().to_owned();
        let blocks: Vec<_> = desk
            .at(target.place())
            .into_iter()
            .map(|ask| self.ask_block(desk, ask, &place, cx).into_any_element())
            .collect();
        let is_answering = desk.selected().is_some_and(|id| desk.is_answering(id));
        let input = match target {
            AskTarget::Unavailable { .. } => None,
            AskTarget::Ready { .. } => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(is_answering.then(|| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(colors.muted_text)
                            .child(busy_dot(&colors, "ask-waiting-dot"))
                            .child("Waiting for the answer...")
                    }))
                    .children(
                        desk.notice()
                            .map(|notice| div().text_color(colors.danger).child(notice.message())),
                    )
                    .child(
                        Field::new(&self.question_focus)
                            .px_3()
                            .h(px(36.))
                            .flex()
                            .items_center()
                            .child(self.question_box(cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_xs()
                            .text_color(colors.muted_text)
                            .child(key_hint(&colors, &[KeyLabel::Icon(Icon::Enter)], "to ask"))
                            .child(key_hint(&colors, &[KeyLabel::Text("esc")], "to close")),
                    ),
            ),
        };
        let empty_state = blocks.is_empty().then(|| {
            let (title, line, tone) = match target {
                AskTarget::Unavailable { why, .. } => {
                    ("Ask is not available", why.message(), colors.danger)
                }
                AskTarget::Ready { .. } => (
                    "Ask about this place",
                    "The AI reads the code and answers here. Follow up on an answer to keep going.",
                    colors.accent,
                ),
            };
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .px_6()
                .text_center()
                .child(
                    div()
                        .size(px(40.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(CARD_RADIUS))
                        .bg(colors.surface)
                        .child(sized_icon(Icon::Sparkles, tone, px(18.))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
                        .child(div().text_color(colors.muted_text).child(line)),
                )
        });
        let panel = div()
            .id("ask-panel")
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .w(px(PANEL_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(colors.overlay)
            .rounded_l(px(CARD_RADIUS))
            .border_l_1()
            .border_color(colors.subtle_border)
            .shadow(colors.page_shadow())
            .font_family(TEXT_FONT)
            .text_sm()
            .text_color(colors.text)
            .child(
                div()
                    .flex_none()
                    .h(px(40.))
                    .pl_4()
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(Icon::Sparkles, colors.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Ask"),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(colors.muted_text)
                                    .child(place),
                            ),
                    )
                    .child(
                        icon_button(
                            &colors,
                            "ask-close",
                            ButtonKind::Ghost,
                            ButtonSize::Small,
                            Icon::Close,
                            "Close",
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    ),
            )
            .child(
                div()
                    .id("ask-answers")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .children(blocks)
                    .children(empty_state),
            )
            .children(input.map(|input| div().flex_none().p_3().child(input)));
        // The page runs under the rounded corners, so they cut into the page and not into the sidebar.
        let panel = match self.presence.shown() {
            Some(Shown::Leaving) => panel.child(input_blocker()),
            Some(Shown::Entering) | None => panel,
        };
        let id = ("ask-panel-slide", self.presence.turn());
        let travel = self.presence.travel();
        // The panel's room grows and shrinks with its move, so the page glides wider as it leaves
        // instead of jumping when it is gone.
        along(
            div()
                .flex_none()
                .h_full()
                .flex()
                .justify_end()
                .overflow_hidden()
                .bg(colors.background)
                .child(along(panel, id, travel, |panel, position| {
                    let level = panel_level(position);
                    panel
                        .relative()
                        .opacity(level)
                        .left(PANEL_SLIDE * (1. - level))
                })),
            ("ask-panel-room", self.presence.turn()),
            travel,
            |room, position| room.w(px(PANEL_WIDTH * panel_level(position))),
        )
        .into_any_element()
    }
}

/// The button on a place, `Tinted` where it is the accent action of a bar and `Ghost` beside a
/// file's name. It shows how many Asks the place already has.
pub fn ask_button(
    colors: &Colors,
    id: impl Into<ElementId>,
    count: usize,
    kind: ButtonKind,
) -> Stateful<Div> {
    let ask = icon_label_button(colors, id, kind, ButtonSize::Small, Icon::Sparkles, "Ask");
    if count == 0 {
        return ask;
    }
    ask.child(
        div()
            .min_w(px(16.))
            .h(px(16.))
            .px_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(colors.current)
            .border_1()
            .border_color(colors.current_border)
            .text_color(colors.accent)
            .child(count.to_string()),
    )
}
