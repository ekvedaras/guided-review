use std::time::{Duration, Instant};

use super::ask::AskId;

/// Which Asks have a live AI process. Each process costs memory and plan limits, so only a few
/// stay live; a stopped Ask comes back with `--resume` on its next question. The shell owns the
/// processes and the clock and does what the pool decides.
#[derive(Debug)]
pub struct AgentPool {
    max_live: usize,
    idle_limit: Duration,
    /// Least recently used first.
    live: Vec<LiveAgent>,
}

#[derive(Debug)]
struct LiveAgent {
    ask: AskId,
    last_used: Instant,
    is_answering: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// The Ask has a process already.
    Live,
    /// Start a process; first stop the one of `stop`, to stay within the limit.
    Start { stop: Option<AskId> },
    /// Every live process is answering; the Ask waits until one is done.
    AllAnswering,
}

impl Default for AgentPool {
    fn default() -> Self {
        Self {
            max_live: 3,
            idle_limit: Duration::from_secs(10 * 60),
            live: Vec::new(),
        }
    }
}

impl AgentPool {
    pub fn open(&mut self, ask: AskId, now: Instant) -> Opening {
        if let Some(agent) = self.live.iter().find(|agent| agent.ask == ask) {
            self.touch(ask, now, agent.is_answering);
            return Opening::Live;
        }
        let mut stop = None;
        if self.live.len() >= self.max_live {
            let Some(oldest_idle) = self.live.iter().position(|agent| !agent.is_answering) else {
                return Opening::AllAnswering;
            };
            stop = Some(self.live.remove(oldest_idle).ask);
        }
        self.live.push(LiveAgent {
            ask,
            last_used: now,
            is_answering: false,
        });
        Opening::Start { stop }
    }

    pub fn is_live(&self, ask: AskId) -> bool {
        self.live.iter().any(|agent| agent.ask == ask)
    }

    pub fn is_answering(&self, ask: AskId) -> bool {
        self.live
            .iter()
            .any(|agent| agent.ask == ask && agent.is_answering)
    }

    pub fn turn_started(&mut self, ask: AskId, now: Instant) {
        self.touch(ask, now, true);
    }

    pub fn turn_ended(&mut self, ask: AskId, now: Instant) {
        self.touch(ask, now, false);
    }

    /// The process is gone: it failed, or the shell stopped it.
    pub fn ended(&mut self, ask: AskId) {
        self.live.retain(|agent| agent.ask != ask);
    }

    /// Takes out the processes nobody used for the idle limit and returns their Asks to stop.
    pub fn stop_idle(&mut self, now: Instant) -> Vec<AskId> {
        let is_idle = |agent: &LiveAgent| {
            !agent.is_answering && now.duration_since(agent.last_used) >= self.idle_limit
        };
        let idle = self
            .live
            .iter()
            .filter(|agent| is_idle(agent))
            .map(|agent| agent.ask)
            .collect();
        self.live.retain(|agent| !is_idle(agent));
        idle
    }

    fn touch(&mut self, ask: AskId, now: Instant, is_answering: bool) {
        let Some(at) = self.live.iter().position(|agent| agent.ask == ask) else {
            return;
        };
        let mut agent = self.live.remove(at);
        agent.last_used = now;
        agent.is_answering = is_answering;
        self.live.push(agent);
    }
}
