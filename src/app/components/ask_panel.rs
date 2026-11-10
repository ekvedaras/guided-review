use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::prelude::FluentBuilder;
use gpui::{
    ClipboardItem, Context, Div, ElementId, EventEmitter, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::config::AskSettings;
use crate::viewer::{
    AgentLaunch, AgentLauncher, AgentPool, Ask, AskId, AskTarget, Asks, Author, Opening, Place,
    Review, Screen, StepId, TurnStopper,
};

use super::button::{ButtonKind, ButtonSize, icon_button, icon_label_button};
use super::card::card;
use super::icon::Icon;
use super::markdown_view::{MarkdownView, parsed_markdown};
use super::motion::{Edge, OVERLAY, PANEL_SLIDE, busy_dot, slide_in};
use super::palette::{Colors, TEXT_FONT};
use super::text_box::{Field, TextBox};
use super::text_input::Lines;

/// Everything the panel needs to talk to the AI, built once at startup.
pub struct AskSetup {
    pub asks: Asks,
    pub launcher: Arc<dyn AgentLauncher>,
    /// Without a local clone the AI has no code to read, so Ask is off.
    pub clone: Option<PathBuf>,
    pub settings: AskSettings,
}

enum WorkerMessage {
    Line(String),
    TurnEnded,
    Failed(String),
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
        into: UnboundedSender<(AskId, WorkerMessage)>,
    ) -> Self {
        let (questions, incoming) = mpsc::channel::<String>();
        let stopper = Arc::new(Mutex::new(None));
        let stopper_of_thread = Arc::clone(&stopper);
        std::thread::spawn(move || {
            // A closed channel means the window is gone, so nobody is left to tell.
            let tell = |message: WorkerMessage| {
                into.unbounded_send((id, message)).ok();
            };
            let mut agent = match launcher.start(&launch) {
                Ok(agent) => agent,
                Err(error) => {
                    tell(WorkerMessage::Failed(format!("{error:#}")));
                    return;
                }
            };
            if let Ok(mut slot) = stopper_of_thread.lock() {
                *slot = Some(agent.turn_stopper());
            }
            while let Ok(message) = incoming.recv() {
                let result = agent.ask(&message, &mut |line| {
                    tell(WorkerMessage::Line(line.to_owned()));
                });
                match result {
                    Ok(()) => {
                        tell(WorkerMessage::TurnEnded);
                    }
                    Err(error) => {
                        tell(WorkerMessage::Failed(format!("{error:#}")));
                        return;
                    }
                }
            }
        });
        Self { questions, stopper }
    }

    fn stopper(&self) -> Option<Arc<dyn TurnStopper>> {
        self.stopper.lock().ok()?.clone()
    }
}

/// Asks the layout to put an answer into the comment box; only the review can place it.
pub struct UseAsComment(pub Ask);

/// Asks the layout to type the Ask into the reviewer's own tour agent; only the session knows the agent.
pub struct HandToAgent {
    pub ask: Ask,
    pub place: String,
}

/// Why the last question did not go out.
#[derive(Clone, Copy)]
enum AskNotice {
    AllAnswering,
}

impl AskNotice {
    fn message(self) -> &'static str {
        match self {
            Self::AllAnswering => "Three Asks are answering now: wait for one, or stop it",
        }
    }
}

/// The right side panel: Asks about one place, newest first, and the box to ask the next one.
pub struct AskPanel {
    setup: AskSetup,
    target: Option<AskTarget>,
    selected: Option<AskId>,
    question: String,
    pool: AgentPool,
    workers: HashMap<AskId, Worker>,
    /// Asks a re-pin changed the code under, as of the review last shown.
    code_changed: HashSet<AskId>,
    notice: Option<AskNotice>,
    /// What became of each Ask handed to the tour agent, or that it is being handed.
    hand_offs: HashMap<AskId, SharedString>,
    from_workers: UnboundedSender<(AskId, WorkerMessage)>,
    /// The panel's, so escape closes it when no question can be typed.
    focus: FocusHandle,
    question_focus: FocusHandle,
    layout_focus: FocusHandle,
}

impl AskPanel {
    pub const KEY_CONTEXT: &str = "AskInput";

