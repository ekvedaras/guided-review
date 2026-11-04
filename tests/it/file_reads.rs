use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::support::file_path;
use guided_review::cached_code_source::CachedCodeSource;
use guided_review::code_source::{BlobInfo, CodeSource, FileAt, FileDiff, parse_unified_diff};
use guided_review::file_path::FilePath;
use guided_review::pieces;
use guided_review::pull_request::Sha;
use guided_review::tour::Tour;
use guided_review::viewer::{Review, ReviewStateFile};
use serde_json::json;
use tempfile::TempDir;

/// Counts every read that reaches it. A read of a path in `read_together` waits until as many
/// such reads have started as there are paths, and fails if they never do.
#[derive(Default)]
struct CountingSource {
    files: HashMap<(String, String), String>,
    patch: String,
    reads: Mutex<HashMap<(String, String), usize>>,
    read_together: Vec<String>,
    started: Mutex<usize>,
    started_changed: Condvar,
}

impl CountingSource {
    fn with_file(mut self, sha: &str, path: &str, text: &str) -> Self {
        self.files
            .insert((sha.to_owned(), path.to_owned()), text.to_owned());
        self
    }

    fn with_patch(mut self, patch: &str) -> Self {
        patch.clone_into(&mut self.patch);
        self
    }

    fn reading_together(mut self, paths: &[&str]) -> Self {
        self.read_together = paths.iter().map(ToString::to_string).collect();
        self
    }

    fn reads_of(&self, sha: &str, path: &str) -> usize {
        let reads = self.reads.lock().unwrap();
        *reads.get(&(sha.to_owned(), path.to_owned())).unwrap_or(&0)
    }

    fn wait_for_other_reads(&self) -> anyhow::Result<()> {
        let mut started = self.started.lock().unwrap();
        *started += 1;
        self.started_changed.notify_all();
        let (started, timeout) = self
            .started_changed
            .wait_timeout_while(started, Duration::from_secs(5), |count| {
                *count < self.read_together.len()
            })
            .unwrap();
        drop(started);
        if timeout.timed_out() {
            anyhow::bail!("reads ran one after another");
        }
        Ok(())
    }
}

impl CodeSource for CountingSource {
    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        Ok(String::from_utf8_lossy(&self.blob_at(sha, path)?).into_owned())
    }

    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        let key = (sha.to_string(), path.to_string());
        *self.reads.lock().unwrap().entry(key.clone()).or_default() += 1;
        if self
            .read_together
            .iter()
            .any(|wanted| wanted == path.as_str())
        {
            self.wait_for_other_reads()?;
        }
        match self.files.get(&key) {
            Some(text) => Ok(text.as_bytes().into()),
            None => anyhow::bail!("no {path} at {sha}"),
        }
    }

    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
        anyhow::bail!("no blob info for {path} at {sha}")
    }

    fn diff(&self, _base: &Sha, _head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        parse_unified_diff(&self.patch)
    }

    fn merge_base(&self, a: &Sha, _b: &Sha) -> anyhow::Result<Sha> {
        Ok(a.clone())
    }

    fn working_tree_is_at(&self, _head: &Sha) -> bool {
        false
    }
}

fn cached(source: &Arc<CountingSource>) -> CachedCodeSource {
    CachedCodeSource::wrapping(Arc::clone(source) as Arc<dyn CodeSource>)
}

fn file_at(sha: &str, path: &str) -> FileAt {
    FileAt {
        sha: Sha::new(sha),
        path: file_path(path),
    }
}

#[test]
fn a_file_is_read_once_however_often_it_is_asked_for() {
    let source = Arc::new(CountingSource::default().with_file("head", "app/Order.php", "<?php"));
    let cache = cached(&source);

    for _ in 0..3 {
        assert_eq!(
            cache
                .file_at(&Sha::new("head"), &file_path("app/Order.php"))
                .unwrap(),
            "<?php"
        );
        assert_eq!(
            *cache
                .blob_at(&Sha::new("head"), &file_path("app/Order.php"))
                .unwrap(),
            *b"<?php"
        );
    }

    assert_eq!(source.reads_of("head", "app/Order.php"), 1);
}

