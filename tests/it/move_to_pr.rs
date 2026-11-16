//! A local review finds the PR opened from its branch and offers to move to it.

use std::time::{Duration, Instant};

use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use crate::support::work::drive_without_agents;
use crate::support::{FixtureRepo, serve_files};
use guided_review::github::GitHub;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::review_subject::{BranchName, ReviewSubject};
use guided_review::tour::ToursOnDisk;
use guided_review::viewer::{
    Audience, CommentGesture, CommentPlace, LineLocation, LocalAuthor, LocalThreadStore, MoveOffer,
    PULL_OF_BRANCH_RETRY, PieceDiff, PullOfBranch, PullsOfBranchPort, Resolution, ReviewStateFile,
    SendMode, Side, TextBox,
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

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", "snapshot", &self.sent)
    }

    fn open(&self) -> OpenedReview {
        open_review(&self.subject, self.repo.path(), &self.repo.folders()).unwrap()
    }

    /// The window comes to the front, and what the review wants of GitHub is done.
    fn focus(&self, opened: &mut OpenedReview, github: &FakeGitHub, now: Instant) {
        opened.review.pull_from_github();
        drive_without_agents(&mut opened.review, github, opened.source.as_ref(), now);
    }
}

fn offer_of(pr: &str, title: &str) -> MoveOffer {
    MoveOffer {
        pr: pr.parse().unwrap(),
        title: title.to_owned(),
    }
}

#[test]
fn a_local_review_offers_to_move_to_the_open_pr_whose_head_is_its_branch() {
    let feature = Feature::with_work_in_progress();
    let github = feature
        .github()
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    let mut opened = feature.open();
    assert_eq!(opened.review.move_offer(), None);

    feature.focus(&mut opened, &github, Instant::now());

    let offer = opened.review.move_offer().unwrap();
    assert_eq!(offer, offer_of(SHOP_7, "Add the cart"));
    assert_eq!(offer.label(), "Move to acme/shop#7");
    assert_eq!(feature.sent.branch_lookups(), ["acme/shop:feature"]);
}

#[test]
fn there_is_no_offer_without_a_pr_from_the_branch_or_with_one_from_another_branch() {
    let feature = Feature::with_work_in_progress();
    let github = feature
        .github()
        .with_open_pull_from("another", SHOP_7, "Another change");
    let mut opened = feature.open();

    feature.focus(&mut opened, &github, Instant::now());

    assert_eq!(opened.review.move_offer(), None);
    assert_eq!(feature.sent.branch_lookups(), ["acme/shop:feature"]);
    assert_eq!(feature.sent.github_calls(), []);
}

#[test]
fn a_look_that_fails_is_silent_and_tried_again_later() {
    let feature = Feature::with_work_in_progress();
    let github = feature
        .github()
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    github.inject(GitHubFault::ReadsFail);
    let mut opened = feature.open();
    let now = Instant::now();

    feature.focus(&mut opened, &github, now);

    assert_eq!(opened.review.move_offer(), None);
    assert_eq!(opened.review.problems(), []);
    github.heal(&GitHubFault::ReadsFail);

    drive_without_agents(
        &mut opened.review,
        &github,
        opened.source.as_ref(),
        now + PULL_OF_BRANCH_RETRY + Duration::from_secs(1),
    );

    assert_eq!(
        opened.review.move_offer(),
        Some(offer_of(SHOP_7, "Add the cart"))
    );
}

#[test]
fn a_pr_review_looks_for_no_branch() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", "head", &sent).with_open_pull_from(
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
    let state_file = guided_review::viewer::ReviewStateFile::for_subject(
        &repo.folders(),
        &ReviewSubject::from(pr),
    );
    let mut review = guided_review::viewer::Review::open_basic(tour, &source, state_file).unwrap();

    review.pull_from_github();
    drive_without_agents(&mut review, &github, &source, Instant::now());

    assert_eq!(review.move_offer(), None);
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
    let github = GitHub::without_token_at(&url);
    let repo = RepoName::new("acme", "shop");

    let found = github
        .open_pull_of_branch(&repo, &BranchName::new("feature/cart").unwrap())
        .unwrap();
    let none = github
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

