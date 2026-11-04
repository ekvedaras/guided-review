use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use crate::code_source::{BlobInfo, CodeSource, FileAt, FileDiff};
use crate::file_path::FilePath;
use crate::pull_request::Sha;

const PARALLEL_READS: usize = 8;

/// By commit, then path, so a lookup borrows both and allocates nothing.
type FilesBySha = HashMap<Sha, HashMap<FilePath, Arc<[u8]>>>;

/// Reads each file once per process: a file at a commit never changes.
/// Failed reads are not kept, so a passing network error does not stick
/// and the caller always gets the real error.
pub struct CachedCodeSource {
    source: Arc<dyn CodeSource>,
    files: Mutex<FilesBySha>,
}

impl CachedCodeSource {
    pub fn wrapping(source: Arc<dyn CodeSource>) -> Self {
        Self {
            source,
            files: Mutex::new(HashMap::new()),
        }
    }

    /// A reader that panicked cannot leave a half-written entry, so a poisoned lock is still safe to use.
    fn files(&self) -> MutexGuard<'_, FilesBySha> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn already_read(&self, sha: &Sha, path: &FilePath) -> Option<Arc<[u8]>> {
        self.files().get(sha)?.get(path).cloned()
    }

    fn read(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        if let Some(bytes) = self.already_read(sha, path) {
            return Ok(bytes);
        }
        let bytes = self.source.blob_at(sha, path)?;
        self.files()
            .entry(sha.clone())
            .or_default()
            .insert(path.clone(), Arc::clone(&bytes));
        Ok(bytes)
    }
}

impl CodeSource for CachedCodeSource {
    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        Ok(String::from_utf8_lossy(&self.blob_at(sha, path)?).into_owned())
    }

    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        self.read(sha, path)
    }

    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
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

    fn fallback_reason(&self) -> Option<&str> {
        self.source.fallback_reason()
    }

    fn read_ahead(&self, files: &[FileAt]) {
        let missing: Vec<&FileAt> = files
            .iter()
            .filter(|file| self.already_read(&file.sha, &file.path).is_none())
            .collect();
        let next = AtomicUsize::new(0);
        thread::scope(|scope| {
            for _ in 0..PARALLEL_READS.min(missing.len()) {
                scope.spawn(|| {
                    while let Some(file) = missing.get(next.fetch_add(1, Ordering::Relaxed)) {
                        self.read(&file.sha, &file.path).ok();
                    }
                });
            }
        });
    }
}
