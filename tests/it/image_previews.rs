use std::sync::atomic::{AtomicUsize, Ordering};

use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, SentLog};
use crate::support::{FixtureRepo, file_path, png_of, svg_of};
use guided_review::code_source::{BlobInfo, CodeSource, FileDiff};
use guided_review::file_path::FilePath;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    CommentGesture, CommentResult, ImageCache, ImageMark, ImageSide, Review, ReviewStateFile,
    SendMode, TextBox,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";
const TEN_MB: usize = 10 * 1024 * 1024;

/// Reads through a real clone and counts how many times bytes were fetched.
struct CountingSource {
    inner: LocalGit,
    blob_reads: AtomicUsize,
}

impl CountingSource {
    fn blob_reads(&self) -> usize {
        self.blob_reads.load(Ordering::SeqCst)
    }
}

impl CodeSource for CountingSource {
    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        self.inner.file_at(sha, path)
    }
    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<std::sync::Arc<[u8]>> {
        self.blob_reads.fetch_add(1, Ordering::SeqCst);
        self.inner.blob_at(sha, path)
    }
    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
        self.inner.blob_info_at(sha, path)
    }
    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        self.inner.diff(base, head)
    }
    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        self.inner.merge_base(a, b)
    }
    fn working_tree_is_at(&self, head: &Sha) -> bool {
        self.inner.working_tree_is_at(head)
    }
}

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    path: &'static str,
}

impl Pull {
    /// One file, changed from `before` (nothing: added) to `after` (nothing: deleted).
    fn changing(path: &'static str, before: Option<&[u8]>, after: Option<&[u8]>) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "shop\n");
        if let Some(before) = before {
            repo.write_bytes(path, before);
        }
        let base = repo.commit("base");
        match after {
            Some(after) => repo.write_bytes(path, after),
            None => std::fs::remove_file(repo.path().join(path)).unwrap(),
        }
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            path,
        }
    }

    fn source(&self) -> CountingSource {
        CountingSource {
            inner: LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
                .unwrap()
                .unwrap(),
            blob_reads: AtomicUsize::new(0),
        }
    }

    fn cache(&self) -> ImageCache {
        ImageCache::under(self.repo.home())
    }

    fn open(&self, source: &CountingSource) -> Review {
        self.open_at(&self.head, source)
    }

    fn open_at(&self, head: &str, source: &CountingSource) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR, "base": self.base, "head": head, "title": "Images", "summary": [],
            "steps": [{"id": "pics", "title": "Pictures", "why": "because",
                "pieces": [{"file": self.path}], "children": []}],
        }))
        .unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &PR.parse().unwrap());
        let mut review = Review::open(tour, source, state_file).unwrap();
        review.go_forward();
        review
    }

    fn load_preview(&self, review: &mut Review, source: &CountingSource) {
        for request in review.image_preview_requests() {
            let preview = request.load(source, &self.cache());
            review.display_options_mut().set_image_preview(preview);
        }
    }
}

fn loaded(side: Option<&ImageSide>) -> String {
    match side {
        Some(side) => side.facts(),
        None => "none".to_owned(),
    }
}

#[test]
fn a_changed_image_shows_both_sides_with_pixel_size_file_size_and_the_change() {
    let (before, after) = (
        png_of(64, 64, [255, 0, 0, 255]),
        png_of(128, 96, [0, 0, 255, 255]),
    );
    let pull = Pull::changing("logo.png", Some(&before), Some(&after));
    let source = pull.source();
    let mut review = pull.open(&source);
    assert!(
        review
            .display_options()
            .image_preview(&file_path("logo.png"))
            .is_none(),
        "nothing loads before it is asked for"
    );

    pull.load_preview(&mut review, &source);

    let preview = review
        .display_options()
        .image_preview(&file_path("logo.png"))
        .unwrap();
    assert_eq!(
        (loaded(preview.base.as_ref()), loaded(preview.head.as_ref())),
        (
            format!("64×64, {} B", before.len()),
            format!("128×96, {} B", after.len())
        )
    );
    assert_eq!(
        preview.size_change().unwrap(),
        format!("{} B → {} B", before.len(), after.len())
    );
    assert_eq!(preview.mark(), None);
}

#[test]
fn an_added_image_is_one_image_marked_added_and_a_deleted_one_marked_deleted() {
    let picture = png_of(16, 16, [0, 0, 0, 255]);
    let added = Pull::changing("new.png", None, Some(&picture));
    let source = added.source();
    let mut review = added.open(&source);
    added.load_preview(&mut review, &source);
    let preview = review
        .display_options()
        .image_preview(&file_path("new.png"))
        .unwrap();
    assert!(preview.base.is_none());
    assert_eq!(
        loaded(preview.head.as_ref()),
        format!("16×16, {} B", picture.len())
    );
    assert_eq!(
        (preview.mark(), preview.size_change()),
        (Some(ImageMark::Added), None)
    );

    let deleted = Pull::changing("old.png", Some(&picture), None);
    let source = deleted.source();
    let mut review = deleted.open(&source);
    deleted.load_preview(&mut review, &source);
    let preview = review
        .display_options()
        .image_preview(&file_path("old.png"))
        .unwrap();
    assert!(preview.head.is_none());
    assert_eq!(
        loaded(preview.base.as_ref()),
        format!("16×16, {} B", picture.len())
    );
    assert_eq!(preview.mark(), Some(ImageMark::Deleted));
    assert_eq!(ImageMark::Added.label(), "added");
    assert_eq!(ImageMark::Deleted.label(), "deleted");
}

