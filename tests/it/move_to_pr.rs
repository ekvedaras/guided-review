//! A local review finds the PR opened from its branch and offers to move to it.

use std::time::{Duration, Instant};

use anyhow::Context as _;

use crate::support::fake_ports::{FakeHost, HostCall, HostFault, SentLog};
use crate::support::work::drive_without_agents;
use crate::support::{FixtureRepo, serve_files};
use guided_review::github::GitHub;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::make_basic_tour::make_basic_tour;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::pull_request::{HostSite, HostedRepo, PullRequestUrl, RepoName, Sha};
use guided_review::review_subject::{BranchName, LocalChangeSet, ReviewSubject};
use guided_review::tour::{Tour, ToursOnDisk};
use guided_review::viewer::{AfterJob, JobOutcome, ReplacingJob, StartedJob, StartedMove};
use guided_review::viewer::{
    Audience, BranchOffer, CommentGesture, CommentPlace, LineLocation, LocalAuthor,
    LocalThreadStore, MoveFrom, MoveHereOffer, MoveOffer, PULL_OF_BRANCH_RETRY, PieceDiff, PrMover,
    Problem, PullOfBranch, PullsOfBranchPort, Resolution, Review, ReviewStateFile, SendMode, Side,
    TextBox,
};

const SHOP_7: &str = "https://github.com/acme/shop/pull/7";

/// `feature` one commit past `main` that adds the cart, with the README's second line written and not
/// committed.
struct Feature {
    repo: FixtureRepo,
    subject: ReviewSubject,
    sent: SentLog,
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
        Self {
            repo,
            subject,
            sent: SentLog::default(),
        }
    }

    fn host(&self) -> FakeHost {
        FakeHost::with_pull("main-tip", "snapshot", &self.sent)
    }

    fn open(&self) -> OpenedReview {
        open_review(&self.subject, self.repo.path(), &self.repo.folders()).unwrap()
    }

    /// The window comes to the front, and what the review wants of GitHub is done.
    fn focus(&self, opened: &mut OpenedReview, host: &FakeHost, now: Instant) {
        opened.review.pull_from_host();
        drive_without_agents(&mut opened.review, host, opened.source.as_ref(), now);
    }
}

fn offer_of(pr: &str, title: &str) -> MoveOffer {
    MoveOffer {
        pr: pr.parse().unwrap(),
        title: title.to_owned(),
    }
}

fn move_offer(review: &Review) -> Option<MoveOffer> {
    match review.branch_offer()? {
        BranchOffer::MoveToPr(offer) => Some(offer),
        _ => None,
    }
}

fn move_here_offer(review: &Review) -> Option<MoveHereOffer> {
    match review.branch_offer()? {
        BranchOffer::MoveHere(offer) => Some(offer),
        _ => None,
    }
}

/// The Keep button, on whichever side's offer, written without a problem.
fn keep_the_local_review(review: &mut Review) {
    review.decline_the_move();
    assert!(
        !review
            .problems()
            .iter()
            .any(|problem| matches!(problem, Problem::MoveNotDeclined(_))),
        "{:?}",
        review.problems()
    );
}

#[test]
fn a_local_review_offers_to_move_to_the_open_pr_whose_head_is_its_branch() {
    let feature = Feature::with_work_in_progress();
    let host = feature
        .host()
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    let mut opened = feature.open();
    assert_eq!(move_offer(&opened.review), None);

    feature.focus(&mut opened, &host, Instant::now());

    let offer = move_offer(&opened.review).unwrap();
    assert_eq!(offer, offer_of(SHOP_7, "Add the cart"));
    assert_eq!(offer.label(), "Move to acme/shop#7");
    assert_eq!(feature.sent.branch_lookups(), ["acme/shop:feature"]);
}

#[test]
fn there_is_no_offer_without_a_pr_from_the_branch_or_with_one_from_another_branch() {
    let feature = Feature::with_work_in_progress();
    let host = feature
        .host()
        .with_open_pull_from("another", SHOP_7, "Another change");
    let mut opened = feature.open();

    feature.focus(&mut opened, &host, Instant::now());

    assert_eq!(move_offer(&opened.review), None);
    assert_eq!(feature.sent.branch_lookups(), ["acme/shop:feature"]);
    assert_eq!(feature.sent.host_calls(), []);
}

#[test]
fn a_look_that_fails_is_silent_and_tried_again_later() {
    let feature = Feature::with_work_in_progress();
    let host = feature
        .host()
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    host.inject(HostFault::ReadsFail);
    let mut opened = feature.open();
    let now = Instant::now();

    feature.focus(&mut opened, &host, now);

    assert_eq!(move_offer(&opened.review), None);
    assert_eq!(opened.review.problems(), []);
    host.heal(&HostFault::ReadsFail);

    drive_without_agents(
        &mut opened.review,
        &host,
        opened.source.as_ref(),
        now + PULL_OF_BRANCH_RETRY + Duration::from_secs(1),
    );

    assert_eq!(
        move_offer(&opened.review),
        Some(offer_of(SHOP_7, "Add the cart"))
    );
}

