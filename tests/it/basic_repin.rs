use crate::support::fake_ports::{FakeHost, SentLog};
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, Run, file_path};
use guided_review::local_git::LocalGit;
use guided_review::opened_subject::TourRepinner;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::repin::RepinStatus;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, Tour, TourPin};
use guided_review::viewer::{
    GuidedTourProgress, PageRow, ReadSubject, RefreshBrief, Review, ReviewStateFile, TreeRowKind,
    repin_tour,
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    base: String,
    first_head: String,
}

impl Shop {
    /// A basic tour pinned to a head that changes a.txt, b.txt and gone.txt.
    fn with_basic_tour_at_first_head() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        for path in ["a.txt", "b.txt", "gone.txt"] {
            repo.write(path, "old\n");
        }
        let base = repo.commit("base");
        for path in ["a.txt", "b.txt", "gone.txt"] {
            repo.write(path, "first\n");
        }
        let first_head = repo.commit("first head");
        let shop = Self {
            repo,
            base,
            first_head,
        };
        BasicTour::new(
            TourPin {
                version: 1,
                pr: PR.to_owned(),
                local: None,
                repo: None,
                host: None,
                base: Sha::new(&shop.base),
                head: Sha::new(&shop.first_head),
                previous_head: None,
            },
            "Orders cost two".to_owned(),
        )
        .save(&shop.tour_path())
        .unwrap();
        shop
    }

    fn pr(&self) -> PullRequestUrl {
        PR.parse().unwrap()
    }

    fn tour_path(&self) -> PathBuf {
        let path = Tour::basic_path_for(&self.repo.folders(), &ReviewSubject::from(self.pr()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        path
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(&self.repo.folders(), &ReviewSubject::from(self.pr()))
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour = BasicTour::load(&self.tour_path()).unwrap();
        Review::open_basic(
            tour,
            &self.source(),
            self.state_file(),
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }

    /// a.txt changes again, gone.txt goes back to base, new.txt joins, b.txt stays as it was.
    fn commit_second_head(&self) -> String {
        self.repo.write("a.txt", "second\n");
        self.repo.write("gone.txt", "old\n");
        self.repo.write("new.txt", "new\n");
        self.repo.commit("second head")
    }

    fn repin_onto(&self, head: &str) {
        repin_tour(
            &self.repo.folders(),
            &ReviewSubject::from(self.pr()),
            &self.tour_path(),
            &self.state_file(),
            &self.source(),
            &Sha::new(&self.base),
            &Sha::new(head),
        )
        .unwrap();
    }

    fn state_on_disk(&self) -> Value {
        let path = self
            .repo
            .home()
            .join(".local/state/guided-review/state/acme/shop/7.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }
}

fn marks(review: &Review) -> Vec<(String, Option<RepinStatus>)> {
    review
        .basic_sidebar()
        .into_iter()
        .filter_map(|row| match row.kind {
            TreeRowKind::File { path, repin, .. } => Some((path.to_string(), repin)),
            TreeRowKind::Folder { .. } => None,
        })
        .collect()
}

#[test]
fn re_pin_moves_a_basic_tour_to_the_new_head_and_it_stays_basic() {
    let shop = Shop::with_basic_tour_at_first_head();
    let second_head = shop.commit_second_head();

    shop.repin_onto(&second_head);

    let on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(shop.tour_path()).unwrap()).unwrap();
    assert_eq!(on_disk["kind"], "basic");
    assert_eq!(on_disk["head"], second_head.as_str());
    assert_eq!(on_disk["previous_head"], shop.first_head.as_str());
    assert_eq!(on_disk["pr_title"], "Orders cost two");
    assert!(on_disk.get("steps").is_none());
}

#[test]
fn a_changed_file_loses_its_tick_and_unchanged_files_keep_theirs() {
    let shop = Shop::with_basic_tour_at_first_head();
    let mut review = shop.open();
    for path in ["a.txt", "b.txt"] {
        review.tick_file(&file_path(path)).unwrap();
    }
    let second_head = shop.commit_second_head();

    shop.repin_onto(&second_head);

    let review = shop.open();
    assert!(!review.is_file_ticked(&file_path("a.txt")));
    assert!(review.is_file_ticked(&file_path("b.txt")));
    assert_eq!(
        shop.state_on_disk()["file_read_before"]["a.txt"]["head"],
        shop.first_head.as_str()
    );
}

#[test]
fn a_file_that_lost_its_tick_shows_what_changed_since_it_was_read() {
    let shop = Shop::with_basic_tour_at_first_head();
    let mut review = shop.open();
    for path in ["a.txt", "b.txt"] {
        review.tick_file(&file_path(path)).unwrap();
    }
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);

    let mut review = shop.open();
    review.go_to_file(&file_path("a.txt"));
    let page = review.file_page().unwrap();
    let since_read = page.since_read.unwrap();
    assert_eq!(since_read.read_at, Sha::new(&shop.first_head));
    let texts: Vec<&str> = page
        .pieces
        .iter()
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .map(|line| line.text.as_str())
        .collect();
    assert!(texts.contains(&"second"), "{texts:?}");
    assert!(!texts.contains(&"old"), "{texts:?}");

    review.go_to_file(&file_path("new.txt"));
    assert!(review.file_page().unwrap().since_read.is_none());
}

#[test]
fn new_files_join_the_tree_and_gone_files_leave_it_with_re_pin_marks() {
    let shop = Shop::with_basic_tour_at_first_head();
    let second_head = shop.commit_second_head();

    shop.repin_onto(&second_head);

    assert_eq!(
        marks(&shop.open()),
        [
            ("a.txt".to_owned(), Some(RepinStatus::Changed)),
            ("b.txt".to_owned(), None),
            ("new.txt".to_owned(), Some(RepinStatus::New)),
        ]
    );
}

#[test]
fn the_full_diff_of_a_file_read_before_can_be_asked_for_and_the_toolbar_offers_it() {
    let shop = Shop::with_basic_tour_at_first_head();
    let mut review = shop.open();
    review.tick_file(&file_path("a.txt")).unwrap();
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();
    review.go_to_file(&file_path("a.txt"));

    let offered = review.page().rows().iter().any(|row| {
        matches!(row, PageRow::Toolbar(toolbar)
        if toolbar.full_diff.as_ref().is_some_and(|control| {
            control.subject == ReadSubject::File(file_path("a.txt"))
                && !control.is_showing_full_diff
        }))
    });
    assert!(offered);

    review.toggle_full_diff_of(&ReadSubject::File(file_path("a.txt")));

    let page = review.file_page().unwrap();
    assert!(page.since_read.unwrap().showing_full_diff);
    let texts: Vec<&str> = page
        .pieces
        .iter()
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .map(|line| line.text.as_str())
        .collect();
    assert!(texts.contains(&"old"), "{texts:?}");
}

#[test]
fn a_read_file_the_new_commits_left_alone_says_it_has_no_change_since_it_was_read() {
    let shop = Shop::with_basic_tour_at_first_head();
    let mut review = shop.open();
    review.tick_file(&file_path("b.txt")).unwrap();
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();
    review.go_to_file(&file_path("b.txt"));

    assert!(
        review
            .page()
            .rows()
            .iter()
            .any(|row| matches!(row, PageRow::NoChangeSinceRead))
    );
}

#[test]
fn update_tour_re_pins_the_basic_tour_not_the_guided_one_beside_it_and_the_banner_survives() {
    let shop = Shop::with_basic_tour_at_first_head();
    let guided_path = Tour::path_for(&shop.repo.folders(), &ReviewSubject::from(shop.pr()));
    let guided = serde_json::json!({
        "pr": PR, "base": shop.base, "head": shop.first_head,
        "title": "Orders", "summary": ["Orders change"], "steps": [],
    });
    std::fs::write(&guided_path, guided.to_string()).unwrap();
    let mut old = shop.open();
    old.guided_tour_is(GuidedTourProgress::Ready);
    let second_head = shop.commit_second_head();
    let host = FakeHost::with_pull(&shop.base, &second_head, &SentLog::default());
    let clone = shop.repo.path().to_path_buf();
    let repinner = TourRepinner {
        host: crate::support::fake_ports::every_capability(),
        folders: shop.repo.folders(),
        subject: shop.pr().into(),
        pick_code_source: Arc::new(move |_| {
            Ok(Box::new(
                LocalGit::find(&RepoName::new("acme", "shop"), &clone)?.unwrap(),
            ))
        }),
    };

    let mut repinned = repinner.run(&host).unwrap().review;
    repinned.carry_on_from(old);

    assert_eq!(repinned.basic_mode_label(), Some("Basic mode"));
    assert_eq!(repinned.head(), &Sha::new(&second_head));
    let guided_on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(&guided_path).unwrap()).unwrap();
    assert_eq!(guided_on_disk["head"], shop.first_head.as_str());
    assert!(repinned.guided_tour_banner().is_some());
    assert_eq!(repinned.refresh_brief(), RefreshBrief::default());
}

fn cli_repin(shop: &Shop, head: &str) -> Run {
    let github = FakeGitHub::serving_pull("acme/shop", 7, &shop.base, head);
    GuidedReview::new(&shop.repo, &github).run(&["repin", PR])
}

#[test]
fn the_cli_re_pins_the_basic_tour_of_a_pr_with_no_guided_tour() {
    let shop = Shop::with_basic_tour_at_first_head();
    let second_head = shop.commit_second_head();

    let run = cli_repin(&shop, &second_head);

    assert!(run.success, "{}", run.stderr);
    assert!(run.stdout.contains("new new.txt"), "{}", run.stdout);
    let on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(shop.tour_path()).unwrap()).unwrap();
    assert_eq!(on_disk["kind"], "basic");
    assert_eq!(on_disk["head"], second_head.as_str());
    assert!(!Tour::path_for(&shop.repo.folders(), &ReviewSubject::from(shop.pr())).exists());
}

