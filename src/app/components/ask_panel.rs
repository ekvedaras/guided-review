use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{
    Context, Div, ElementId, EventEmitter, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement,
    Styled, Window, div, px, red,
};

use crate::config::AskSettings;
use crate::viewer::{
    AgentLaunch, AgentLauncher, AgentPool, Ask, AskId, Asks, Author, Opening, Place, Review,
    Screen, StepId, TurnStopper,
};

use super::palette::{Colors, TEXT_FONT};
use super::text_box::TextBox;
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

struct Target {
    place: Place,
    label: String,
    context: String,
    launch: AgentLaunch,
    /// Why no new question can be asked here.
    unavailable: Option<&'static str>,
}

/// The right side panel: Asks about one place, newest first, and the box to ask the next one.
pub struct AskPanel {
    setup: AskSetup,
    target: Option<Target>,
    selected: Option<AskId>,
    question: String,
    pool: AgentPool,
    workers: HashMap<AskId, Worker>,
    /// Asks a re-pin changed the code under, as of the review last shown.
    code_changed: HashSet<AskId>,
    /// Why the last question did not go out.
    notice: Option<&'static str>,
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
            from_workers,
            focus: cx.focus_handle(),
            question_focus: cx.focus_handle(),
            layout_focus,
        }
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
        let Some(clone) = self.setup.clone.clone() else {
            return self.show_unavailable(place, label, NO_CLONE, window, cx);
        };
        let Some(context) = review.ask_context(&place) else {
            return self.show_unavailable(place, label, PLACE_GONE, window, cx);
        };
        let launch = review.agent_launch(clone, &self.setup.settings, None);
        let reuses_draft = self.selected.is_some_and(|id| {
            self.setup
                .asks
                .get(id)
                .is_some_and(|ask| ask.place == place && ask.messages.is_empty())
        });
        if !reuses_draft {
            self.selected = Some(self.setup.asks.start(place.clone(), review.head()));
        }
        self.target = Some(Target {
            place,
            label,
            context,
            launch,
            unavailable: None,
        });
        self.question.clear();
        if let Some(id) = self.selected {
            self.ensure_worker(id);
        }
        window.focus(&self.question_focus, cx);
        cx.notify();
    }

    fn show_unavailable(
        &mut self,
        place: Place,
        label: String,
        reason: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = None;
        self.target = Some(Target {
            place,
            label,
            context: String::new(),
            launch: AgentLaunch {
                cwd: PathBuf::new(),
                model: String::new(),
                effort: String::new(),
                system_prompt: String::new(),
                resume: None,
            },
            unavailable: Some(reason),
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.target = None;
        window.focus(&self.layout_focus, cx);
        cx.notify();
    }

    /// Gets the Ask a live process, stopping the one used longest ago when three are live.
    /// False when all three are answering.
    fn ensure_worker(&mut self, id: AskId) -> bool {
        let Some(target) = &self.target else {
            return false;
        };
        let launch = self.setup.asks.launch_for(id, target.launch.clone());
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
        let (Some(id), Some(target)) = (self.selected, &self.target) else {
            return;
        };
        if question.is_empty() || self.pool.is_answering(id) {
            return;
        }
        let context = target.context.clone();
        if !self.ensure_worker(id) {
            self.notice = Some(ALL_ANSWERING);
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

    fn ask_block(&self, ask: &Ask, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let id = ask.id;
        let is_selected = self.selected == Some(id);
        let messages = ask.messages.iter().map(|message| {
            let text = div().flex().flex_col().children(
                message
                    .text
                    .lines()
                    .map(|line| div().min_h(px(16.)).child(line.to_owned())),
            );
            match message.author {
                Author::Reviewer => div().font_weight(gpui::FontWeight::BOLD).child(text),
                Author::Ai => div().child(text),
            }
        });
        let mut block = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(if is_selected {
                colors.text
            } else {
                colors.border
            })
            .rounded_md()
            .children(messages);
        if let Some(activity) = &ask.activity {
            block = block.child(
                div()
                    .text_color(colors.muted_text)
                    .child(format!("{activity}...")),
            );
        }
        if let Some(failure) = &ask.failure {
            block = block.child(div().text_color(red()).child(failure.clone()));
        }
        if self.code_changed.contains(&id) {
            block = block.child(
                div()
                    .text_color(colors.muted_text)
                    .child("Code changed since: this answer may be old"),
            );
        }
        let is_answering = self.pool.is_answering(id);
        let mut actions = div().flex().gap_3().text_color(colors.muted_text);
        if is_answering {
            actions = actions.child(
                div()
                    .id(SharedString::from(format!("stop-turn-{}", id_label(id))))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.stop_turn(id, cx)))
                    .child("Stop"),
            );
        }
        let has_lines = matches!(ask.place, Place::Lines { .. } | Place::Piece { .. });
        if has_lines && !is_answering && ask.latest_answer().is_some() {
            let ask_to_use = ask.clone();
            actions = actions.child(
                div()
                    .id(SharedString::from(format!(
                        "use-as-comment-{}",
                        id_label(id)
                    )))
                    .cursor_pointer()
                    .on_click(
                        cx.listener(move |_, _, _, cx| cx.emit(UseAsComment(ask_to_use.clone()))),
                    )
                    .child("Use as comment"),
            );
        }
        if !is_selected {
            actions = actions.child(
                div()
                    .id(SharedString::from(format!("follow-up-{}", id_label(id))))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(id);
                        this.question.clear();
                        this.notice = None;
                        this.ensure_worker(id);
                        cx.notify();
                    }))
                    .child("Follow up"),
            );
        }
        block.child(actions)
    }
}