#[test]
fn a_pr_review_looks_for_no_branch() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let sent = SentLog::default();
    let host = FakeHost::with_pull("main-tip", "head", &sent).with_open_pull_from(
        "feature",
        SHOP_7,
        "Add the cart",
    );
    repo.write("a.txt", "one\n");
    let base = repo.commit("base");
    repo.write("a.txt", "two\n");
    let head = repo.commit("head");
    let pr: PullRequestUrl = SHOP_7.parse().unwrap();
    let tour = guided_review::tour::BasicTour::new(
        serde_json::from_value(serde_json::json!({
            "pr": SHOP_7, "base": base, "head": head,
        }))
        .unwrap(),
        "Orders cost two".to_owned(),
    );
    let source = guided_review::local_git::LocalGit::find(&pr.repo, repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_subject(&repo.folders(), &ReviewSubject::from(pr));
    let mut review = Review::open_basic(
        tour,
        &source,
        state_file,
        crate::support::fake_ports::every_capability(),
    )
    .unwrap();

    review.pull_from_host();
    drive_without_agents(&mut review, &host, &source, Instant::now());

    assert_eq!(review.branch_offer(), None);
    assert_eq!(sent.branch_lookups(), Vec::<String>::new());
}

#[test]
fn the_github_adapter_lists_the_open_pr_of_a_branch_of_the_repo_itself() {
    let listed =
        r#"[{"number":7,"title":"Add the cart","html_url":"https://github.com/acme/shop/pull/7"}]"#;
    let url = serve_files(vec![
        (
            "/repos/acme/shop/pulls?head=acme%3Afeature/cart&state=open",
            listed.as_bytes().to_vec(),
        ),
        (
            "/repos/acme/shop/pulls?head=acme%3Aquiet&state=open",
            b"[]".to_vec(),
        ),
    ]);
    let host = GitHub::without_token_at(&url);
    let repo = HostedRepo {
        host: HostSite::GitHub,
        repo: RepoName::new("acme", "shop"),
    };

    let found = host
        .open_pull_of_branch(&repo, &BranchName::new("feature/cart").unwrap())
        .unwrap();
    let none = host
        .open_pull_of_branch(&repo, &BranchName::new("quiet").unwrap())
        .unwrap();

    assert_eq!(
        found,
        Some(PullOfBranch {
            pr: SHOP_7.parse().unwrap(),
            title: "Add the cart".to_owned(),
        })
    );
    assert_eq!(none, None);
}

#[test]
fn the_github_adapter_names_the_branch_a_pr_is_opened_from_unless_it_is_from_a_fork() {
    let own = r#"{"head":{"ref":"feature/cart","sha":"a","repo":{"full_name":"Acme/Shop"}}}"#;
    let fork = r#"{"head":{"ref":"patch","sha":"b","repo":{"full_name":"someone/shop"}}}"#;
    let url = serve_files(vec![
        ("/repos/acme/shop/pulls/7", own.as_bytes().to_vec()),
        ("/repos/acme/shop/pulls/8", fork.as_bytes().to_vec()),
    ]);
    let host = GitHub::without_token_at(&url);

    let own = host
        .pull_request_head_branch(&SHOP_7.parse().unwrap())
        .unwrap();
    let fork = host
        .pull_request_head_branch(&"https://github.com/acme/shop/pull/8".parse().unwrap())
        .unwrap();

    assert_eq!(own, Some(BranchName::new("feature/cart").unwrap()));
    assert_eq!(fork, None);
}

fn path(text: &str) -> guided_review::file_path::FilePath {
    crate::support::file_path(text)
}

/// What the PR's review has of the move: the PR's state file, read from disk.
fn pr_state(feature: &Feature) -> guided_review::viewer::ReviewState {
    ReviewStateFile::for_subject(
        &feature.repo.folders(),
        &ReviewSubject::from(SHOP_7.parse::<PullRequestUrl>().unwrap()),
    )
    .load()
    .unwrap()
}

impl Feature {
    fn snapshot_head(&self) -> Sha {
        guided_review::tour::BasicTour::load(&Tour::basic_path_for(
            &self.repo.folders(),
            &self.subject,
        ))
        .unwrap()
        .pin
        .head
    }

    /// The reviewer commits the work and goes on with the cart, and a PR is opened from the branch.
    /// Returns GitHub as it is then, and the PR's head.
    fn open_a_pr_after_one_more_commit(&self) -> (FakeHost, Sha) {
        self.repo
            .write("src/cart.rs", "pub fn total() {}\npub fn tax() {}\n");
        let head = Sha::new(&self.repo.commit("Finish the cart"));
        let main = self.repo.git(&["rev-parse", "main"]);
        let host = FakeHost::with_pull(main.trim(), head.as_str(), &self.sent).with_open_pull_from(
            "feature",
            SHOP_7,
            "Add the cart",
        );
        (host, head)
    }
}

#[test]
fn moving_carries_the_tick_of_a_file_whose_code_is_the_same_and_shows_what_changed_for_one_that_changed()
 {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    opened.review.tick_file(&path("README.md")).unwrap();
    opened.review.tick_file(&path("src/cart.rs")).unwrap();
    let read_at = feature.snapshot_head();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &host, Instant::now());
    let offer = move_offer(&opened.review).unwrap();

    feature
        .mover_of(feature.subject.local().unwrap())
        .run(&offer, &host)
        .unwrap();

    let state = pr_state(&feature);
    assert_eq!(
        state.file_read_tick(&path("README.md")).unwrap().head,
        pr_head
    );
    assert_eq!(state.file_read_tick(&path("src/cart.rs")), None);
    assert_eq!(
        state.file_read_before(&path("src/cart.rs")).unwrap().head,
        read_at
    );
}

