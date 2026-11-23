use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use guided_review::pull_request::Sha;
use guided_review::viewer::CheckoutPort;

/// Folders with a git checkout at a commit, clean or not. Any other folder is no checkout.
#[derive(Clone, Default)]
pub struct FakeCheckouts {
    standing: Arc<Mutex<HashMap<PathBuf, (String, bool)>>>,
}

impl FakeCheckouts {
    pub fn clean_at(self, folder: &str, head: &str) -> Self {
        self.put(folder, head, false)
    }

    pub fn dirty_at(self, folder: &str, head: &str) -> Self {
        self.put(folder, head, true)
    }

    fn put(self, folder: &str, head: &str, is_dirty: bool) -> Self {
        self.standing
            .lock()
            .unwrap()
            .insert(PathBuf::from(folder), (head.to_owned(), is_dirty));
        self
    }

    fn state_of(&self, folder: &Path) -> anyhow::Result<(String, bool)> {
        self.standing
            .lock()
            .unwrap()
            .get(folder)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{} is not a git repository", folder.display()))
    }
}

impl CheckoutPort for FakeCheckouts {
    fn head_of(&self, folder: &Path) -> anyhow::Result<Sha> {
        Ok(Sha::new(&self.state_of(folder)?.0))
    }

    fn has_uncommitted_changes(&self, folder: &Path) -> anyhow::Result<bool> {
        Ok(self.state_of(folder)?.1)
    }
}
