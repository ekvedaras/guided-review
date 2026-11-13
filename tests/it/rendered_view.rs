use crate::support::{FixtureRepo, file_path, line, svg_of};
use guided_review::file_path::FilePath;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::viewer::{
    BlockChange, CommentGesture, CommentId, FileView, ImageCache, LineLocation, PageRow, Review,
    ReviewStateFile, ReviewThread, Side, ThreadComment, ThreadId,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

const README_AT_BASE: &str = "# Shop\n\nSells things.\n\n## Install\n\nRun make.\n\n## Old section\n\nGone soon.\n\n## Usage\n\nUse it.\n";
const README_AT_HEAD: &str = "# Shop\n\nSells many things.\n\n## Install\n\nRun make.\n\n## Usage\n\nUse it.\n\n## License\n\nMIT.\n";

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    files: Vec<&'static str>,
}

impl Pull {
    fn of(changes: &[(&'static str, Option<&str>, Option<&str>)]) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("keep.txt", "kept\n");
        for (path, before, _) in changes {
            if let Some(before) = before {
                repo.write(path, before);
            }
        }
        let base = repo.commit("base");
        for (path, _, after) in changes {
            match after {
                Some(after) => repo.write(path, after),
                None => std::fs::remove_file(repo.path().join(path)).unwrap(),
            }
        }
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            files: changes.iter().map(|(path, _, _)| *path).collect(),
        }
    }

    fn readme() -> Self {
        Self::of(&[("README.md", Some(README_AT_BASE), Some(README_AT_HEAD))])
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    /// One step with every file, its files at head read.
    fn open(&self) -> Review {
        let pieces: Vec<_> = self
            .files
            .iter()
            .map(|path| json!({"file": path}))
            .collect();
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR, "base": self.base, "head": self.head, "title": "Docs", "summary": [],
            "steps": [{"id": "docs", "title": "Docs", "why": "because", "pieces": pieces,
                "children": []}],
        }))
        .unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &PR.parse().unwrap());
        let source = self.source();
        let mut review = Review::open(tour, &source, state_file).unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_to(&"docs".into());
        review
    }

    fn load_pictures(&self, review: &mut Review) {
        let source = self.source();
        for request in review.image_preview_requests() {
            let preview = request.load(&source, &ImageCache::under(self.repo.home()));
            review.display_options_mut().set_image_preview(preview);
        }
    }
}

fn view_switch(review: &Review, path: &str) -> Option<FileView> {
    review
        .page()
        .rows()
        .iter()
        .find_map(|row| match row {
            PageRow::PieceHeader(heading) if heading.path == file_path(path) => Some(heading),
            _ => None,
        })
        .expect("the file has a box")
        .view
        .as_ref()
        .map(|switch| switch.view)
}

fn show_as(review: &mut Review, path: &str, view: FileView) {
    review
        .display_options_mut()
        .show_file_as(&file_path(path), view);
}

/// The rows of a file's box, from its header to its end.
fn box_rows(review: &Review, path: &str) -> Vec<PageRow> {
    let path = file_path(path);
    review
        .page()
        .rows()
        .iter()
        .skip_while(|row| !matches!(row, PageRow::PieceHeader(heading) if heading.path == path))
        .take_while(|row| !matches!(row, PageRow::PieceEnd))
        .cloned()
        .collect()
}

fn blocks(review: &Review, path: &str) -> Vec<(String, Option<BlockChange>)> {
    box_rows(review, path)
        .iter()
        .filter_map(|row| match row {
            PageRow::RenderedBlock(block) => Some((block.source.to_string(), block.change)),
            _ => None,
        })
        .collect()
}

fn shows_lines_of(review: &Review, path: &FilePath) -> bool {
    review
        .page()
        .rows()
        .iter()
        .any(|row| matches!(row, PageRow::Line(line) if line.location.path == *path))
}

fn shows_picture_of(review: &Review, path: &FilePath) -> bool {
    review
        .page()
        .rows()
        .iter()
        .any(|row| matches!(row, PageRow::Image { path: shown, .. } if shown == path))
}

