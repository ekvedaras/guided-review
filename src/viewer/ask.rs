use crate::pull_request::Sha;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::StepId;
use super::agent_event::AgentEvent;
use super::ports::{AgentLaunch, AgentPort, TurnStopper};
use super::step_page::LineLocation;

/// The place in a tour an Ask is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Place {
    Tour,
    Step {
        step: StepId,
    },
    /// All lines of one piece, as shown in `step`.
    Piece {
        step: StepId,
        piece: String,
    },
    /// Rows the reviewer selected, top to bottom, in a step or (`step: None`) the safety net.
    Lines {
        step: Option<StepId>,
        start: LineLocation,
        end: LineLocation,
        /// The pieces the rows belong to, to tell when a re-pin changed them.
        pieces: Vec<String>,
    },
}

impl Place {
    pub fn lines_label(&self) -> Option<String> {
        let Self::Lines { start, end, .. } = self else {
            return None;
        };
        Some(if start == end {
            format!("{} line {}", end.path, end.line)
        } else {
            format!("{} lines {} to {}", end.path, start.line, end.line)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AskId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    Reviewer,
    Ai,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub author: Author,
    pub text: String,
    /// The reviewer stopped the AI before it finished this answer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
}

/// A chat with the AI about one place. Pinned to the tour head it was asked at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub id: AskId,
    pub place: Place,
    pub head: Sha,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub messages: Vec<Message>,
    /// What the AI is doing now ("Read app/Order.php"). Only lives while a turn runs.
    #[serde(skip)]
    pub activity: Option<String>,
    #[serde(skip)]
    pub failure: Option<String>,
}

impl Ask {
    /// The AI's last answer, the one "Use as comment" takes.
    pub fn latest_answer(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find(|message| message.author == Author::Ai && !message.text.is_empty())
            .map(|message| message.text.as_str())
    }
}

/// Where Asks are kept between runs. The per-PR state file implements it; the viewer never
/// learns the file format.
pub trait AskStore: Send + Sync {
    fn load(&self) -> anyhow::Result<Vec<Ask>>;
    fn save(&self, asks: &[Ask]) -> anyhow::Result<()>;
}

/// Every Ask of one PR, newest first.
pub struct Asks {
    asks: Vec<Ask>,
    store: Arc<dyn AskStore>,
}

impl Asks {
    pub fn load(store: Arc<dyn AskStore>) -> anyhow::Result<Self> {
        Ok(Self {
            asks: store.load()?,
            store,
        })
    }

    /// A new, empty Ask. It is kept only once it holds a question.
    pub fn start(&mut self, place: Place, head: &Sha) -> AskId {
        let id = AskId(self.asks.iter().map(|ask| ask.id.0 + 1).max().unwrap_or(1));
        self.asks.insert(
            0,
            Ask {
                id,
                place,
                head: head.clone(),
                session_id: None,
                messages: Vec::new(),
                activity: None,
                failure: None,
            },
        );
        id
    }

    pub fn all(&self) -> &[Ask] {
        &self.asks
    }

    pub fn get(&self, id: AskId) -> Option<&Ask> {
        self.asks.iter().find(|ask| ask.id == id)
    }

    /// Asks about this place, newest first. Empty drafts are left out.
    pub fn at(&self, place: &Place) -> Vec<&Ask> {
        self.asks
            .iter()
            .filter(|ask| &ask.place == place && !ask.messages.is_empty())
            .collect()
    }

    /// What the marker on a place shows.
    pub fn count_at(&self, place: &Place) -> usize {
        self.at(place).len()
    }

    /// Records the question and returns the text to send to the AI: the first question of an
    /// Ask carries the place's context, follow-ups are sent as they are.
    pub fn begin_turn(&mut self, id: AskId, question: &str, context: &str) -> String {
        let Some(ask) = self.ask_mut(id) else {
            return question.to_owned();
        };
        let is_first = ask.messages.is_empty();
        ask.failure = None;
        ask.messages.push(Message {
            author: Author::Reviewer,
            text: question.to_owned(),
            stopped: false,
        });
        ask.messages.push(Message {
            author: Author::Ai,
            text: String::new(),
            stopped: false,
        });
        let message = if is_first && ask.session_id.is_none() {
            format!("{context}\n\nQuestion: {question}")
        } else {
            question.to_owned()
        };
        self.save(id);
        message
    }

    /// How to start the AI for this Ask: an Ask answered before continues its session.
    pub fn launch_for(&self, id: AskId, launch: AgentLaunch) -> AgentLaunch {
        AgentLaunch {
            resume: self.get(id).and_then(|ask| ask.session_id.clone()),
            ..launch
        }
    }

    /// The answer so far stays, marked as stopped; the AI ends the turn and keeps the session.
    pub fn stop_turn(&mut self, id: AskId, stopper: &dyn TurnStopper) {
        let Some(ask) = self.ask_mut(id) else { return };
        if let Some(answer) = ask.messages.last_mut().filter(|m| m.author == Author::Ai) {
            answer.stopped = true;
        }
        ask.activity = None;
        if let Err(error) = stopper.stop_turn() {
            ask.failure = Some(format!("could not stop the AI: {error:#}"));
        }
        self.save(id);
    }

    pub fn apply_line(&mut self, id: AskId, line: &str) {
        for event in AgentEvent::from_line(line) {
            self.apply(id, event);
        }
    }

    pub fn apply(&mut self, id: AskId, event: AgentEvent) {
        let Some(ask) = self.ask_mut(id) else {
            return;
        };
        let mut persist = false;
        match event {
            AgentEvent::TextDelta(text) => {
                let continues_after_tool = ask.activity.take().is_some();
                if let Some(answer) = ask.messages.last_mut().filter(|m| m.author == Author::Ai) {
                    if continues_after_tool && !answer.text.is_empty() {
                        answer.text.push_str("\n\n");
                    }
                    answer.text.push_str(&text);
                }
            }
            AgentEvent::ToolUse { summary } => ask.activity = Some(summary),
            AgentEvent::Done { session_id } => {
                ask.activity = None;
                if let Some(session_id) = session_id {
                    ask.session_id = Some(session_id);
                }
                persist = true;
            }
            AgentEvent::Error(reason) => {
                ask.activity = None;
                ask.failure = Some(reason);
                persist = true;
            }
        }
        if persist {
            self.save(id);
        }
    }

    /// Runs one whole turn against a live conversation.
    pub fn ask(
        &mut self,
        id: AskId,
        question: &str,
        context: &str,
        agent: &mut dyn AgentPort,
    ) -> anyhow::Result<()> {
        let message = self.begin_turn(id, question, context);
        agent.ask(&message, &mut |line| self.apply_line(id, line))
    }

    fn ask_mut(&mut self, id: AskId) -> Option<&mut Ask> {
        self.asks.iter_mut().find(|ask| ask.id == id)
    }

    fn save(&mut self, id: AskId) {
        let kept: Vec<Ask> = self
            .asks
            .iter()
            .filter(|ask| !ask.messages.is_empty())
            .cloned()
            .collect();
        if let Err(error) = self.store.save(&kept)
            && let Some(ask) = self.ask_mut(id)
        {
            ask.failure = Some(format!("could not save the Ask: {error:#}"));
        }
    }
}
