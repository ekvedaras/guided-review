//! What servers ask the reviewer to be shown, told to the port once per run of the app.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::rpc::locked;
use crate::navigation::{ServerMessage, ServerMessages};

pub(super) struct Notices {
    port: Arc<dyn ServerMessages>,
    /// (server, text) pairs already told. A restarted server says its notices again, and a notice is
    /// the same text each time.
    told: Mutex<HashSet<(String, String)>>,
}

impl Notices {
    pub(super) fn new(port: Arc<dyn ServerMessages>) -> Self {
        Self {
            port,
            told: Mutex::default(),
        }
    }

    pub(super) fn show(&self, message: ServerMessage) {
        let key = (message.server.to_string(), message.text.clone());
        if locked(&self.told).insert(key) {
            self.port.show(message);
        }
    }
}