#[test]
fn only_markdown_and_picture_boxes_offer_the_view_switch_markdown_first_as_code_pictures_rendered()
{
    let pull = Pull::of(&[
        ("README.md", Some("# A\n"), Some("# B\n")),
        ("docs/guide.markdown", Some("a\n"), Some("b\n")),
        ("src/lib.rs", Some("fn a() {}\n"), Some("fn b() {}\n")),
        ("logo.svg", Some(&svg_of(4, 4)), Some(&svg_of(8, 8))),
    ]);
    let review = pull.open();

    assert_eq!(view_switch(&review, "README.md"), Some(FileView::Code));
    assert_eq!(
        view_switch(&review, "docs/guide.markdown"),
        Some(FileView::Code)
    );
    assert_eq!(view_switch(&review, "src/lib.rs"), None);
    assert_eq!(view_switch(&review, "logo.svg"), Some(FileView::Rendered));
}

#[test]
fn a_picture_with_no_code_to_show_offers_no_switch() {
    let repo_pull = Pull::of(&[("README.md", Some("a\n"), Some("b\n"))]);
    repo_pull
        .repo
        .write_bytes("logo.png", &crate::support::png_of(4, 4, [0, 0, 0, 255]));
    let head = repo_pull.repo.commit("a picture");
    let pull = Pull {
        head,
        files: vec!["logo.png"],
        ..repo_pull
    };
    let review = pull.open();

    assert_eq!(view_switch(&review, "logo.png"), None);
    assert!(shows_picture_of(&review, &file_path("logo.png")));
}

#[test]
fn the_switch_shows_a_markdown_file_rendered_and_back_as_code() {
    let pull = Pull::readme();
    let mut review = pull.open();
    let readme = file_path("README.md");
    assert!(shows_lines_of(&review, &readme));
    assert_eq!(blocks(&review, "README.md").len(), 0);

    show_as(&mut review, "README.md", FileView::Rendered);

    assert_eq!(view_switch(&review, "README.md"), Some(FileView::Rendered));
    assert!(!shows_lines_of(&review, &readme));
    assert_eq!(blocks(&review, "README.md").len(), 10);

    show_as(&mut review, "README.md", FileView::Code);

    assert!(shows_lines_of(&review, &readme));
    assert_eq!(blocks(&review, "README.md").len(), 0);
}

#[test]
fn the_rendered_view_marks_changed_and_added_blocks_and_shows_removed_ones_where_they_were() {
    let pull = Pull::readme();
    let mut review = pull.open();

    show_as(&mut review, "README.md", FileView::Rendered);

    let shown = blocks(&review, "README.md");
    let expected: Vec<(String, Option<BlockChange>)> = [
        ("# Shop", None),
        ("Sells many things.", Some(BlockChange::Changed)),
        ("## Install", None),
        ("Run make.", None),
        ("## Old section", Some(BlockChange::Removed)),
        ("Gone soon.", Some(BlockChange::Removed)),
        ("## Usage", None),
        ("Use it.", None),
        ("## License", Some(BlockChange::Added)),
        ("MIT.", Some(BlockChange::Added)),
    ]
    .into_iter()
    .map(|(source, change)| (source.to_owned(), change))
    .collect();
    assert_eq!(shown, expected);
}

#[test]
fn a_line_taken_out_of_a_list_marks_the_list_changed() {
    let pull = Pull::of(&[(
        "notes.md",
        Some("Intro.\n\n- one\n- two\n- three\n"),
        Some("Intro.\n\n- one\n- three\n"),
    )]);
    let mut review = pull.open();

    show_as(&mut review, "notes.md", FileView::Rendered);

    assert_eq!(
        blocks(&review, "notes.md"),
        vec![
            ("Intro.".to_owned(), None),
            ("- one\n- three".to_owned(), Some(BlockChange::Changed)),
        ]
    );
}

