//! The reviewer's Cancel of a wait for the tour agent.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// A Cancel the waiting side looks at and the reviewer presses. Clones share it.
#[derive(Clone, Default)]
pub struct Cancel(Arc<(Mutex<bool>, Condvar)>);

/// The reviewer cancelled the refresh, which is no failure.
#[derive(Debug)]
pub struct RefreshCancelled;

impl fmt::Display for RefreshCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the refresh was cancelled")
    }
}

impl std::error::Error for RefreshCancelled {}

impl Cancel {
    pub fn cancel(&self) {
        *self.flag() = true;
        self.0.1.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        *self.flag()
    }

    /// Waits up to `time`, and ends the wait at once when cancelled.
    pub fn pause(&self, time: Duration) -> Result<(), RefreshCancelled> {
        let (cancelled, _) = self
            .0
            .1
            .wait_timeout_while(self.flag(), time, |cancelled| !*cancelled)
            .unwrap_or_else(PoisonError::into_inner);
        if *cancelled {
            Err(RefreshCancelled)
        } else {
            Ok(())
        }
    }

    fn flag(&self) -> MutexGuard<'_, bool> {
        self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn check(&self) -> Result<(), RefreshCancelled> {
        self.pause(Duration::ZERO)
    }
}