#[test]
fn the_same_path_at_another_commit_is_another_file() {
    let source = Arc::new(
        CountingSource::default()
            .with_file("base", "app/Order.php", "old")
            .with_file("head", "app/Order.php", "new"),
    );
    let cache = cached(&source);

    assert_eq!(
        cache
            .file_at(&Sha::new("base"), &file_path("app/Order.php"))
            .unwrap(),
        "old"
    );
    assert_eq!(
        cache
            .file_at(&Sha::new("head"), &file_path("app/Order.php"))
            .unwrap(),
        "new"
    );
}

#[test]
fn a_file_that_could_not_be_read_is_asked_for_again() {
    let source = Arc::new(CountingSource::default());
    let cache = cached(&source);

    assert!(
        cache
            .file_at(&Sha::new("head"), &file_path(".gitattributes"))
            .is_err()
    );
    assert!(
        cache
            .file_at(&Sha::new("head"), &file_path(".gitattributes"))
            .is_err()
    );

    assert_eq!(source.reads_of("head", ".gitattributes"), 2);
}

#[test]
fn files_read_ahead_are_read_at_the_same_time_and_not_again_later() {
    let source = Arc::new(
        CountingSource::default()
            .with_file("head", "a.php", "a")
            .with_file("head", "b.php", "b")
            .with_file("head", "c.php", "c")
            .reading_together(&["a.php", "b.php", "c.php"]),
    );
    let cache = cached(&source);

    cache.read_ahead(&[
        file_at("head", "a.php"),
        file_at("head", "b.php"),
        file_at("head", "c.php"),
    ]);

    for (path, text) in [("a.php", "a"), ("b.php", "b"), ("c.php", "c")] {
        assert_eq!(
            cache.file_at(&Sha::new("head"), &file_path(path)).unwrap(),
            text
        );
        assert_eq!(source.reads_of("head", path), 1);
    }
}

#[test]
fn reading_ahead_skips_files_already_read() {
    let source = Arc::new(CountingSource::default().with_file("head", "a.php", "a"));
    let cache = cached(&source);
    cache
        .file_at(&Sha::new("head"), &file_path("a.php"))
        .unwrap();

    cache.read_ahead(&[file_at("head", "a.php")]);

    assert_eq!(source.reads_of("head", "a.php"), 1);
}

const TWO_PHP_FILES_CHANGED: &str = "\
diff --git a/app/Order.php b/app/Order.php
index 1111111..2222222 100644
--- a/app/Order.php
+++ b/app/Order.php
@@ -1 +1 @@
-<?php // order v1
+<?php // order v2
diff --git a/app/Cart.php b/app/Cart.php
index 3333333..4444444 100644
--- a/app/Cart.php
+++ b/app/Cart.php
@@ -1 +1 @@
-<?php // cart v1
+<?php // cart v2
";

#[test]
fn cutting_pieces_reads_both_sides_of_every_changed_file_and_the_git_attributes_at_the_same_time() {
    let source = Arc::new(
        CountingSource::default()
            .with_patch(TWO_PHP_FILES_CHANGED)
            .with_file("base", "app/Order.php", "<?php // order v1\n")
            .with_file("head", "app/Order.php", "<?php // order v2\n")
            .with_file("base", "app/Cart.php", "<?php // cart v1\n")
            .with_file("head", "app/Cart.php", "<?php // cart v2\n")
            .with_file("head", ".gitattributes", "")
            .reading_together(&["app/Order.php", "app/Cart.php", ".gitattributes"]),
    );
    let cache = cached(&source);

    let files = pieces::cut(&cache, &Sha::new("base"), &Sha::new("head")).unwrap();

    assert_eq!(files.len(), 2);
    for path in ["app/Order.php", "app/Cart.php"] {
        assert_eq!(source.reads_of("base", path), 1);
        assert_eq!(source.reads_of("head", path), 1);
    }
}