#[test]
fn the_cli_re_pin_of_a_pr_with_no_tour_names_the_guided_tour_file_it_could_not_read() {
    let shop = Shop::with_basic_tour_at_first_head();
    std::fs::remove_file(shop.tour_path()).unwrap();
    let second_head = shop.commit_second_head();

    let run = cli_repin(&shop, &second_head);

    assert!(!run.success);
    let guided_path = Tour::path_for(&shop.repo.folders(), &ReviewSubject::from(shop.pr()));
    assert!(
        run.stderr.contains(&guided_path.display().to_string()),
        "{}",
        run.stderr
    );
}

#[test]
fn the_cli_re_pins_the_guided_tour_and_leaves_the_basic_one_when_both_exist() {
    let shop = Shop::with_basic_tour_at_first_head();
    let guided_path = Tour::path_for(&shop.repo.folders(), &ReviewSubject::from(shop.pr()));
    let guided = serde_json::json!({
        "pr": PR, "base": shop.base, "head": shop.first_head,
        "title": "Orders", "summary": ["Orders change"], "steps": [],
    });
    std::fs::write(&guided_path, guided.to_string()).unwrap();
    let second_head = shop.commit_second_head();

    let run = cli_repin(&shop, &second_head);

    assert!(run.success, "{}", run.stderr);
    let guided_on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(&guided_path).unwrap()).unwrap();
    assert_eq!(guided_on_disk["head"], second_head.as_str());
    let basic_on_disk: Value =
        serde_json::from_str(&std::fs::read_to_string(shop.tour_path()).unwrap()).unwrap();
    assert_eq!(basic_on_disk["head"], shop.first_head.as_str());
}

#[test]
fn a_re_pinned_basic_tour_has_nothing_to_refresh() {
    let shop = Shop::with_basic_tour_at_first_head();
    let second_head = shop.commit_second_head();

    shop.repin_onto(&second_head);

    assert_eq!(shop.open().refresh_brief(), RefreshBrief::default());
}