impl Feature {
    /// The local review ticks both files, a PR is opened after one more commit to the cart, and the
    /// review moves to it: the cart's tick becomes a mark of what it was read at, the snapshot.
    fn moved_with_the_cart_read_at_the_snapshot(&self) -> Sha {
        let mut opened = self.open();
        opened.review.tick_file(&path("README.md")).unwrap();
        opened.review.tick_file(&path("src/cart.rs")).unwrap();
        let read_at = self.snapshot_head();
        let (host, _) = self.open_a_pr_after_one_more_commit();
        self.mover_of(self.subject.local().unwrap())
            .run(&offer_of(SHOP_7, "Add the cart"), &host)
            .unwrap();
        read_at
    }

    /// `git gc` as it runs once two weeks have gone by.
    fn collect_garbage(&self) {
        self.repo
            .git(&["reflog", "expire", "--expire=now", "--all"]);
        self.repo.git(&["gc", "--quiet", "--prune=now"]);
    }

    fn has_commit(&self, sha: &Sha) -> bool {
        self.repo
            .git(&[
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname)",
            ])
            .lines()
            .any(|object| object == sha.as_str())
    }

    fn open_the_moved_pr(&self) -> OpenedReview {
        open_review(
            &ReviewSubject::from(SHOP_7.parse::<PullRequestUrl>().unwrap()),
            self.repo.path(),
            &self.repo.folders(),
        )
        .unwrap()
    }
}

#[test]
fn the_snapshot_a_moved_file_was_read_at_outlives_git_gc_while_the_pr_review_shows_what_changed_since()
 {
    let feature = Feature::with_work_in_progress();
    let read_at = feature.moved_with_the_cart_read_at_the_snapshot();

    feature.collect_garbage();
    drop(feature.open_the_moved_pr());
    feature.collect_garbage();

    assert!(feature.has_commit(&read_at));
    assert_eq!(
        pr_state(&feature)
            .file_read_before(&path("src/cart.rs"))
            .map(|tick| &tick.head),
        Some(&read_at)
    );
}

#[test]
fn once_the_pr_review_reads_from_no_snapshot_the_next_open_lets_git_gc_take_them() {
    let feature = Feature::with_work_in_progress();
    let read_at = feature.moved_with_the_cart_read_at_the_snapshot();
    let mut opened = feature.open_the_moved_pr();
    opened.review.tick_file(&path("src/cart.rs")).unwrap();
    drop(opened);

    drop(feature.open_the_moved_pr());
    feature.collect_garbage();

    assert_eq!(snapshot_refs(&feature), "");
    assert!(!feature.has_commit(&read_at));
}

impl Feature {
    /// The writer's guided tour in place of the basic one, pinned to the same snapshot, with a step of
    /// the README and one of the rest.
    fn with_a_guided_tour_of_the_readme_and_the_rest(&self) {
        let folders = self.repo.folders();
        let basic_path = Tour::basic_path_for(&folders, &self.subject);
        let pin = guided_review::tour::BasicTour::load(&basic_path)
            .unwrap()
            .pin;
        let mut tour = serde_json::to_value(&pin).unwrap();
        let steps = serde_json::json!({
            "title": "Cart",
            "summary": ["A cart"],
            "steps": [
                {"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]},
                {"id": "cart", "title": "Cart", "why": "w", "pieces": [{"rest": ""}]},
            ],
        });
        tour.as_object_mut()
            .unwrap()
            .extend(steps.as_object().unwrap().clone());
        std::fs::write(Tour::path_for(&folders, &self.subject), tour.to_string()).unwrap();
        std::fs::remove_file(basic_path).unwrap();
    }
}

#[test]
fn moving_a_guided_local_review_carries_its_step_ticks_as_file_ticks_by_the_same_rule() {
    let feature = Feature::with_work_in_progress();
    drop(feature.open());
    let read_at = feature.snapshot_head();
    feature.with_a_guided_tour_of_the_readme_and_the_rest();
    ReviewStateFile::for_subject(&feature.repo.folders(), &feature.subject)
        .update(|state| {
            state.tick(&"readme".into(), &read_at);
            state.tick(&"cart".into(), &read_at);
        })
        .unwrap();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();

    feature
        .mover_of(feature.subject.local().unwrap())
        .run(&offer_of(SHOP_7, "Add the cart"), &host)
        .unwrap();

    let state = pr_state(&feature);
    assert_eq!(
        state
            .file_read_tick(&path("README.md"))
            .map(|tick| &tick.head),
        Some(&pr_head)
    );
    assert_eq!(state.file_read_tick(&path("src/cart.rs")), None);
    assert_eq!(
        state
            .file_read_before(&path("src/cart.rs"))
            .map(|tick| &tick.head),
        Some(&read_at)
    );
}

fn line_of(opened: &OpenedReview, text: &str) -> LineLocation {
    opened
        .review
        .file_page()
        .unwrap()
        .pieces
        .iter()
        .flat_map(PieceDiff::lines)
        .find(|row| row.text == text)
        .unwrap_or_else(|| panic!("no row shows {text:?}"))
        .location
        .clone()
}

/// The comment box's Add to tasks, as the app runs it: the task is a thread waiting for Send tasks.
fn add_to_tasks(feature: &Feature, opened: &mut OpenedReview, on: &str, text: &str) {
    let at = line_of(opened, on);
    opened.review.begin_selection(at.clone());
    opened.review.extend_selection(at);
    let place = opened
        .review
        .open_comment_box(CommentGesture::DragEnded)
        .unwrap()
        .unwrap();
    opened
        .review
        .type_in(&TextBox::Draft(place.clone()), text)
        .unwrap();
    let request = opened
        .review
        .start_sending(&place, SendMode::AddToTasks, Audience::Agent)
        .unwrap();
    opened
        .review
        .apply_comment(request.run(&feature.host()))
        .unwrap();
}

fn threads_store(feature: &Feature) -> LocalThreadStore {
    LocalThreadStore::in_file(ReviewStateFile::for_subject(
        &feature.repo.folders(),
        &feature.subject,
    ))
}

fn readme_line(line_number: usize) -> CommentPlace {
    let at = LineLocation {
        path: path("README.md"),
        side: Side::Head,
        line: crate::support::line(line_number),
    };
    CommentPlace::Lines {
        start: at.clone(),
        end: at,
    }
}

type ShownDraft = (String, String, Option<Sha>);

/// The PR's review as the app opens it, with the drafts its comment boxes hold.
fn drafts_of_the_pr(feature: &Feature) -> Vec<ShownDraft> {
    let pr: ReviewSubject = SHOP_7.parse().unwrap();
    let opened = open_review(&pr, feature.repo.path(), &feature.repo.folders()).unwrap();
    crate::support::drafts(&opened.review)
        .into_iter()
        .map(|draft| (draft.place.describe(), draft.body, draft.written_at))
        .collect()
}

#[test]
fn moving_makes_drafts_of_open_threads_and_queued_tasks_none_of_resolved_ones_and_posts_nothing() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    opened.review.go_to_file(&path("README.md"));
    let snapshot = feature.snapshot_head();
    let store = threads_store(&feature);
    let asked = store
        .start(&readme_line(2), "Say who the world is", &snapshot)
        .unwrap();
    store
        .reply(&asked, "The one you live in", LocalAuthor::Agent)
        .unwrap();
    let settled = store
        .start(&readme_line(1), "Greet nicer", &snapshot)
        .unwrap();
    store.set_resolved(&settled, Resolution::Resolved).unwrap();
    opened.review.go_to_file(&path("src/cart.rs"));
    add_to_tasks(&feature, &mut opened, "pub fn total() {}", "Shout it");
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &host, Instant::now());
    let offer = move_offer(&opened.review).unwrap();

    feature
        .mover_of(feature.subject.local().unwrap())
        .run(&offer, &host)
        .unwrap();

    assert_eq!(
        drafts_of_the_pr(&feature),
        [
            (
                "Comment on README.md line 2".to_owned(),
                "Say who the world is".to_owned(),
                Some(pr_head.clone())
            ),
            (
                "Comment on src/cart.rs line 1".to_owned(),
                "Shout it".to_owned(),
                Some(pr_head)
            ),
        ]
    );
    assert_eq!(
        feature.sent.host_calls(),
        [HostCall::TipsOf(SHOP_7.parse().unwrap())]
    );
}