const TWO_NOTES_CHANGED: &str = "\
diff --git a/docs/a.txt b/docs/a.txt
index 1111111..2222222 100644
--- a/docs/a.txt
+++ b/docs/a.txt
@@ -1 +1 @@
-a v1
+a v2
diff --git a/docs/b.txt b/docs/b.txt
index 3333333..4444444 100644
--- a/docs/b.txt
+++ b/docs/b.txt
@@ -1 +1 @@
-b v1
+b v2
";

#[test]
fn the_review_reads_every_changed_file_at_head_for_context_at_the_same_time() {
    let source = Arc::new(
        CountingSource::default()
            .with_patch(TWO_NOTES_CHANGED)
            .with_file("head", "docs/a.txt", "a v2\n")
            .with_file("head", "docs/b.txt", "b v2\n")
            .reading_together(&["docs/a.txt", "docs/b.txt"]),
    );
    let cache = cached(&source);
    let tour: Tour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": "base",
        "head": "head",
        "title": "Notes",
        "summary": [],
        "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": [{"file": "docs/a.txt"}, {"file": "docs/b.txt"}], "children": []}],
    }))
    .unwrap();
    let home = TempDir::new().unwrap();
    let state_file = ReviewStateFile::for_pr(
        home.path(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    let review = Review::open(tour, &cache, state_file).unwrap();

    review.files_at_head_request().read(&cache);

    assert_eq!(
        cache
            .file_at(&Sha::new("head"), &file_path("docs/a.txt"))
            .unwrap(),
        "a v2\n"
    );
    assert_eq!(
        cache
            .file_at(&Sha::new("head"), &file_path("docs/b.txt"))
            .unwrap(),
        "b v2\n"
    );
    assert_eq!(source.reads_of("head", "docs/a.txt"), 1);
    assert_eq!(source.reads_of("head", "docs/b.txt"), 1);
}

/// Two hunks that claim the same head lines: the lines above line 4 add up to no base line.
const HUNKS_THAT_DO_NOT_ADD_UP: &str = "diff --git a/docs/a.txt b/docs/a.txt
--- a/docs/a.txt
+++ b/docs/a.txt
@@ -0,0 +1,2 @@
+new 1
+new 2
@@ -0,0 +2,1 @@
+new 2
";

#[test]
fn an_unchanged_line_whose_base_line_does_not_add_up_is_still_shown() {
    let source = Arc::new(
        CountingSource::default()
            .with_patch(HUNKS_THAT_DO_NOT_ADD_UP)
            .with_file("base", "docs/a.txt", "")
            .with_file("head", "docs/a.txt", "new 1\nnew 2\nkept\n"),
    );
    let cache = cached(&source);
    let tour: Tour = serde_json::from_value(json!({
        "pr": "https://github.com/acme/shop/pull/7",
        "base": "base",
        "head": "head",
        "title": "Notes",
        "summary": [],
        "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": [{"file": "docs/a.txt"}], "children": []}],
    }))
    .unwrap();
    let home = TempDir::new().unwrap();
    let state_file = ReviewStateFile::for_pr(
        home.path(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    let mut review = Review::open(tour, &cache, state_file).unwrap();
    let files_at_head = review.files_at_head_request().read(&cache);
    review.set_files_at_head(files_at_head);
    review.go_forward();

    let shown: Vec<String> = review
        .current_step()
        .unwrap()
        .pieces
        .iter()
        .flat_map(|piece| &piece.hunks)
        .flat_map(|hunk| &hunk.rows)
        .map(|row| row.text.clone())
        .collect();
    assert!(shown.iter().any(|text| text == "kept"), "{shown:?}");
}