const NO_CLONE: &str = "Ask needs a local clone of this repo: run the app inside it or add it under [clones] in ~/.guided-review/config.toml";
const PLACE_GONE: &str = "These lines are not in the tour any more: select them again to ask";
const ALL_ANSWERING: &str = "Three Asks are answering now: wait for one, or stop it";
const QUESTION_NOT_SENT: &str = "The AI stopped before it got the question, and three other Asks are answering: ask again when one is done";

impl EventEmitter<UseAsComment> for AskPanel {}

fn id_label(id: AskId) -> String {
    serde_json::to_string(&id).unwrap_or_default()
}

impl Render for AskPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        let Some(target) = &self.target else {
            return div().into_any_element();
        };
        let place = target.label.clone();
        let blocks: Vec<_> = self
            .setup
            .asks
            .at(&target.place)
            .into_iter()
            .map(|ask| self.ask_block(ask, cx).into_any_element())
            .collect();
        let is_answering = self.selected.is_some_and(|id| self.pool.is_answering(id));
        let input = match target.unavailable {
            Some(reason) => div().text_color(red()).child(reason),
            None => div()
                .flex()
                .flex_col()
                .children(is_answering.then(|| {
                    div()
                        .text_color(colors.muted_text)
                        .child("Waiting for the answer...")
                }))
                .children(
                    self.notice
                        .map(|notice| div().text_color(red()).child(notice)),
                )
                .child(self.question_box(cx)),
        };
        div()
            .id("ask-panel")
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .w(px(380.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(colors.surface)
            .border_l_1()
            .border_color(colors.border)
            .font_family(TEXT_FONT)
            .text_sm()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .p_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(div().child(format!("Ask: {place}")))
                    .child(
                        div()
                            .id("ask-close")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
                            .child("Close"),
                    ),
            )
            .child(
                div()
                    .id("ask-answers")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .children(blocks),
            )
            .child(
                div()
                    .p_2()
                    .border_t_1()
                    .border_color(colors.border)
                    .child(input),
            )
            .into_any_element()
    }
}

/// The button on a place. It shows how many Asks the place already has.
pub fn ask_button(colors: &Colors, id: impl Into<ElementId>, count: usize) -> Stateful<Div> {
    let label = if count == 0 {
        "Ask".to_owned()
    } else {
        format!("Ask \u{25cf} {count}")
    };
    super::button::button(
        colors,
        id,
        super::button::ButtonKind::Secondary,
        super::button::ButtonSize::Small,
    )
    .child(label)
}