fn snapshot_refs(feature: &Feature) -> String {
    feature
        .repo
        .git(&["for-each-ref", "--format=%(refname)", "refs/guided-review"])
}

fn local_review_is_on_disk(feature: &Feature) -> bool {
    ToursOnDisk::of(&feature.repo.folders(), &feature.subject) != ToursOnDisk::None
        && ReviewStateFile::for_subject(&feature.repo.folders(), &feature.subject).exists()
        && !snapshot_refs(feature).is_empty()
}

fn pr_state_path(feature: &Feature) -> std::path::PathBuf {
    feature
        .repo
        .home()
        .join(".local/state/guided-review/state/acme/shop/7.json")
}

#[test]
fn the_local_review_and_its_ref_go_only_once_the_prs_state_is_saved() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    opened.review.tick_file(&path("README.md")).unwrap();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &host, Instant::now());
    let offer = move_offer(&opened.review).unwrap();
    let mover = feature.mover_of(feature.subject.local().unwrap());
    assert!(local_review_is_on_disk(&feature));

    mover.carry(&offer, &host).unwrap();

    assert!(
        local_review_is_on_disk(&feature),
        "carrying deletes nothing"
    );
    assert_eq!(
        pr_state(&feature)
            .file_read_tick(&path("README.md"))
            .unwrap()
            .head,
        pr_head
    );

    mover.forget_local().unwrap();

    assert!(!local_review_is_on_disk(&feature));
    assert_eq!(
        ToursOnDisk::of(&feature.repo.folders(), &feature.subject),
        ToursOnDisk::None
    );
    assert!(!ReviewStateFile::for_subject(&feature.repo.folders(), &feature.subject).exists());
    assert_eq!(snapshot_refs(&feature), "");
}

#[test]
fn a_pr_state_that_cannot_be_saved_keeps_the_local_review_whole() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    opened.review.tick_file(&path("README.md")).unwrap();
    let (host, _) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &host, Instant::now());
    let offer = move_offer(&opened.review).unwrap();
    std::fs::create_dir_all(pr_state_path(&feature).parent().unwrap()).unwrap();
    std::fs::write(pr_state_path(&feature), "{ not json").unwrap();

    let moved = feature
        .mover_of(feature.subject.local().unwrap())
        .run(&offer, &host);

    assert!(moved.is_err());
    assert!(local_review_is_on_disk(&feature));
    assert_eq!(
        ReviewStateFile::for_subject(&feature.repo.folders(), &feature.subject)
            .load()
            .unwrap()
            .file_read_tick(&path("README.md"))
            .unwrap()
            .head,
        feature.snapshot_head()
    );
}