#[test]
fn an_added_markdown_file_is_all_added_and_a_deleted_one_is_its_base_all_removed() {
    let pull = Pull::of(&[
        ("docs/new.md", None, Some("# New\n\nFresh.\n")),
        ("docs/old.md", Some("# Old\n\nStale.\n"), None),
    ]);
    let mut review = pull.open();

    show_as(&mut review, "docs/new.md", FileView::Rendered);
    show_as(&mut review, "docs/old.md", FileView::Rendered);

    assert_eq!(
        blocks(&review, "docs/new.md"),
        vec![
            ("# New".to_owned(), Some(BlockChange::Added)),
            ("Fresh.".to_owned(), Some(BlockChange::Added)),
        ]
    );
    assert_eq!(
        blocks(&review, "docs/old.md"),
        vec![
            ("# Old".to_owned(), Some(BlockChange::Removed)),
            ("Stale.".to_owned(), Some(BlockChange::Removed)),
        ]
    );
}

#[test]
fn a_drag_over_a_rendered_file_opens_no_comment_box() {
    let pull = Pull::readme();
    let mut review = pull.open();
    show_as(&mut review, "README.md", FileView::Rendered);
    let changed_line = LineLocation {
        path: file_path("README.md"),
        side: Side::Head,
        line: line(3),
    };

    review.begin_selection(changed_line);
    let opened = review.open_comment_box(CommentGesture::DragEnded);

    assert!(matches!(opened, Ok(None)));
    assert_eq!(review.comment_boxes().count(), 0);
}

fn thread_on_line(number: usize) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new("t1"),
        is_resolved: false,
        is_outdated: false,
        path: file_path("README.md"),
        line: Some(line(number)),
        start_line: None,
        side: Side::Head,
        is_file_comment: false,
        original_line: Some(line(number)),
        original_start_line: None,
        original_commit: Some("abc".into()),
        comments: vec![ThreadComment {
            avatar_url: None,
            id: CommentId::new("c1"),
            author: "alice".into(),
            is_bot: false,
            by_viewer: false,
            body: "Many?".into(),
            url: String::new(),
            reactions: Vec::new(),
        }],
    }
}

#[test]
fn threads_on_lines_of_a_rendered_file_are_listed_under_it_inside_its_box() {
    let pull = Pull::readme();
    let mut review = pull.open();
    review.set_threads(&[thread_on_line(3)]);

    show_as(&mut review, "README.md", FileView::Rendered);

    let rows = box_rows(&review, "README.md");
    let last_block = rows
        .iter()
        .rposition(|row| matches!(row, PageRow::RenderedBlock(_)))
        .unwrap();
    let thread = rows
        .iter()
        .position(
            |row| matches!(row, PageRow::Thread { view, .. } if view.thread.id.as_str() == "t1"),
        )
        .expect("the thread is on the page");
    assert!(thread > last_block, "the thread follows the rendered file");
}

#[test]
fn a_changed_svg_shows_its_pictures_rendered_and_its_lines_as_code() {
    let pull = Pull::of(&[("logo.svg", Some(&svg_of(4, 4)), Some(&svg_of(8, 8)))]);
    let mut review = pull.open();
    let logo = file_path("logo.svg");

    assert!(shows_picture_of(&review, &logo));
    assert!(!shows_lines_of(&review, &logo));

    show_as(&mut review, "logo.svg", FileView::Code);

    assert!(!shows_picture_of(&review, &logo));
    assert!(shows_lines_of(&review, &logo));
}

#[test]
fn a_rendered_picture_pairs_before_and_after_and_shows_one_side_when_added_or_deleted() {
    let pull = Pull::of(&[
        ("changed.svg", Some(&svg_of(4, 4)), Some(&svg_of(8, 8))),
        ("added.svg", None, Some(&svg_of(8, 8))),
        ("deleted.svg", Some(&svg_of(4, 4)), None),
    ]);
    let mut review = pull.open();

    pull.load_pictures(&mut review);

    let sides = |path: &str| {
        assert!(shows_picture_of(&review, &file_path(path)));
        let preview = review
            .display_options()
            .image_preview(&file_path(path))
            .unwrap();
        (preview.base.is_some(), preview.head.is_some())
    };
    assert_eq!(sides("changed.svg"), (true, true));
    assert_eq!(sides("added.svg"), (false, true));
    assert_eq!(sides("deleted.svg"), (true, false));
}