#[test]
fn a_side_above_ten_megabytes_is_never_fetched_and_shows_only_its_facts() {
    let huge = vec![0u8; TEN_MB + 1];
    let small = png_of(4, 4, [0, 0, 0, 255]);
    let pull = Pull::changing("big.png", Some(&huge), Some(&small));
    let source = pull.source();
    let mut review = pull.open(&source);
    let reads_to_open = source.blob_reads();

    pull.load_preview(&mut review, &source);

    let preview = review
        .display_options()
        .image_preview(&file_path("big.png"))
        .unwrap();
    assert!(matches!(preview.base, Some(ImageSide::TooLarge { .. })));
    assert_eq!(
        loaded(preview.base.as_ref()),
        "too large to preview, 10.0 MB"
    );
    assert_eq!(
        loaded(preview.head.as_ref()),
        format!("4×4, {} B", small.len())
    );
    assert_eq!(
        source.blob_reads() - reads_to_open,
        1,
        "only the small side was read"
    );
    assert_eq!(reads_to_open, 1, "the outline does not read it either");
}

#[test]
fn bytes_of_the_same_blob_come_from_the_disk_cache_even_in_a_later_session() {
    let picture = png_of(8, 8, [10, 20, 30, 255]);
    let pull = Pull::changing("a.png", Some(&picture), Some(&png_of(9, 9, [1, 2, 3, 255])));

    let first = pull.source();
    let mut review = pull.open(&first);
    let reads_to_open = first.blob_reads();
    pull.load_preview(&mut review, &first);
    assert_eq!(first.blob_reads() - reads_to_open, 2);

    let later = pull.source();
    let mut review = pull.open(&later);
    let reads_to_open = later.blob_reads();
    pull.load_preview(&mut review, &later);
    assert_eq!(
        later.blob_reads(),
        reads_to_open,
        "both blobs were cached by their sha"
    );
    assert!(
        review
            .display_options()
            .image_preview(&file_path("a.png"))
            .unwrap()
            .head
            .is_some()
    );
}

#[test]
fn an_svg_keeps_its_text_diff_next_to_the_picture() {
    let pull = Pull::changing(
        "icon.svg",
        Some(svg_of(24, 24).as_bytes()),
        Some(svg_of(48, 32).as_bytes()),
    );
    let source = pull.source();
    let mut review = pull.open(&source);

    pull.load_preview(&mut review, &source);

    let page = review.current_step().unwrap();
    let piece = &page.pieces[0];
    assert!(piece.is_image);
    assert!(piece.lines().any(|line| line.text.contains("width=\"48\"")));
    let preview = review
        .display_options()
        .image_preview(&file_path("icon.svg"))
        .unwrap();
    assert!(preview.is_svg);
    assert!(loaded(preview.head.as_ref()).starts_with("48×32"));
}

#[test]
fn a_file_comment_on_the_image_goes_through_the_comment_rules_as_a_file_thread() {
    let pull = Pull::changing(
        "logo.png",
        Some(&png_of(2, 2, [0, 0, 0, 255])),
        Some(&png_of(3, 3, [0, 0, 0, 255])),
    );
    let source = pull.source();
    let mut review = pull.open(&source);
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", &pull.head, &sent);

    let place = review
        .open_comment_box(CommentGesture::OnFile(&file_path("logo.png")))
        .unwrap()
        .unwrap();
    review
        .type_in(&TextBox::Draft(place.clone()), "is the red intended?")
        .unwrap();
    let result = review
        .send_comment(&place, SendMode::AddToReview, &github)
        .unwrap();

    assert_eq!(result, Some(CommentResult::Sent));
    assert!(place.is_file_comment());
    assert!(sent.github_calls().contains(&GitHubCall::AddThread {
        pr: PR.parse().unwrap(),
        place,
        body: "is the red intended?".into()
    }));
}

#[test]
fn github_names_a_blob_and_its_size_without_sending_the_bytes() {
    use std::io::{BufRead, BufReader, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let asked = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            request.push_str(&line.to_lowercase());
        }
        let body = r#"{"type":"file","sha":"980a0d5f","size":4096,"content":""}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        request
    });

    let info = guided_review::github::GitHub::at(&url)
        .code_source(&RepoName::new("acme", "shop"))
        .blob_info_at(&Sha::new("abc123"), &file_path("assets/my logo.png"))
        .unwrap();

    assert_eq!((info.id.as_str(), info.size), ("980a0d5f", 4096));
    let request = asked.join().unwrap();
    assert!(
        request.starts_with("get /repos/acme/shop/contents/assets/my%20logo.png?ref=abc123 "),
        "{request}"
    );
    assert!(
        request.contains("accept: application/vnd.github.object+json"),
        "{request}"
    );
}

#[test]
fn a_preview_loaded_for_another_head_is_dropped() {
    let (before, after) = (
        png_of(64, 64, [255, 0, 0, 255]),
        png_of(128, 96, [0, 0, 255, 255]),
    );
    let pull = Pull::changing("logo.png", Some(&before), Some(&after));
    let source = pull.source();
    let old_review = pull.open(&source);
    let stale = old_review
        .image_preview_requests()
        .remove(0)
        .load(&source, &pull.cache());

    pull.repo
        .write_bytes("logo.png", &png_of(8, 8, [0, 255, 0, 255]));
    let newer_head = pull.repo.commit("newer head");
    let mut new_review = pull.open_at(&newer_head, &source);
    new_review.display_options_mut().set_image_preview(stale);

    assert!(
        new_review
            .display_options()
            .image_preview(&file_path("logo.png"))
            .is_none()
    );
    assert_eq!(new_review.image_preview_requests().len(), 1);
}