#[test]
fn a_move_that_crashed_after_saving_carries_again_without_doubling_drafts_or_undoing_newer_ticks() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    opened.review.go_to_file(&path("README.md"));
    opened.review.tick_file(&path("src/cart.rs")).unwrap();
    threads_store(&feature)
        .start(
            &readme_line(2),
            "Say who the world is",
            &feature.snapshot_head(),
        )
        .unwrap();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &host, Instant::now());
    let offer = move_offer(&opened.review).unwrap();
    let mover = feature.mover_of(feature.subject.local().unwrap());
    mover.carry(&offer, &host).unwrap();
    assert!(
        pr_state(&feature)
            .file_read_before(&path("src/cart.rs"))
            .is_some()
    );
    // The reviewer reads the cart in the PR's review before the app is back.
    ReviewStateFile::for_subject(
        &feature.repo.folders(),
        &ReviewSubject::from(offer.pr.clone()),
    )
    .update(|state| state.tick_file(&path("src/cart.rs"), &pr_head))
    .unwrap();

    mover.run(&offer, &host).unwrap();

    let state = pr_state(&feature);
    assert_eq!(
        state.file_read_tick(&path("src/cart.rs")).unwrap().head,
        pr_head
    );
    assert_eq!(state.file_read_before(&path("src/cart.rs")), None);
    assert_eq!(
        drafts_of_the_pr(&feature),
        [(
            "Comment on README.md line 2".to_owned(),
            "Say who the world is".to_owned(),
            Some(pr_head)
        )]
    );
}

#[test]
fn a_thread_follows_its_lines_to_the_prs_head_and_one_whose_file_is_gone_becomes_a_file_draft() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let snapshot = feature.snapshot_head();
    let store = threads_store(&feature);
    store
        .start(&readme_line(2), "Say who the world is", &snapshot)
        .unwrap();
    let on_the_cart = LineLocation {
        path: path("src/cart.rs"),
        side: Side::Head,
        line: crate::support::line(1),
    };
    store
        .start(
            &CommentPlace::Lines {
                start: on_the_cart.clone(),
                end: on_the_cart,
            },
            "Why a cart",
            &snapshot,
        )
        .unwrap();
    feature.repo.write("README.md", "intro\nhello\nworld\n");
    std::fs::remove_file(feature.repo.path().join("src/cart.rs")).unwrap();
    let head = Sha::new(&feature.repo.commit("Rewrite the readme and drop the cart"));
    let main = feature.repo.git(&["rev-parse", "main"]);
    let host = FakeHost::with_pull(main.trim(), head.as_str(), &feature.sent).with_open_pull_from(
        "feature",
        SHOP_7,
        "Add the cart",
    );
    feature.focus(&mut opened, &host, Instant::now());
    let offer = move_offer(&opened.review).unwrap();

    feature
        .mover_of(feature.subject.local().unwrap())
        .run(&offer, &host)
        .unwrap();

    assert_eq!(
        drafts_of_the_pr(&feature),
        [
            (
                "Comment on README.md line 3".to_owned(),
                "Say who the world is".to_owned(),
                Some(head.clone())
            ),
            (
                "Comment on src/cart.rs".to_owned(),
                "Why a cart".to_owned(),
                Some(head)
            ),
        ]
    );
}

#[test]
fn declining_hides_the_offer_for_that_pr_for_good_and_keeps_the_local_review() {
    let feature = Feature::with_work_in_progress();
    let host = feature
        .host()
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    let mut opened = feature.open();
    feature.focus(&mut opened, &host, Instant::now());

    keep_the_local_review(&mut opened.review);

    assert_eq!(move_offer(&opened.review), None);
    assert!(local_review_is_on_disk(&feature));
    let mut reopened = feature.open();
    feature.focus(&mut reopened, &host, Instant::now());
    assert_eq!(move_offer(&reopened.review), None);
    let another = feature.host().with_open_pull_from(
        "feature",
        "https://github.com/acme/shop/pull/8",
        "Again",
    );
    feature.focus(&mut reopened, &another, Instant::now());
    assert_eq!(
        move_offer(&reopened.review).map(|offer| offer.label()),
        Some("Move to acme/shop#8".to_owned())
    );
}

impl Feature {
    /// The move of a local review, reading the PR's commits from the repo of the fixture.
    fn mover_of(&self, change_set: &LocalChangeSet) -> PrMover {
        let repo_path = self.repo.path().to_path_buf();
        PrMover {
            folders: self.repo.folders(),
            change_set: change_set.clone(),
            pick_code_source: std::sync::Arc::new(move |pr, _| {
                let clone = guided_review::local_git::LocalGit::find(&pr.repo, &repo_path)?
                    .context("the fixture has a clone")?;
                Ok(Box::new(clone))
            }),
            drop_local_review: std::sync::Arc::new(|change_set, folders| {
                guided_review::opened_subject::OpenedChangeSet::new(change_set, folders)
                    .drop_review()
            }),
        }
    }

    /// The PR's review is open, as when the reviewer opens the PR from the switcher: its basic tour is
    /// on disk, made at the PR's tips.
    fn open_the_pr(&self, host: &FakeHost, head: &Sha) -> OpenedReview {
        let pr: PullRequestUrl = SHOP_7.parse().unwrap();
        let source = guided_review::local_git::LocalGit::find(&pr.repo, self.repo.path())
            .unwrap()
            .unwrap();
        let main = self.repo.git(&["rev-parse", "main"]);
        make_basic_tour(
            &self.repo.folders(),
            &ReviewSubject::from(pr.clone()),
            &source,
            &guided_review::pull_request::ChangeTips {
                base_branch_tip: Sha::new(main.trim()),
                head: head.clone(),
            },
            "Add the cart",
        )
        .unwrap();
        let mut opened = open_review(
            &ReviewSubject::from(pr),
            self.repo.path(),
            &self.repo.folders(),
        )
        .unwrap();
        self.focus(&mut opened, host, Instant::now());
        opened
    }
}

