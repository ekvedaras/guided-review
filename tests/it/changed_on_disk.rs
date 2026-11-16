use std::time::{Duration, Instant};

use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::work::drive_without_agents;
use crate::support::{FixtureRepo, file_path};
use guided_review::config::Config;
use guided_review::ide::Ide;
use guided_review::local_work_tree::{LocalWorkTree, snapshot_ref};
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{
    Destination, DiskNews, DiskOffer, IdeCaveat, PieceDiff, Review, Side, WorkKind,
};

/// `feature` one commit past `main` that adds the cart, with the README changed and not committed.
struct Feature {
    repo: FixtureRepo,
    subject: ReviewSubject,
}

impl Feature {
    fn with_work_in_progress() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        repo.commit("Start the shop");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("src/cart.rs", "pub fn total() {}\n");
        repo.commit("Add the cart");
        repo.write("README.md", "hello\nworld\n");
        let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
        let subject = ReviewSubject::from(work_tree.change_set().unwrap());
        Self { repo, subject }
    }

    /// The review of `feature`, whatever the folder is on now.
    fn subject(&self) -> ReviewSubject {
        self.subject.clone()
    }

    fn open(&self) -> OpenedReview {
        open_review(&self.subject(), self.repo.path(), &self.repo.folders()).unwrap()
    }
}

/// Runs the window's focus pull, as the app does when the window comes to the front.
fn check_on_focus(opened: &mut OpenedReview, now: Instant) -> Vec<WorkKind> {
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", "snapshot", &sent);
    opened.review.pull_from_github();
    let ran = drive_without_agents(&mut opened.review, &github, opened.source.as_ref(), now);
    assert_eq!(sent.github_calls(), []);
    ran
}

fn news(review: &Review) -> Option<DiskNews> {
    review.disk_news()
}

fn changed_on_disk() -> Option<DiskNews> {
    Some(DiskNews {
        line: "Changed on disk".to_owned(),
        offer: Some(DiskOffer::Update),
    })
}

#[test]
fn a_folder_left_as_it_was_snapshotted_says_nothing_and_is_checked_alone_on_focus() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();

    let ran = check_on_focus(&mut opened, Instant::now());

    assert_eq!(ran, [WorkKind::CheckWorkingTree]);
    assert_eq!(news(&opened.review), None);
}

#[test]
fn writing_a_file_says_changed_on_disk_with_update() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.write("notes.txt", "a new file\n");

    check_on_focus(&mut opened, Instant::now());

    assert_eq!(news(&opened.review), changed_on_disk());
}

#[test]
fn committing_says_changed_on_disk_with_update() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.commit("Say world");

    check_on_focus(&mut opened, Instant::now());

    assert_eq!(news(&opened.review), changed_on_disk());
}

#[test]
fn a_folder_on_another_branch_says_so_and_offers_no_update() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.git(&["checkout", "-q", "-b", "other"]);

    check_on_focus(&mut opened, Instant::now());

    assert_eq!(
        news(&opened.review),
        Some(DiskNews {
            line: "The folder is on other now; this review is of feature".to_owned(),
            offer: None,
        })
    );
}

#[test]
fn the_review_is_itself_again_once_its_branch_is_checked_out_again() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.git(&["stash", "-q"]);
    feature.repo.git(&["checkout", "-q", "main"]);
    let now = Instant::now();
    check_on_focus(&mut opened, now);
    feature.repo.git(&["checkout", "-q", "feature"]);
    feature.repo.git(&["stash", "pop", "-q"]);

    check_on_focus(&mut opened, now);

    assert_eq!(news(&opened.review), None);
}

#[test]
fn a_deleted_branch_says_it_is_gone_and_offers_drop_review() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.git(&["stash", "-q"]);
    feature.repo.git(&["checkout", "-q", "main"]);
    feature.repo.git(&["branch", "-q", "-D", "feature"]);

    check_on_focus(&mut opened, Instant::now());

    assert_eq!(
        news(&opened.review),
        Some(DiskNews {
            line: "The branch feature is gone".to_owned(),
            offer: Some(DiskOffer::DropReview),
        })
    );
}

fn readme_caveat(opened: &mut OpenedReview, feature: &Feature) -> Option<IdeCaveat> {
    let config = Config::from_text("ide = \"idea://open?file={path}&line={line}\"").unwrap();
    let ide = Ide::for_clone(
        Some(feature.repo.path().to_path_buf()),
        config.ide_template(),
    );
    let review = &mut opened.review;
    let opener = review.code_opener(ide);
    review.go_to_file(&file_path("README.md"));
    let page = review.file_page().unwrap();
    let line = page
        .pieces
        .iter()
        .flat_map(PieceDiff::lines)
        .find(|line| line.location.side == Side::Head)
        .unwrap();
    match opener.destination(&line.code_link) {
        Destination::Ide { caveat, .. } => caveat,
        other => panic!("README.md does not open in the IDE: {other:?}"),
    }
}

