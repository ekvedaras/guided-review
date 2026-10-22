use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{
    Context, Div, ElementId, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div,
    px, red,
};

use crate::config::AskSettings;
use crate::viewer::{AgentLaunch, AgentLauncher, Ask, AskId, Asks, Author, Place, Review};

use super::palette::{Palette, TEXT_FONT};
use super::text_input::TextDraft;

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

/// One live `claude` process, driven from its own thread so the window never waits on it.
struct Worker {
    questions: mpsc::Sender<String>,
}

impl Worker {
    fn spawn(
        launcher: Arc<dyn AgentLauncher>,
        launch: AgentLaunch,
        id: AskId,
        into: UnboundedSender<(AskId, WorkerMessage)>,
    ) -> Self {
        let (questions, incoming) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut agent = match launcher.start(&launch) {
                Ok(agent) => agent,
                Err(error) => {
                    let _ = into.unbounded_send((id, WorkerMessage::Failed(format!("{error:#}"))));
                    return;
                }
            };
            while let Ok(message) = incoming.recv() {
                let result = agent.ask(&message, &mut |line| {
                    let _ = into.unbounded_send((id, WorkerMessage::Line(line.to_string())));
                });
                match result {
                    Ok(()) => {
                        let _ = into.unbounded_send((id, WorkerMessage::TurnEnded));
                    }
                    Err(error) => {
                        let _ =
                            into.unbounded_send((id, WorkerMessage::Failed(format!("{error:#}"))));
                        return;
                    }
                }
            }
        });
        Self { questions }
    }
}

struct Target {
    place: Place,
    label: String,
    context: String,
    launch: AgentLaunch,
}

/// The right side panel: Asks about one place, newest first, and the box to ask the next one.
pub struct AskPanel {
    setup: AskSetup,
    target: Option<Target>,
    selected: Option<AskId>,
    draft: TextDraft,
    busy: HashSet<AskId>,
    workers: HashMap<AskId, Worker>,
    from_workers: UnboundedSender<(AskId, WorkerMessage)>,
    focus: FocusHandle,
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
        Self {
            setup,
            target: None,
            selected: None,
            draft: TextDraft::default(),
            busy: HashSet::new(),
            workers: HashMap::new(),
            from_workers,
            focus: cx.focus_handle(),
            layout_focus,
        }
    }

    pub fn is_open(&self) -> bool {
        self.target.is_some()
    }

    pub fn count_at(&self, place: &Place) -> usize {
        self.setup.asks.count_at(place)
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
        let (Some(context), Some(clone)) = (review.ask_context(&place), self.setup.clone.clone())
        else {
            return self.show_unavailable(place, label, window, cx);
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
        });
        self.draft.clear();
        if let Some(id) = self.selected {
            self.ensure_worker(id);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn show_unavailable(
        &mut self,
        place: Place,
        label: String,
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
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.target = None;
        window.focus(&self.layout_focus, cx);
        cx.notify();
    }

    fn ensure_worker(&mut self, id: AskId) {
        if self.workers.contains_key(&id) {
            return;
        }
        let Some(target) = &self.target else { return };
        let mut launch = target.launch.clone();
        launch.resume = self
            .setup
            .asks
            .get(id)
            .and_then(|ask| ask.session_id.clone());
        let worker = Worker::spawn(
            self.setup.launcher.clone(),
            launch,
            id,
            self.from_workers.clone(),
        );
        self.workers.insert(id, worker);
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let question = self.draft.text().trim().to_string();
        let (Some(id), Some(target)) = (self.selected, &self.target) else {
            return;
        };
        if question.is_empty() || self.busy.contains(&id) {
            return;
        }
        let message = self.setup.asks.begin_turn(id, &question, &target.context);
        self.draft.clear();
        self.ensure_worker(id);
        let delivered = self
            .workers
            .get(&id)
            .is_some_and(|worker| worker.questions.send(message.clone()).is_ok());
        if !delivered {
            self.workers.remove(&id);
            self.ensure_worker(id);
            if let Some(worker) = self.workers.get(&id) {
                let _ = worker.questions.send(message);
            }
        }
        self.busy.insert(id);
        cx.notify();
    }

    fn on_worker_message(&mut self, id: AskId, message: WorkerMessage, cx: &mut Context<Self>) {
        match message {
            WorkerMessage::Line(line) => self.setup.asks.apply_line(id, &line),
            WorkerMessage::TurnEnded => {
                self.busy.remove(&id);
            }
            WorkerMessage::Failed(reason) => {
                self.busy.remove(&id);
                self.workers.remove(&id);
                self.setup
                    .asks
                    .apply(id, crate::viewer::AgentEvent::Error(reason));
            }
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        match keystroke.key.as_str() {
            "enter" => self.send(cx),
            "escape" => self.close(window, cx),
            _ => {
                self.draft.edit(keystroke);
            }
        }
        cx.notify();
    }

    fn ask_block(&self, ask: &Ask, cx: &mut Context<Self>) -> impl IntoElement {
        let id = ask.id;
        let is_selected = self.selected == Some(id);
        let messages = ask.messages.iter().map(|message| {
            let text = div().flex().flex_col().children(
                message
                    .text
                    .lines()
                    .map(|line| div().min_h(px(16.)).child(line.to_string()))
                    .collect::<Vec<_>>(),
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
                Palette::text()
            } else {
                Palette::border()
            })
            .rounded_md()
            .children(messages);
        if let Some(activity) = &ask.activity {
            block = block.child(
                div()
                    .text_color(Palette::muted_text())
                    .child(format!("{activity}...")),
            );
        }
        if let Some(failure) = &ask.failure {
            block = block.child(div().text_color(red()).child(failure.clone()));
        }
        if !is_selected {
            block = block.child(
                div()
                    .id(SharedString::from(format!("follow-up-{}", id_label(id))))
                    .cursor_pointer()
                    .text_color(Palette::muted_text())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(id);
                        this.draft.clear();
                        this.ensure_worker(id);
                        cx.notify();
                    }))
                    .child("Follow up"),
            );
        }
        block
    }
}

fn id_label(id: AskId) -> String {
    serde_json::to_string(&id).unwrap_or_default()
}

impl Render for AskPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let input = match self.selected {
            Some(id) if self.busy.contains(&id) => div()
                .text_color(Palette::muted_text())
                .child("Waiting for the answer..."),
            Some(_) => div().child(self.draft.shown()),
            None => div().text_color(red()).child(
                "Ask needs a local clone of this repo: run the app inside it or add it under [clones] in ~/.guided-review/config.toml",
            ),
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
            .bg(Palette::sidebar())
            .border_l_1()
            .border_color(Palette::border())
            .font_family(TEXT_FONT)
            .text_sm()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .p_2()
                    .border_b_1()
                    .border_color(Palette::border())
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
                    .border_color(Palette::border())
                    .child(input),
            )
            .into_any_element()
    }
}

/// The button on a place. It shows how many Asks the place already has.
pub fn ask_button(id: impl Into<ElementId>, count: usize) -> Stateful<Div> {
    let label = if count == 0 {
        "Ask".to_string()
    } else {
        format!("Ask \u{25cf} {count}")
    };
    div()
        .id(id)
        .px_2()
        .py_1()
        .cursor_pointer()
        .text_sm()
        .text_color(Palette::muted_text())
        .border_1()
        .border_color(Palette::border())
        .rounded_md()
        .child(label)
}