#[test]
fn a_pr_review_offers_to_take_over_the_local_review_of_the_branch_it_is_opened_from() {
    let feature = Feature::with_work_in_progress();
    drop(feature.open());
    let (host, head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");

    let opened = feature.open_the_pr(&host, &head);

    let offer = move_here_offer(&opened.review).unwrap();
    assert_eq!(offer.change_set, *feature.subject.local().unwrap());
    assert_eq!(offer.label(), "Move feature here");
    assert_eq!(offer.offer.pr, SHOP_7.parse().unwrap());
    assert!(
        feature
            .sent
            .branch_lookups()
            .contains(&"head of acme/shop#7".to_owned())
    );
}

#[test]
fn a_pr_review_offers_nothing_for_another_branch_or_without_a_local_review_and_asks_nothing_then() {
    let other_branch = Feature::with_work_in_progress();
    drop(other_branch.open());
    let (host, head) = other_branch.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "another");
    let opened = other_branch.open_the_pr(&host, &head);
    assert_eq!(move_here_offer(&opened.review), None);

    let no_local_review = Feature::with_work_in_progress();
    let (host, head) = no_local_review.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    opened_without_a_local_review(&no_local_review);
    let opened = no_local_review.open_the_pr(&host, &head);
    assert_eq!(move_here_offer(&opened.review), None);
}

fn opened_without_a_local_review(feature: &Feature) {
    // The local review was never opened, so none is on disk: nothing is looked up for the PR's branch.
    assert!(!local_review_is_on_disk(feature));
}

#[test]
fn moving_into_an_open_pr_review_keeps_its_earlier_tick_and_the_moved_ticks_and_drafts_through_its_next_save()
 {
    let feature = Feature::with_work_in_progress();
    let mut local = feature.open();
    local.review.go_to_file(&path("README.md"));
    local.review.tick_file(&path("README.md")).unwrap();
    local.review.tick_file(&path("src/cart.rs")).unwrap();
    threads_store(&feature)
        .start(
            &readme_line(2),
            "Say who the world is",
            &feature.snapshot_head(),
        )
        .unwrap();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    let mut opened = feature.open_the_pr(&host, &pr_head);
    opened.review.tick_file(&path("src/cart.rs")).unwrap();
    let offer = move_here_offer(&opened.review).unwrap();

    assert!(start_move(&mut opened.review).is_some());
    feature
        .mover_of(&offer.change_set)
        .run(&offer.offer, &host)
        .unwrap();
    assert!(!local_review_is_on_disk(&feature));
    assert!(
        opened.review.tick_file(&path("README.md")).is_err(),
        "the open review's stale copy takes no more changes"
    );

    let mut reopened = feature.open_the_pr(&host, &pr_head);
    reopened.review.tick_file(&path("README.md")).unwrap();
    let state = pr_state(&feature);
    assert_eq!(
        state.file_read_tick(&path("README.md")).unwrap().head,
        pr_head
    );
    assert_eq!(
        state.file_read_tick(&path("src/cart.rs")).unwrap().head,
        pr_head
    );
    assert_eq!(state.file_read_before(&path("src/cart.rs")), None);
    assert_eq!(
        drafts_of_the_pr(&feature),
        [(
            "Comment on README.md line 2".to_owned(),
            "Say who the world is".to_owned(),
            Some(pr_head)
        )]
    );
}

#[test]
fn keeping_the_local_review_hides_the_pr_reviews_offer_and_the_local_ones_too() {
    let feature = Feature::with_work_in_progress();
    let local = feature.open();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    let mut opened = feature.open_the_pr(&host, &pr_head);
    assert!(move_here_offer(&opened.review).is_some());

    keep_the_local_review(&mut opened.review);

    assert_eq!(move_here_offer(&opened.review), None);
    let reopened = feature.open_the_pr(&host, &pr_head);
    assert_eq!(move_here_offer(&reopened.review), None);
    drop(local);
    let mut local = feature.open();
    feature.focus(&mut local, &host, Instant::now());
    assert_eq!(move_offer(&local.review), None);
    assert!(local_review_is_on_disk(&feature));
}

fn head_branch_lookups(feature: &Feature) -> usize {
    feature
        .sent
        .branch_lookups()
        .iter()
        .filter(|lookup| *lookup == "head of acme/shop#7")
        .count()
}

#[test]
fn a_pr_review_asks_github_for_its_head_branch_once_and_not_again_when_it_comes_to_the_front() {
    let feature = Feature::with_work_in_progress();
    drop(feature.open());
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    let mut opened = feature.open_the_pr(&host, &pr_head);

    feature.focus(&mut opened, &host, Instant::now());
    feature.focus(&mut opened, &host, Instant::now());

    assert_eq!(head_branch_lookups(&feature), 1);
    assert!(move_here_offer(&opened.review).is_some());
}