    pub fn new(setup: AskSetup, layout_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        let (from_workers, incoming): (_, UnboundedReceiver<(AskId, WorkerMessage)>) = unbounded();
        cx.spawn(async move |this, cx| {
            let mut incoming = incoming;
            while let Some((id, message)) = incoming.next().await {
                if this
                    .update(cx, |panel, cx| panel.on_worker_message(id, message, cx))
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
                if this.update(cx, |panel, _| panel.stop_idle()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            setup,
            target: None,
            selected: None,
            question: String::new(),
            pool: AgentPool::default(),
            workers: HashMap::new(),
            code_changed: HashSet::new(),
            notice: None,
            hand_offs: HashMap::new(),
            from_workers,
            focus: cx.focus_handle(),
            question_focus: cx.focus_handle(),
            layout_focus,
        }
    }

    pub fn answering(&self) -> u16 {
        self.pool.answering()
    }

    pub fn is_open(&self) -> bool {
        self.target.is_some()
    }

    pub fn count_at(&self, place: &Place) -> usize {
        self.setup.asks.count_at(place)
    }

    /// Asks about selected lines in a step (or, with `None`, the safety net), by place.
    fn line_places(&self, step: Option<&StepId>) -> Vec<(Place, usize)> {
        let mut places: Vec<(Place, usize)> = Vec::new();
        for ask in self.setup.asks.all() {
            let Place::Lines { step: of, .. } = &ask.place else {
                continue;
            };
            if of.as_ref() != step || ask.messages.is_empty() {
                continue;
            }
            match places.iter_mut().find(|(place, _)| place == &ask.place) {
                Some((_, count)) => *count += 1,
                None => places.push((ask.place.clone(), 1)),
            }
        }
        places
    }

    /// The selected lines with Asks on a screen, which get a marker under their last row.
    pub fn asked_lines_on(&self, screen: &Screen) -> Vec<Place> {
        let step = match screen {
            Screen::Step(id) => Some(id),
            Screen::SafetyNet => None,
            Screen::Summary => return Vec::new(),
        };
        self.line_places(step)
            .into_iter()
            .map(|(place, _)| place)
            .collect()
    }

    /// The review was re-pinned or reopened: notes on old answers follow it.
    pub fn recheck_code_changes(&mut self, review: &Review, cx: &mut Context<Self>) {
        self.code_changed = self
            .setup
            .asks
            .all()
            .iter()
            .filter(|ask| review.code_changed_since(ask))
            .map(|ask| ask.id)
            .collect();
        cx.notify();
    }

    /// Shows the Asks of a place and gets the AI ready: its process starts now, not at the first question.
    pub fn open(
        &mut self,
        review: &Review,
        place: Place,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recheck_code_changes(review, cx);
        self.notice = None;
        let target =
            review.ask_target(place, label, self.setup.clone.clone(), &self.setup.settings);
        if let AskTarget::Unavailable { .. } = target {
            self.selected = None;
            self.target = Some(target);
            window.focus(&self.focus, cx);
            return cx.notify();
        }
        let place = target.place();
        let reuses_draft = self.selected.is_some_and(|id| {
            self.setup
                .asks
                .get(id)
                .is_some_and(|ask| &ask.place == place && ask.messages.is_empty())
        });
        if !reuses_draft {
            self.selected = Some(self.setup.asks.start(place.clone(), review.head()));
        }
        self.target = Some(target);
        self.question.clear();
        if let Some(id) = self.selected {
            self.ensure_worker(id);
        }
        window.focus(&self.question_focus, cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.target = None;
        window.focus(&self.layout_focus, cx);
        cx.notify();
    }

    /// Gets the Ask a live process, stopping the one used longest ago when three are live.
    /// False when all three are answering.
    fn ensure_worker(&mut self, id: AskId) -> bool {
        let Some(AskTarget::Ready { launch, .. }) = &self.target else {
            return false;
        };
        let launch = self.setup.asks.launch_for(id, launch.clone());
        match self.pool.open(id, Instant::now()) {
            Opening::Live if self.workers.contains_key(&id) => return true,
            Opening::Live => {}
            Opening::Start { stop } => {
                if let Some(stopped) = stop {
                    self.workers.remove(&stopped);
                }
            }
            Opening::AllAnswering => return false,
        }
        let worker = Worker::spawn(
            Arc::clone(&self.setup.launcher),
            launch,
            id,
            self.from_workers.clone(),
        );
        self.workers.insert(id, worker);
        true
    }

    fn stop_idle(&mut self) {
        for id in self.pool.stop_idle(Instant::now()) {
            self.workers.remove(&id);
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let question = self.question.trim().to_owned();
        let (Some(id), Some(AskTarget::Ready { context, .. })) = (self.selected, &self.target)
        else {
            return;
        };
        if question.is_empty() || self.pool.is_answering(id) {
            return;
        }
        let context = context.clone();
        if !self.ensure_worker(id) {
            self.notice = Some(AskNotice::AllAnswering);
            return cx.notify();
        }
        self.notice = None;
        let message = self.setup.asks.begin_turn(id, &question, &context);
        self.question.clear();
        let delivered = self
            .workers
            .get(&id)
            .is_some_and(|worker| worker.questions.send(message.clone()).is_ok());
        if !delivered {
            self.workers.remove(&id);
            self.pool.ended(id);
            let resent = self.ensure_worker(id)
                && self
                    .workers
                    .get(&id)
                    .is_some_and(|worker| worker.questions.send(message).is_ok());
            if !resent {
                self.setup.asks.apply(
                    id,
                    crate::viewer::AgentEvent::Error(QUESTION_NOT_SENT.to_owned()),
                );
                return cx.notify();
            }
        }
        self.pool.turn_started(id, Instant::now());
        cx.notify();
    }

    fn stop_turn(&mut self, id: AskId, cx: &mut Context<Self>) {
        if let Some(stopper) = self.workers.get(&id).and_then(Worker::stopper) {
            self.setup.asks.stop_turn(id, stopper.as_ref());
        }
        cx.notify();
    }

    fn on_worker_message(&mut self, id: AskId, message: WorkerMessage, cx: &mut Context<Self>) {
        match message {
            WorkerMessage::Line(line) => self.setup.asks.apply_line(id, &line),
            WorkerMessage::TurnEnded => self.pool.turn_ended(id, Instant::now()),
            WorkerMessage::Failed(reason) => {
                self.pool.ended(id);
                self.workers.remove(&id);
                self.setup
                    .asks
                    .apply(id, crate::viewer::AgentEvent::Error(reason));
            }
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.close(window, cx);
        }
    }

    fn question_box(&self, cx: &mut Context<Self>) -> TextBox {
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

    fn ask_block(&self, ask: &Ask, place: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let id = ask.id;
        let is_selected = self.selected == Some(id);
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
                    .child(format!("{activity}...")),
            );
        }
        if let Some(failure) = &ask.failure {
            block = block.child(div().text_color(colors.danger).child(failure.clone()));
        }
        if self.code_changed.contains(&id) {
            block = block.child(
                div()
                    .text_color(colors.warning)
                    .child("Code changed since: this answer may be old"),
            );
        }
        let is_answering = self.pool.is_answering(id);
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
                action("follow-up".to_owned(), Icon::Reply, "Follow up").on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.selected = Some(id);
                        this.question.clear();
                        this.notice = None;
                        this.ensure_worker(id);
                        cx.notify();
                    },
                )),
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
                    this.hand_offs.insert(id, "Handing to your agent...".into());
                    cx.emit(HandToAgent {
                        ask: ask_to_hand.clone(),
                        place: place.clone(),
                    });
                    cx.notify();
                })),
            );
        }
        let hand_off = self
            .hand_offs
            .get(&id)
            .map(|line| div().text_color(colors.muted_text).child(line.clone()));
        block.child(actions).children(hand_off)
    }

    /// The layout's word on an Ask it handed, or tried to hand, to the tour agent.
    pub fn hand_off_ended(&mut self, id: AskId, line: String, cx: &mut Context<Self>) {
        self.hand_offs.insert(id, line.into());
        cx.notify();
    }
}