#[test]
fn code_links_say_the_lines_may_differ_only_while_the_folder_is_not_the_snapshot() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let now = Instant::now();
    check_on_focus(&mut opened, now);
    let while_same = readme_caveat(&mut opened, &feature);
    feature.repo.write("README.md", "hello\nthere\n");

    check_on_focus(&mut opened, now);

    assert_eq!(while_same, None);
    assert_eq!(
        readme_caveat(&mut opened, &feature),
        Some(IdeCaveat::CloneNotAtTourHead)
    );
}

#[test]
fn the_folder_is_checked_again_every_thirty_seconds() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let now = Instant::now();
    check_on_focus(&mut opened, now);
    feature.repo.write("notes.txt", "a new file\n");
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", "snapshot", &sent);

    let before = drive_without_agents(
        &mut opened.review,
        &github,
        opened.source.as_ref(),
        now + Duration::from_secs(29),
    );
    let after = drive_without_agents(
        &mut opened.review,
        &github,
        opened.source.as_ref(),
        now + Duration::from_secs(30),
    );

    assert_eq!(before, []);
    assert_eq!(after, [WorkKind::CheckWorkingTree]);
    assert_eq!(news(&opened.review), changed_on_disk());
}

/// Update, as the sidebar's button runs it: the re-pinned review, with the old one carried into it.
fn update(opened: &mut OpenedReview) {
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", "snapshot", &sent);
    let updated = opened.repinner.run(&github).unwrap();
    let replaced = std::mem::replace(&mut opened.review, updated.review);
    opened.review.carry_on_from(replaced);
    opened.source = updated.source;
    assert_eq!(sent.github_calls(), []);
}

fn snapshot_ref_target(feature: &Feature) -> Sha {
    let change_set = feature.subject().local().unwrap().clone();
    Sha::new(
        feature
            .repo
            .git(&["rev-parse", &snapshot_ref(&change_set)])
            .trim(),
    )
}

fn ticked(review: &Review) -> Vec<String> {
    ["README.md", "src/cart.rs", "notes.txt"]
        .into_iter()
        .filter(|path| review.is_file_ticked(&file_path(path)))
        .map(str::to_owned)
        .collect()
}

#[test]
fn update_moves_the_tour_and_its_one_ref_to_a_new_snapshot_with_the_new_facts() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let old_head = opened.review.head().clone();
    feature.repo.write("notes.txt", "a new file\n");

    update(&mut opened);

    let new_head = opened.review.head().clone();
    assert_ne!(new_head, old_head);
    assert_eq!(snapshot_ref_target(&feature), new_head);
    assert_eq!(
        feature
            .repo
            .git(&["for-each-ref", "--format=%(refname)", "refs/guided-review/"])
            .lines()
            .count(),
        1
    );
    let uncommitted = &opened.review.local_facts().unwrap().uncommitted_files;
    assert!(
        uncommitted.contains(&file_path("notes.txt")),
        "{uncommitted:?}"
    );
    check_on_focus(&mut opened, Instant::now());
    assert_eq!(news(&opened.review), None);
}

#[test]
fn update_keeps_the_ticks_of_unchanged_files_and_shows_what_changed_since_a_file_was_read() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let read_at = opened.review.head().clone();
    for path in ["README.md", "src/cart.rs"] {
        opened.review.tick_file(&file_path(path)).unwrap();
    }
    feature.repo.write("README.md", "hello\nworld\nagain\n");

    update(&mut opened);

    assert_eq!(ticked(&opened.review), ["src/cart.rs"]);
    opened.review.go_to_file(&file_path("README.md"));
    let since_read = opened.review.file_page().unwrap().since_read.unwrap();
    assert_eq!(since_read.read_at, read_at);
}

#[test]
fn committing_what_was_uncommitted_keeps_every_tick_and_names_it_committed() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    for path in ["README.md", "src/cart.rs"] {
        opened.review.tick_file(&file_path(path)).unwrap();
    }
    feature.repo.commit("Say world");

    update(&mut opened);

    assert_eq!(ticked(&opened.review), ["README.md", "src/cart.rs"]);
    let facts = opened.review.local_facts().unwrap();
    assert!(facts.uncommitted_files.is_empty(), "{facts:?}");
    assert_eq!(facts.commit_subjects, ["Add the cart", "Say world"]);
}

#[test]
fn two_updates_of_a_folder_that_did_not_change_pin_the_same_snapshot() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.write("notes.txt", "a new file\n");
    update(&mut opened);
    let first = opened.review.head().clone();

    update(&mut opened);

    assert_eq!(opened.review.head(), &first);
    assert_eq!(snapshot_ref_target(&feature), first);
}

#[test]
fn update_refuses_while_the_folder_is_on_another_branch_and_moves_nothing() {
    let feature = Feature::with_work_in_progress();
    let opened = feature.open();
    let head = opened.review.head().clone();
    feature.repo.git(&["checkout", "-q", "-b", "other"]);
    let sent = SentLog::default();

    let refused = opened
        .repinner
        .run(&FakeGitHub::with_pull("main-tip", "snapshot", &sent));

    assert!(refused.is_err());
    assert_eq!(snapshot_ref_target(&feature), head);
    assert_eq!(feature.open().review.head(), &head);
}