#[test]
fn a_failed_look_up_of_the_head_branch_is_tried_again_later_and_later_but_not_on_focus() {
    let feature = Feature::with_work_in_progress();
    drop(feature.open());
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    host.inject(HostFault::ReadsFail);
    let opened_at = Instant::now();
    let mut opened = feature.open_the_pr(&host, &pr_head);
    let drive_at = |opened: &mut OpenedReview, after: Duration| {
        drive_without_agents(
            &mut opened.review,
            &host,
            opened.source.as_ref(),
            opened_at + after,
        );
    };
    let a_second = Duration::from_secs(1);

    drive_at(&mut opened, PULL_OF_BRANCH_RETRY + a_second);
    assert_eq!(head_branch_lookups(&feature), 2);
    feature.focus(
        &mut opened,
        &host,
        opened_at + PULL_OF_BRANCH_RETRY * 2 + a_second,
    );
    assert_eq!(
        head_branch_lookups(&feature),
        2,
        "not on focus, nor before the longer wait"
    );

    host.heal(&HostFault::ReadsFail);
    drive_at(&mut opened, PULL_OF_BRANCH_RETRY * 3 + a_second * 2);
    assert_eq!(head_branch_lookups(&feature), 3);
    assert!(move_here_offer(&opened.review).is_some());
}

#[test]
fn a_pr_review_finds_the_local_review_of_its_repo_by_its_tour_file_without_asking_the_clone() {
    let feature = Feature::with_work_in_progress();
    drop(feature.open());
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    // The clone's origin now names another repo; only the local tour file still says acme/shop.
    feature
        .repo
        .git(&["remote", "rename", "origin", "upstream"]);
    feature.repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/acme/elsewhere",
    ]);

    let opened = feature.open_the_pr(&host, &pr_head);

    assert!(move_here_offer(&opened.review).is_some());
}

#[test]
fn a_kept_local_review_is_not_offered_again_when_the_pr_review_comes_back_to_the_front() {
    let feature = Feature::with_work_in_progress();
    drop(feature.open());
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    let mut opened = feature.open_the_pr(&host, &pr_head);

    keep_the_local_review(&mut opened.review);
    feature.focus(&mut opened, &host, Instant::now());

    assert_eq!(move_here_offer(&opened.review), None);
}

#[test]
fn a_local_tour_pinned_to_another_folder_is_refused_by_the_move_and_nothing_is_carried() {
    let feature = Feature::with_work_in_progress();
    drop(feature.open());
    let (host, _) = feature.open_a_pr_after_one_more_commit();
    let tour_path = Tour::shown_path_for(&feature.repo.folders(), &feature.subject);
    let other = "local:/Users/me/Other/shop@feature";
    let pinned = std::fs::read_to_string(&tour_path).unwrap();
    std::fs::write(
        &tour_path,
        pinned.replace(&feature.subject.to_string(), other),
    )
    .unwrap();

    let moved = feature
        .mover_of(feature.subject.local().unwrap())
        .run(&offer_of(SHOP_7, "Add the cart"), &host);

    let refused = format!("{:#}", moved.unwrap_err());
    assert!(refused.contains(other), "{refused}");
    assert!(!pr_state_path(&feature).exists());
}

fn start_move(review: &mut Review) -> Option<StartedMove> {
    match review.start_job(ReplacingJob::MoveToPr)? {
        StartedJob::MoveToPr(started) => Some(started),
        other => panic!("{other:?} started instead of the move"),
    }
}

#[test]
fn the_move_button_starts_the_move_the_review_offers_and_the_review_takes_no_changes_from_then() {
    let feature = Feature::with_work_in_progress();
    let mut local = feature.open();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut local, &host, Instant::now());

    let started = start_move(&mut local.review).unwrap();

    assert_eq!(started.from, MoveFrom::TheLocalReview);
    assert_eq!(started.offer.pr, SHOP_7.parse().unwrap());
    assert_eq!(started.change_set, *feature.subject.local().unwrap());
    assert!(local.review.tick_file(&path("README.md")).is_err());

    let host = host.with_head_branch(SHOP_7, "feature");
    let mut in_the_pr = feature.open_the_pr(&host, &pr_head);

    let started = start_move(&mut in_the_pr.review).unwrap();

    assert_eq!(started.from, MoveFrom::ThePr);
    assert_eq!(started.change_set, *feature.subject.local().unwrap());
    assert!(in_the_pr.review.tick_file(&path("README.md")).is_err());
}

#[test]
fn the_move_button_starts_nothing_without_an_offer() {
    let feature = Feature::with_work_in_progress();
    let mut local = feature.open();

    assert_eq!(start_move(&mut local.review), None);
    local.review.tick_file(&path("README.md")).unwrap();
}

#[test]
fn a_failed_move_says_which_move_it_was_and_why_and_the_review_takes_changes_again() {
    let feature = Feature::with_work_in_progress();
    let mut local = feature.open();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut local, &host, Instant::now());
    let failed = || JobOutcome::MoveToPr(Err(anyhow::anyhow!("no network")));

    start_move(&mut local.review).unwrap();
    let AfterJob::Stays { toast: Some(toast) } = local.review.land_job(failed()) else {
        panic!("nothing is told");
    };
    assert_eq!(toast, "Could not move to acme/shop#7: no network");
    local.review.tick_file(&path("README.md")).unwrap();

    let host = host.with_head_branch(SHOP_7, "feature");
    let mut in_the_pr = feature.open_the_pr(&host, &pr_head);
    start_move(&mut in_the_pr.review).unwrap();
    let AfterJob::Stays { toast: Some(toast) } = in_the_pr.review.land_job(failed()) else {
        panic!("nothing is told");
    };
    assert_eq!(toast, "Could not move feature here: no network");
}

