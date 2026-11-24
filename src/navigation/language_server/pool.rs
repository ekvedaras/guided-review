//! The servers every review shares: one per (codebase, server), started on the first question and
//! stopped when idle, by a no for its codebase or at quit, and the server checkout of each codebase,
//! moved to the head a question is asked at.

use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};
use std::time::Duration;

use crate::codebase::Codebase;
use crate::language::Language;
use crate::pull_request::Sha;

use super::notices::Notices;
use super::rpc::locked;
use super::running::Running;
use super::{
    Checkout, FallbackReason, ServerAnswer, ServerCatalog, ServerCheckouts, ServerLaunch,
    ServerName, ServerOptIns, Timeouts, Waiting,
};

type Slot = Arc<Mutex<Option<Arc<Running>>>>;

/// A codebase's server checkout as last moved, and the clone it was filled from.
struct Placed {
    clone: PathBuf,
    checkout: Checkout,
}

impl Placed {
    fn is(&self, clone: &Path, head: &Sha) -> bool {
        self.clone == clone && self.checkout.head == *head
    }
}

/// Where one codebase's server checkout is. Shared by the questions asked at its head, held alone by
/// a move.
#[derive(Default)]
struct CheckoutPlace(RwLock<Option<Placed>>);

impl CheckoutPlace {
    /// Runs `ask` with the checkout at `head` of `clone`, moving it there first. A move waits for the
    /// questions at the old head, so no server is asked about files that changed under it. `Err` is
    /// the move's failure.
    fn at<R>(
        &self,
        clone: &Path,
        head: &Sha,
        move_there: impl Fn() -> anyhow::Result<Checkout>,
        ask: impl FnOnce(&Checkout) -> R,
    ) -> anyhow::Result<R> {
        loop {
            {
                let placed = self.0.read().unwrap_or_else(PoisonError::into_inner);
                if let Some(placed) = placed.as_ref().filter(|placed| placed.is(clone, head)) {
                    return Ok(ask(&placed.checkout));
                }
            }
            let mut placed = self.0.write().unwrap_or_else(PoisonError::into_inner);
            if placed.as_ref().is_some_and(|placed| placed.is(clone, head)) {
                continue;
            }
            *placed = None;
            *placed = Some(Placed {
                clone: clone.to_path_buf(),
                checkout: move_there()?,
            });
        }
    }
}

/// A server of the pool, which a question that finds it broken takes out of the pool.
pub(super) struct PooledServer {
    slot: Slot,
    server: Arc<Running>,
}

impl PooledServer {
    /// The next question starts a new server; this one dies with its last user.
    pub(super) fn forget(&self) {
        let mut held = locked(&self.slot);
        if held
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, &self.server))
        {
            *held = None;
        }
    }

    pub(super) fn forget_and_stop(&self) {
        self.forget();
        self.server.stop();
    }
}

impl Deref for PooledServer {
    type Target = Running;

    fn deref(&self) -> &Running {
        &self.server
    }
}

pub(super) struct Pool {
    catalog: Arc<dyn ServerCatalog>,
    checkouts: ServerCheckouts,
    timeouts: Timeouts,
    notices: Arc<Notices>,
    /// One per (codebase, server): the server is reused by every review of the codebase.
    slots: Mutex<HashMap<(Codebase, String), Slot>>,
    places: Mutex<HashMap<Codebase, Arc<CheckoutPlace>>>,
    answers: Mutex<HashMap<Codebase, ServerAnswer>>,
}

impl ServerOptIns for Pool {
    fn answer(&self, codebase: &Codebase) -> Option<ServerAnswer> {
        locked(&self.answers).get(codebase).copied()
    }

    fn answered(&self, codebase: &Codebase, answer: ServerAnswer) {
        locked(&self.answers).insert(codebase.clone(), answer);
        if answer == ServerAnswer::No {
            self.stop_in(|stopping| stopping == codebase, |_| true);
        }
    }
}

impl Pool {
    /// The pool, with a thread that stops the servers idle for `timeouts.idle` until the pool is gone.
    pub(super) fn new(
        catalog: Arc<dyn ServerCatalog>,
        checkouts: ServerCheckouts,
        timeouts: Timeouts,
        notices: Notices,
    ) -> Arc<Self> {
        let pool = Arc::new(Self {
            catalog,
            checkouts,
            timeouts,
            notices: Arc::new(notices),
            slots: Mutex::default(),
            places: Mutex::default(),
            answers: Mutex::default(),
        });
        let watched: Weak<Self> = Arc::downgrade(&pool);
        let idle = timeouts.idle;
        let tick = (idle / 4).clamp(Duration::from_millis(50), Duration::from_secs(5));
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(tick);
                let Some(pool) = watched.upgrade() else {
                    return;
                };
                pool.stop_where(|server| server.idle_for() >= idle);
            }
        });
        pool
    }

    pub(super) fn launch_for(
        &self,
        language: Language,
        codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        self.catalog.launch_for(language, codebase, waiting)
    }

    /// Runs `ask` with the codebase's server checkout at `head`, filled from `clone`. `Err` says why
    /// the checkout could not be moved there.
    pub(super) fn at_head<R>(
        &self,
        codebase: &Codebase,
        clone: &Path,
        head: &Sha,
        ask: impl FnOnce(&Checkout) -> R,
    ) -> anyhow::Result<R> {
        let place = Arc::clone(locked(&self.places).entry(codebase.clone()).or_default());
        place.at(
            clone,
            head,
            || self.checkouts.moved_to(clone, codebase, head),
            ask,
        )
    }

    /// The codebase's running server of `launch`, caught up with `checkout`, or a new one started in it.
    pub(super) fn server(
        &self,
        codebase: &Codebase,
        launch: &ServerLaunch,
        checkout: &Checkout,
        waiting: &dyn Fn(Waiting),
    ) -> Result<PooledServer, String> {
        let slot = self.slot(codebase, &launch.name);
        let mut held = locked(&slot);
        if let Some(server) = held.as_ref().filter(|server| server.is_alive()) {
            if server.catch_up(checkout).is_ok() {
                let server = Arc::clone(server);
                drop(held);
                return Ok(PooledServer { slot, server });
            }
            // A head the checkout can no longer compare leaves no way to tell the server what changed.
            server.stop();
        }
        waiting(Waiting::Starting(launch.name.clone()));
        let server = Arc::new(Running::start(
            launch,
            checkout,
            self.timeouts,
            &self.notices,
        )?);
        *held = Some(Arc::clone(&server));
        drop(held);
        Ok(PooledServer { slot, server })
    }

    fn slot(&self, codebase: &Codebase, name: &ServerName) -> Slot {
        let mut slots = locked(&self.slots);
        Arc::clone(
            slots
                .entry((codebase.clone(), name.to_string()))
                .or_default(),
        )
    }

    pub(super) fn stop_all(&self) {
        self.stop_where(|_| true);
    }

    fn stop_where(&self, stop_it: impl Fn(&Running) -> bool) {
        self.stop_in(|_| true, stop_it);
    }

    fn stop_in(&self, in_codebase: impl Fn(&Codebase) -> bool, stop_it: impl Fn(&Running) -> bool) {
        let slots: Vec<Slot> = locked(&self.slots)
            .iter()
            .filter(|((codebase, _), _)| in_codebase(codebase))
            .map(|(_, slot)| Arc::clone(slot))
            .collect();
        for slot in slots {
            let mut running = locked(&slot);
            if running.as_ref().is_some_and(|server| stop_it(server))
                && let Some(server) = running.take()
            {
                server.stop();
            }
        }
    }
}