fn path(text: &str) -> guided_review::file_path::FilePath {
    crate::support::file_path(text)
}

/// What the PR's review has of the move: the PR's state file, read from disk.
fn pr_state(feature: &Feature) -> guided_review::viewer::ReviewState {
    guided_review::viewer::ReviewStateFile::for_subject(
        &feature.repo.folders(),
        &ReviewSubject::from(SHOP_7.parse::<PullRequestUrl>().unwrap()),
    )
    .load()
    .unwrap()
}

impl Feature {
    fn snapshot_head(&self) -> Sha {
        guided_review::tour::BasicTour::load(&guided_review::tour::Tour::basic_path_for(
            &self.repo.folders(),
            &self.subject,
        ))
        .unwrap()
        .pin
        .head
    }

    /// The reviewer commits the work and goes on with the cart, and a PR is opened from the branch.
    /// Returns GitHub as it is then, and the PR's head.
    fn open_a_pr_after_one_more_commit(&self) -> (FakeGitHub, Sha) {
        self.repo
            .write("src/cart.rs", "pub fn total() {}\npub fn tax() {}\n");
        let head = Sha::new(&self.repo.commit("Finish the cart"));
        let main = self.repo.git(&["rev-parse", "main"]);
        let github = FakeGitHub::with_pull(main.trim(), head.as_str(), &self.sent)
            .with_open_pull_from("feature", SHOP_7, "Add the cart");
        (github, head)
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
    let (github, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &github, Instant::now());
    let offer = opened.review.move_offer().unwrap();

    opened.mover.as_ref().unwrap().run(&offer, &github).unwrap();

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
        .apply_comment(request.run(&feature.github()))
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
    let (github, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &github, Instant::now());
    let offer = opened.review.move_offer().unwrap();

    opened.mover.as_ref().unwrap().run(&offer, &github).unwrap();

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
        feature.sent.github_calls(),
        [GitHubCall::TipsOf(SHOP_7.parse().unwrap())]
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
    let (github, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &github, Instant::now());
    let offer = opened.review.move_offer().unwrap();
    let mover = opened.mover.as_ref().unwrap();
    assert!(local_review_is_on_disk(&feature));

    mover.carry(&offer, &github).unwrap();

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
    let (github, _) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &github, Instant::now());
    let offer = opened.review.move_offer().unwrap();
    std::fs::create_dir_all(pr_state_path(&feature).parent().unwrap()).unwrap();
    std::fs::write(pr_state_path(&feature), "{ not json").unwrap();

    let moved = opened.mover.as_ref().unwrap().run(&offer, &github);

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
    let (github, pr_head) = feature.open_a_pr_after_one_more_commit();
    feature.focus(&mut opened, &github, Instant::now());
    let offer = opened.review.move_offer().unwrap();
    let mover = opened.mover.as_ref().unwrap();
    mover.carry(&offer, &github).unwrap();
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

    mover.run(&offer, &github).unwrap();

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
    let github = FakeGitHub::with_pull(main.trim(), head.as_str(), &feature.sent)
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    feature.focus(&mut opened, &github, Instant::now());
    let offer = opened.review.move_offer().unwrap();

    opened.mover.as_ref().unwrap().run(&offer, &github).unwrap();

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
    let github = feature
        .github()
        .with_open_pull_from("feature", SHOP_7, "Add the cart");
    let mut opened = feature.open();
    feature.focus(&mut opened, &github, Instant::now());

    opened.review.decline_move().unwrap();

    assert_eq!(opened.review.move_offer(), None);
    assert!(local_review_is_on_disk(&feature));
    let mut reopened = feature.open();
    feature.focus(&mut reopened, &github, Instant::now());
    assert_eq!(reopened.review.move_offer(), None);
    let another = feature.github().with_open_pull_from(
        "feature",
        "https://github.com/acme/shop/pull/8",
        "Again",
    );
    feature.focus(&mut reopened, &another, Instant::now());
    assert_eq!(
        reopened.review.move_offer().map(|offer| offer.label()),
        Some("Move to acme/shop#8".to_owned())
    );
}