#[test]
fn after_a_move_from_the_local_review_it_is_gone_and_the_pr_is_shown() {
    let feature = Feature::with_work_in_progress();
    let mut local = feature.open();
    let (host, _) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut local, &host, Instant::now());
    start_move(&mut local.review).unwrap();

    let after = local.review.land_job(JobOutcome::MoveToPr(Ok(())));

    let AfterJob::ShowPr { moved_away, pr } = after else {
        panic!("the PR is not shown");
    };
    assert_eq!(moved_away, feature.subject);
    assert_eq!(pr, SHOP_7.parse().unwrap());
}

#[test]
fn after_a_move_from_the_pr_side_the_local_review_is_dropped_and_the_pr_reopened() {
    let feature = Feature::with_work_in_progress();
    feature.open();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    let mut in_the_pr = feature.open_the_pr(&host, &pr_head);
    start_move(&mut in_the_pr.review).unwrap();

    let after = in_the_pr.review.land_job(JobOutcome::MoveToPr(Ok(())));

    let AfterJob::DropAndReopen(dropped) = after else {
        panic!("the PR is not reopened");
    };
    assert_eq!(dropped, feature.subject);
}

#[test]
fn update_and_not_now_are_refused_while_a_move_runs() {
    let feature = Feature::with_work_in_progress();
    let mut local = feature.open();
    let (host, _) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut local, &host, Instant::now());
    start_move(&mut local.review).unwrap();

    assert_eq!(local.review.start_job(ReplacingJob::Repin), None);
    local.review.decline_the_move();

    assert!(move_offer(&local.review).is_some(), "the offer stays");
    assert!(local.review.job_is_running(ReplacingJob::MoveToPr));
}

#[test]
fn a_reload_asked_for_while_a_move_runs_never_runs_once_the_review_is_gone() {
    let feature = Feature::with_work_in_progress();
    let mut local = feature.open();
    let (host, _) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut local, &host, Instant::now());
    start_move(&mut local.review).unwrap();
    assert_eq!(local.review.start_job(ReplacingJob::Reload), None);

    local.review.land_job(JobOutcome::MoveToPr(Ok(())));

    assert_eq!(local.review.due_job(), None);
}

#[test]
fn a_decline_that_could_not_be_kept_is_a_problem_the_reviewer_sees_and_one_that_was_kept_is_not() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let (host, _) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &host, Instant::now());
    let state_path = local_state_path(&feature);
    std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
    std::fs::write(&state_path, "{ not json").unwrap();

    opened.review.decline_the_move();

    assert!(
        opened
            .review
            .problems()
            .iter()
            .any(|problem| matches!(problem, Problem::MoveNotDeclined(_))),
        "{:?}",
        opened.review.problems()
    );

    std::fs::remove_file(&state_path).unwrap();
    opened.review.decline_the_move();

    assert!(
        !opened
            .review
            .problems()
            .iter()
            .any(|problem| matches!(problem, Problem::MoveNotDeclined(_)))
    );
}

fn local_state_path(feature: &Feature) -> std::path::PathBuf {
    feature
        .repo
        .folders()
        .state_files()
        .join(feature.subject.files_folder())
        .join(format!("{}.json", feature.subject.file_stem()))
}

#[test]
fn keeping_the_local_review_from_the_pr_side_that_could_not_be_written_is_a_problem_and_the_offer_stays()
 {
    let feature = Feature::with_work_in_progress();
    drop(feature.open());
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    let mut opened = feature.open_the_pr(&host, &pr_head);
    let offered = opened.review.branch_offer();
    assert!(matches!(offered, Some(BranchOffer::MoveHere(_))));
    let local_state = local_state_path(&feature);
    std::fs::create_dir_all(local_state.parent().unwrap()).unwrap();
    std::fs::write(&local_state, "{ not json").unwrap();

    opened.review.decline_the_move();

    assert!(
        opened
            .review
            .problems()
            .iter()
            .any(|problem| matches!(problem, Problem::MoveNotDeclined(_))),
        "{:?}",
        opened.review.problems()
    );
    assert_eq!(opened.review.branch_offer(), offered);
}

#[test]
fn a_rebuilt_review_keeps_what_github_said_of_its_branch_and_asks_it_nothing_again() {
    let feature = Feature::with_work_in_progress();
    let mut local = feature.open();
    let (host, pr_head) = feature.open_a_pr_after_one_more_commit();
    let host = host.with_head_branch(SHOP_7, "feature");
    feature.focus(&mut local, &host, Instant::now());
    let offered = local.review.branch_offer();
    assert!(matches!(offered, Some(BranchOffer::MoveToPr(_))));

    let mut rebuilt = feature.open();
    rebuilt.review.carry_on_from(local.review);
    drive_without_agents(
        &mut rebuilt.review,
        &host,
        rebuilt.source.as_ref(),
        Instant::now(),
    );

    assert_eq!(rebuilt.review.branch_offer(), offered);
    assert_eq!(feature.sent.branch_lookups(), ["acme/shop:feature"]);

    let in_the_pr = feature.open_the_pr(&host, &pr_head);
    let offered = in_the_pr.review.branch_offer();
    assert!(matches!(offered, Some(BranchOffer::MoveHere(_))));
    let pr: PullRequestUrl = SHOP_7.parse().unwrap();
    let mut rebuilt = open_review(
        &ReviewSubject::from(pr),
        feature.repo.path(),
        &feature.repo.folders(),
    )
    .unwrap();
    rebuilt.review.carry_on_from(in_the_pr.review);
    drive_without_agents(
        &mut rebuilt.review,
        &host,
        rebuilt.source.as_ref(),
        Instant::now(),
    );

    assert_eq!(rebuilt.review.branch_offer(), offered);
    assert_eq!(head_branch_lookups(&feature), 1);
}
