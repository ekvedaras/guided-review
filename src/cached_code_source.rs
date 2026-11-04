use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use crate::code_source::{BlobInfo, CodeSource, FileAt, FileDiff};
use crate::pull_request::Sha;

const PARALLEL_READS: usize = 8;

/// Reads each file once per process: a file at a commit never changes.
/// Failed reads are not kept, so a passing network error does not stick
/// and the caller always gets the real error.
pub struct CachedCodeSource {
    source: Arc<dyn CodeSource>,
    files: Mutex<HashMap<FileAt, Arc<Vec<u8>>>>,
}

impl CachedCodeSource {
    pub fn wrapping(source: Arc<dyn CodeSource>) -> Self {
        Self {
            source,
            files: Mutex::new(HashMap::new()),
        }
    }

    /// A reader that panicked cannot leave a half-written entry, so a poisoned lock is still safe to use.
    fn files(&self) -> MutexGuard<'_, HashMap<FileAt, Arc<Vec<u8>>>> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn already_read(&self, file: &FileAt) -> Option<Arc<Vec<u8>>> {
        self.files().get(file).cloned()
    }

    fn read(&self, file: FileAt) -> anyhow::Result<Arc<Vec<u8>>> {
        if let Some(bytes) = self.already_read(&file) {
            return Ok(bytes);
        }
        let bytes = Arc::new(self.source.blob_at(&file.sha, &file.path)?);
        self.files().insert(file, Arc::clone(&bytes));
        Ok(bytes)
    }
}

impl CodeSource for CachedCodeSource {
    fn file_at(&self, sha: &Sha, path: &str) -> anyhow::Result<String> {
        Ok(String::from_utf8_lossy(&self.blob_at(sha, path)?).into_owned())
    }

    fn blob_at(&self, sha: &Sha, path: &str) -> anyhow::Result<Vec<u8>> {
        let file = FileAt {
            sha: sha.clone(),
            path: path.to_string(),
        };
        Ok(self.read(file)?.as_ref().clone())
    }

    fn blob_info_at(&self, sha: &Sha, path: &str) -> anyhow::Result<BlobInfo> {
        self.source.blob_info_at(sha, path)
    }

    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        self.source.diff(base, head)
    }

    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        self.source.merge_base(a, b)
    }

    fn working_tree_is_at(&self, head: &Sha) -> bool {
        self.source.working_tree_is_at(head)
    }

    fn local_clone(&self) -> Option<&Path> {
        self.source.local_clone()
    }

    fn read_ahead(&self, files: &[FileAt]) {
        let missing: Vec<&FileAt> = files
            .iter()
            .filter(|file| self.already_read(file).is_none())
            .collect();
        let next = AtomicUsize::new(0);
        thread::scope(|scope| {
            for _ in 0..PARALLEL_READS.min(missing.len()) {
                scope.spawn(|| {
                    while let Some(file) = missing.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let _ = self.read((*file).clone());
                    }
                });
            }
        });
    }
}