const QUESTION_NOT_SENT: &str = "The AI stopped before it got the question, and three other Asks are answering: ask again when one is done";

impl EventEmitter<UseAsComment> for AskPanel {}
impl EventEmitter<HandToAgent> for AskPanel {}

fn id_label(id: AskId) -> String {
    serde_json::to_string(&id).unwrap_or_default()
}

impl Render for AskPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let Some(target) = &self.target else {
            return div().into_any_element();
        };
        let place = target.label().to_owned();
        let blocks: Vec<_> = self
            .setup
            .asks
            .at(target.place())
            .into_iter()
            .map(|ask| self.ask_block(ask, &place, cx).into_any_element())
            .collect();
        let is_answering = self.selected.is_some_and(|id| self.pool.is_answering(id));
        let input = match target {
            AskTarget::Unavailable { why, .. } => {
                div().text_color(colors.danger).child(why.message())
            }
            AskTarget::Ready { .. } => div()
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
                    self.notice
                        .map(|notice| div().text_color(colors.danger).child(notice.message())),
                )
                .child(
                    Field::new(&self.question_focus)
                        .px_2()
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .child(self.question_box(cx)),
                ),
        };
        let panel = div()
            .id("ask-panel")
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .w(px(380.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(colors.overlay)
            .border_l_1()
            .border_color(colors.border)
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
                    .bg(colors.header_band())
                    .border_b_1()
                    .border_color(colors.subtle_border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(format!("Ask: {place}")),
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
                    .children(blocks),
            )
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_t_1()
                    .border_color(colors.subtle_border)
                    .child(input),
            );
        slide_in(panel, "ask-panel-slide", OVERLAY, Edge::Right, PANEL_SLIDE).into_any_element()
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
