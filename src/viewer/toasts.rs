//! Short notes of outcomes that are otherwise silent or easy to miss: a comment added, settings
//! saved, a review sent, a failure. A note leaves by itself after a few seconds unless the pointer
//! rests on the stack; a failure stays until it is dismissed. Time is handed in, so tests need no clock.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastTone {
    Success,
    Neutral,
    /// Stays until dismissed.
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToastId(u64);

impl ToastId {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: ToastId,
    pub tone: ToastTone,
    pub text: String,
    /// How long it was on show before the pointer last held it.
    shown_for: Duration,
    /// Since when it counts down; `None` while held.
    counting_since: Option<Instant>,
}

impl Toast {
    /// `None` for a toast that does not leave by itself now: a failure, or one held by the pointer.
    fn time_left(&self, now: Instant) -> Option<Duration> {
        if self.tone == ToastTone::Danger {
            return None;
        }
        let since = self.counting_since?;
        let shown = self.shown_for + now.saturating_duration_since(since);
        Some(Toasts::LIFETIME.saturating_sub(shown))
    }
}

#[derive(Debug, Default)]
pub struct Toasts {
    toasts: VecDeque<Toast>,
    next_id: u64,
    is_held: bool,
}

impl Toasts {
    pub const LIFETIME: Duration = Duration::from_secs(4);
    pub const MAX_SHOWN: usize = 3;

    pub fn post(&mut self, tone: ToastTone, text: impl Into<String>, now: Instant) -> ToastId {
        if self.toasts.len() >= Self::MAX_SHOWN {
            let oldest = self
                .toasts
                .iter()
                .position(|toast| toast.tone != ToastTone::Danger)
                .unwrap_or(0);
            self.toasts.remove(oldest);
        }
        let id = ToastId(self.next_id);
        self.next_id += 1;
        self.toasts.push_back(Toast {
            id,
            tone,
            text: text.into(),
            shown_for: Duration::ZERO,
            counting_since: (!self.is_held).then_some(now),
        });
        id
    }

    /// Oldest first.
    pub fn shown(&self) -> impl Iterator<Item = &Toast> {
        self.toasts.iter()
    }

    /// Takes away the toasts whose time is up; true when any left.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.toasts.len();
        self.toasts
            .retain(|toast| toast.time_left(now).is_none_or(|left| !left.is_zero()));
        self.toasts.len() != before
    }

    /// How long until the next toast leaves by itself; `None` when none will.
    pub fn next_expiry(&self, now: Instant) -> Option<Duration> {
        self.toasts
            .iter()
            .filter_map(|toast| toast.time_left(now))
            .min()
    }

    /// The pointer came onto the stack: nothing leaves while it is there.
    pub fn pause(&mut self, now: Instant) {
        self.is_held = true;
        for toast in &mut self.toasts {
            if let Some(since) = toast.counting_since.take() {
                toast.shown_for += now.saturating_duration_since(since);
            }
        }
    }

    pub fn resume(&mut self, now: Instant) {
        self.is_held = false;
        for toast in &mut self.toasts {
            toast.counting_since.get_or_insert(now);
        }
    }

    pub fn dismiss(&mut self, id: ToastId) {
        self.toasts.retain(|toast| toast.id != id);
    }
}
