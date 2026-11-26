use std::time::Instant;

use crate::support::fake_ports::{FakeHost, SentLog};
use crate::support::work::drive_without_agents;
use crate::support::{FixtureRepo, file_path};
use guided_review::code_source::CodeSource;
use guided_review::config::Config;
use guided_review::ide::{Ide, NoIde};
use guided_review::local_facts::LocalChange;
use guided_review::local_git::LocalGit;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::open_review;
use guided_review::opened_subject::OpenedChangeSet;
use guided_review::pull_request::{HostSite, HostedRepo, RepoName, Sha};
use guided_review::review_subject::ReviewSubject;
use guided_review::review_subject::{BranchName, WorkTreeRoot};
use guided_review::tour::{BasicTour, Tour, TourPin};
use guided_review::viewer::{
    CodeOpener, Destination, Effect, IdeCaveat, Intent, NotInIde, Opened, PieceDiff, Review,
    ReviewBuilder, ReviewStateFile, ReviewVerdict, Side, WindowMoment, WorkKind, WorkingCopy,
};

/// `main` with a README of two lines and a file that goes, then `feature` one commit ahead that adds
/// the cart and removes `old.txt`, with the README's second line replaced, the cart changed again and
/// a note added, none of them committed.
struct Feature {
    repo: FixtureRepo,
}

impl Feature {
    fn with_work_in_progress() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\nbye\n");
        repo.write("old.txt", "gone\n");
        repo.commit("Start the shop");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("src/cart.rs", "pub fn total() {}\n");
        std::fs::remove_file(repo.path().join("old.txt")).unwrap();
        repo.commit("Add the cart");
        repo.write("README.md", "hello\nworld\n");
        repo.write("src/cart.rs", "pub fn total() {}\npub fn tax() {}\n");
        repo.write("notes.txt", "draft\n");
        Self { repo }
    }

    fn subject(&self) -> ReviewSubject {
        let work_tree = LocalWorkTree::containing(self.repo.path()).unwrap();
        ReviewSubject::from(work_tree.change_set().unwrap())
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(&self.repo.folders(), &self.subject())
    }

    fn source(&self) -> LocalGit {
        let work_tree = LocalWorkTree::containing(self.repo.path()).unwrap();
        LocalGit::at_worktree(work_tree.change_set().unwrap().folder())
    }

    /// Opened as the app opens a folder: the basic tour is made once, and the clone is asked once
    /// whether its files are the snapshot's.
    fn open(&self) -> Review {
        let work_tree = LocalWorkTree::containing(self.repo.path()).unwrap();
        let change_set = work_tree.change_set().unwrap();
        OpenedChangeSet::new(&change_set, &self.repo.folders())
            .make_basic_tour_if_no_tour()
            .unwrap();
        let tour =
            BasicTour::load(&Tour::basic_path_for(&self.repo.folders(), &self.subject())).unwrap();
        let source = self.source();
        let head = tour.pin.head.clone();
        let facts = work_tree
            .facts(&change_set, &tour.pin.pinned_tips())
            .unwrap();
        let mut review = Review::open_basic(
            tour,
            &source,
            self.state_file(),
            guided_review::viewer::HostCapabilities::of(&guided_review::viewer::NoHost::default()),
        )
        .unwrap();
        review.local_facts_are(facts);
        review.working_copy_is(WorkingCopy::of(source.working_tree_is_at(&head)));
        review
    }

    fn ide(&self) -> Result<Ide, NoIde> {
        let config = Config::from_text("ide = \"idea://open?file={path}&line={line}\"").unwrap();
        Ide::for_clone(Some(self.repo.path().to_path_buf()), config.ide_template())
    }

    fn in_ide(&self, path: &str, line: usize) -> Destination {
        Destination::Ide {
            launch: self
                .ide()
                .unwrap()
                .launch(&file_path(path), crate::support::line(line)),
            caveat: None,
        }
    }
}

const PR: &str = "https://github.com/acme/shop/pull/7";

/// A basic review of a PR, to compare with: a clone with one commit past `main`.
struct PullRequest {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl PullRequest {
    fn changing_a_file() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("seed.txt", "seed\n");
        let base = repo.commit("base");
        repo.write("a.txt", "changed\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn open(&self) -> Review {
        let tour = BasicTour::new(
            TourPin {
                version: 1,
                pr: PR.to_owned(),
                local: None,
                repo: None,
                host: None,
                base: Sha::new(&self.base),
                head: Sha::new(&self.head),
                previous_head: None,
            },
            "Orders cost two".to_owned(),
        );
        let source = LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_subject(
            &self.repo.folders(),
            &PR.parse::<ReviewSubject>().unwrap(),
        );
        Review::open_basic(
            tour,
            &source,
            state_file,
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }
}

fn changes_on_the_overview(review: &Review) -> Vec<(String, Option<LocalChange>)> {
    review
        .overview_page()
        .files
        .into_iter()
        .map(|file| (file.path.to_string(), file.change))
        .collect()
}

fn line_destination(
    review: &mut Review,
    opener: &CodeOpener,
    path: &str,
    side: Side,
) -> Destination {
    review.go_to_file(&file_path(path));
    let page = review.file_page().unwrap();
    let line = page
        .pieces
        .iter()
        .flat_map(PieceDiff::lines)
        .find(|line| line.location.side == side)
        .unwrap();
    opener.destination(&line.code_link)
}

#[test]
fn the_overview_names_the_repo_folder_branch_default_branch_commits_and_uncommitted_files() {
    let feature = Feature::with_work_in_progress();
    let review = feature.open();

    let page = review.overview_page();

    assert_eq!(page.description, None);
    assert_eq!(page.title, "feature");
    let local = page.local.unwrap();
    assert_eq!(
        local.origin,
        Some(HostedRepo {
            host: HostSite::GitHub,
            repo: RepoName::new("acme", "shop")
        })
    );
    assert_eq!(
        local.folder,
        WorkTreeRoot::new(feature.repo.path().canonicalize().unwrap()).unwrap()
    );
    assert_eq!(local.branch, BranchName::new("feature").unwrap());
    assert_eq!(local.default_branch, BranchName::new("main").unwrap());
    assert_eq!(local.commit_subjects, ["Add the cart"]);
    assert_eq!(local.uncommitted_files.len(), 3);
}

#[test]
fn a_pr_review_has_no_local_overview() {
    let page = PullRequest::changing_a_file().open().overview_page();

    assert_eq!(page.local, None);
    assert!(page.files.iter().all(|file| file.change.is_none()));
}

#[test]
fn each_file_of_the_overview_is_marked_committed_uncommitted_or_both() {
    let feature = Feature::with_work_in_progress();
    let review = feature.open();

    assert_eq!(
        changes_on_the_overview(&review),
        [
            ("src/cart.rs".to_owned(), Some(LocalChange::Both)),
            ("notes.txt".to_owned(), Some(LocalChange::Uncommitted)),
            ("old.txt".to_owned(), Some(LocalChange::Committed)),
            ("README.md".to_owned(), Some(LocalChange::Uncommitted)),
        ]
    );
}

#[test]
fn the_file_page_carries_the_same_mark_as_the_overview() {
    let feature = Feature::with_work_in_progress();
    let mut review = feature.open();

    let marks: Vec<_> = ["README.md", "old.txt", "src/cart.rs"]
        .into_iter()
        .map(|path| {
            review.go_to_file(&file_path(path));
            review.file_page().unwrap().change
        })
        .collect();

    assert_eq!(
        marks,
        [
            Some(LocalChange::Uncommitted),
            Some(LocalChange::Committed),
            Some(LocalChange::Both)
        ]
    );
}

#[test]
fn a_line_of_an_added_file_opens_in_the_ide_and_never_on_github() {
    let feature = Feature::with_work_in_progress();
    let mut review = feature.open();
    let opener = review.code_opener(feature.ide());

    let destination = line_destination(&mut review, &opener, "src/cart.rs", Side::Head);

    assert_eq!(destination, feature.in_ide("src/cart.rs", 1));
}

#[test]
fn a_working_tree_that_is_the_snapshot_opens_in_the_ide_with_no_caveat() {
    let feature = Feature::with_work_in_progress();
    let mut review = feature.open();
    let opener = review.code_opener(feature.ide());

    let destination = line_destination(&mut review, &opener, "README.md", Side::Head);

    assert_eq!(destination, feature.in_ide("README.md", 2));
}

#[test]
fn a_file_changed_on_disk_since_the_snapshot_opens_in_the_ide_with_the_caveat() {
    let feature = Feature::with_work_in_progress();
    let mut review = feature.open();
    feature.repo.write("README.md", "changed again\n");
    let head = review.head().clone();
    review.working_copy_is(WorkingCopy::of(feature.source().working_tree_is_at(&head)));
    let opener = review.code_opener(feature.ide());

    assert!(matches!(
        line_destination(&mut review, &opener, "README.md", Side::Head),
        Destination::Ide {
            caveat: Some(IdeCaveat::CloneNotAtTourHead),
            ..
        }
    ));
}

#[test]
fn a_removed_line_cannot_be_opened_and_says_it_exists_only_at_the_branchs_base() {
    let feature = Feature::with_work_in_progress();
    let mut review = feature.open();
    let opener = review.code_opener(feature.ide());

    let destination = line_destination(&mut review, &opener, "README.md", Side::Base);

    assert_eq!(
        destination,
        Destination::NotOpenable {
            because: NotInIde::RemovedFromBranch
        }
    );
    assert_eq!(destination.label(), "Cannot open");
    assert_eq!(
        destination.explanation(),
        "Cannot open: removed lines exist only at the branch's base"
    );
    let mut opened_urls: Vec<String> = Vec::new();
    assert_eq!(
        destination.open(|url| opened_urls.push(url.to_owned())),
        Opened::Nowhere
    );
    assert_eq!(opened_urls, Vec::<String>::new());
}

#[test]
fn a_github_link_in_the_text_opens_in_the_browser_as_written() {
    let feature = Feature::with_work_in_progress();
    let review = feature.open();
    let opener = review.code_opener(feature.ide());
    let url = "https://github.com/acme/shop/blob/main/src/cart.rs#L3";

    assert_eq!(
        opener.destination_of_url(url),
        Destination::Browser(url.to_owned())
    );
}

#[test]
fn a_repo_relative_link_opens_in_the_ide_when_the_folder_holds_the_file_and_otherwise_cannot_open()
{
    let feature = Feature::with_work_in_progress();
    let review = feature.open();
    let opener = review.code_opener(feature.ide());

    assert_eq!(
        opener.destination_of_url("src/cart.rs#L2"),
        feature.in_ide("src/cart.rs", 2)
    );
    assert_eq!(
        opener.destination_of_url("src/missing.rs"),
        Destination::NotOpenable {
            because: NotInIde::NotInWorkingCopy
        }
    );
}

#[test]
fn without_an_ide_template_code_cannot_open_and_says_so() {
    let feature = Feature::with_work_in_progress();
    let mut review = feature.open();
    let opener = review.code_opener(Err(NoIde::NoTemplate));

    assert_eq!(
        line_destination(&mut review, &opener, "src/cart.rs", Side::Head),
        Destination::NotOpenable {
            because: NotInIde::NoIdeTemplate
        }
    );
}

#[test]
fn the_window_title_names_the_repo_and_branch() {
    let feature = Feature::with_work_in_progress();

    assert_eq!(
        feature.open().window_title(),
        "Local changes · acme/shop · feature"
    );
}

#[test]
fn a_clone_with_no_remote_leaves_the_repo_out_of_the_title() {
    let feature = Feature::with_work_in_progress();
    feature.repo.git(&["remote", "remove", "origin"]);

    assert_eq!(feature.open().window_title(), "Local changes · feature");
}

#[test]
fn a_pr_keeps_its_url_and_title_in_the_window_title() {
    let pull = PullRequest::changing_a_file();

    assert_eq!(
        pull.open().window_title(),
        "https://github.com/acme/shop/pull/7: Orders cost two"
    );
}

#[test]
fn only_a_pr_can_be_opened_on_github() {
    let feature = Feature::with_work_in_progress();
    let local = feature.open();
    let pr = PullRequest::changing_a_file().open();

    assert!(!local.can_open_on_host());
    assert_eq!(
        local.decide(Intent::OpenOnHost, WindowMoment::default()),
        Effect::Nothing
    );
    assert!(pr.can_open_on_host());
}

#[test]
fn a_local_review_reads_and_sends_nothing_on_github_whatever_the_reviewer_does() {
    let feature = Feature::with_work_in_progress();
    let sent = SentLog::default();
    let host = FakeHost::with_pull("main-tip", "snapshot", &sent);
    let source = feature.source();
    let mut review = feature.open();

    review.pull_from_host();
    let ran = drive_without_agents(&mut review, &host, &source, Instant::now());
    review.tick_file(&file_path("README.md")).unwrap();
    review.untick_file(&file_path("README.md")).unwrap();
    review.tick_file(&file_path("notes.txt")).unwrap();
    review.pull_from_host();
    let ran_after = drive_without_agents(&mut review, &host, &source, Instant::now());
    let finishing = review.finish_review_request(ReviewVerdict::Comment, "");

    assert_eq!(
        ran,
        [
            WorkKind::CheckWorkingTree,
            WorkKind::LookUpPullOfBranch,
            WorkKind::PullThreads
        ]
    );
    assert_eq!(
        ran_after,
        [
            WorkKind::CheckWorkingTree,
            WorkKind::LookUpPullOfBranch,
            WorkKind::PullThreads
        ]
    );
    assert!(!review.not_synced());
    assert_eq!(review.sync_banner(), None);
    assert!(finishing.is_err());
    assert_eq!(sent.host_calls(), []);
    assert_eq!(review.problems(), []);
}

#[test]
fn a_local_review_has_no_pr_description() {
    let feature = Feature::with_work_in_progress();
    let review = feature.open();

    assert_eq!(review.pr_description(), None);
    assert_eq!(review.overview_page().description, None);
}

#[test]
fn file_ticks_are_saved_and_come_back_when_the_review_is_reopened() {
    let feature = Feature::with_work_in_progress();
    let mut review = feature.open();

    review.tick_file(&file_path("src/cart.rs")).unwrap();
    let reopened = feature.open();

    let read: Vec<_> = reopened
        .overview_page()
        .files
        .into_iter()
        .map(|file| (file.path.to_string(), file.is_read))
        .collect();
    assert_eq!(
        read,
        [
            ("src/cart.rs".to_owned(), true),
            ("notes.txt".to_owned(), false),
            ("old.txt".to_owned(), false),
            ("README.md".to_owned(), false),
        ]
    );
}

#[test]
fn a_change_is_named_as_committed_uncommitted_or_both() {
    assert_eq!(
        [
            LocalChange::Committed,
            LocalChange::Uncommitted,
            LocalChange::Both
        ]
        .map(LocalChange::label),
        ["committed", "uncommitted", "committed and uncommitted"]
    );
}

#[test]
fn the_overview_counts_its_uncommitted_files_in_words() {
    let feature = Feature::with_work_in_progress();
    let local = feature.open().overview_page().local.unwrap();

    assert_eq!(local.uncommitted_label(), "3 files uncommitted");
    let mut one = local.clone();
    one.uncommitted_files
        .retain(|path| path.as_str() == "notes.txt");
    assert_eq!(one.uncommitted_label(), "1 file uncommitted");
    let mut none = local;
    none.uncommitted_files.clear();
    assert_eq!(none.uncommitted_label(), "No uncommitted files");
}

#[test]
fn the_overview_rows_of_a_local_review_hold_its_facts_and_ask_about_the_whole_change() {
    let feature = Feature::with_work_in_progress();
    let review = feature.open();
    let pr = PullRequest::changing_a_file().open();

    let page = review.page();
    let local = page.overview().unwrap().local.as_ref().unwrap();

    assert_eq!(local.branch.as_str(), "feature");
    assert_eq!(local.commit_subjects, ["Add the cart"]);
    assert_eq!(page.screen_ask().unwrap().label, "the whole change");
    assert_eq!(pr.page().overview().unwrap().local, None);
    assert_eq!(pr.page().screen_ask().unwrap().label, "the whole PR");
}

#[test]
fn a_file_screens_toolbar_names_where_the_files_change_comes_from() {
    let feature = Feature::with_work_in_progress();
    let mut review = feature.open();
    let mut pr = PullRequest::changing_a_file().open();

    let marks: Vec<_> = ["README.md", "old.txt", "src/cart.rs"]
        .into_iter()
        .map(|path| {
            review.go_to_file(&file_path(path));
            review.page().screen_change()
        })
        .collect();
    pr.go_to_file(&file_path("a.txt"));

    assert_eq!(
        marks,
        [
            Some(LocalChange::Uncommitted),
            Some(LocalChange::Committed),
            Some(LocalChange::Both)
        ]
    );
    assert_eq!(pr.page().screen_change(), None);
}

#[test]
fn a_local_review_leads_its_sidebar_with_the_window_title_and_a_pr_does_not() {
    let feature = Feature::with_work_in_progress();

    assert_eq!(
        feature.open().local_heading().as_deref(),
        Some("Local changes · acme/shop · feature")
    );
    assert_eq!(PullRequest::changing_a_file().open().local_heading(), None);
}

#[test]
fn a_rebuilt_local_review_reads_the_clone_again() {
    let feature = Feature::with_work_in_progress();
    let shown = feature.open();
    let tour = BasicTour::load(&Tour::basic_path_for(
        &feature.repo.folders(),
        &feature.subject(),
    ))
    .unwrap();
    let mut reloaded = ReviewBuilder::new(
        &feature.repo.folders(),
        &feature.subject(),
        guided_review::viewer::HostCapabilities::of(&guided_review::viewer::NoHost::default()),
    )
    .build(Tour::Basic(tour), &feature.source())
    .unwrap();

    reloaded.carry_on_from(shown);

    assert_eq!(
        reloaded.window_title(),
        "Local changes · acme/shop · feature"
    );
    assert_eq!(
        reloaded.overview_page().local.unwrap().commit_subjects,
        ["Add the cart"]
    );
}

fn opened_by_the_app(feature: &Feature) -> guided_review::open_review::OpenedReview {
    open_review(
        &feature.subject(),
        feature.repo.path(),
        &feature.repo.folders(),
    )
    .unwrap()
}

#[test]
fn the_app_opens_a_local_review_with_its_facts_title_and_the_clone_to_open_code_in() {
    let feature = Feature::with_work_in_progress();

    let opened = opened_by_the_app(&feature);

    assert_eq!(
        opened.review.window_title(),
        "Local changes · acme/shop · feature"
    );
    assert_eq!(
        changes_on_the_overview(&opened.review)
            .into_iter()
            .map(|(_, change)| change)
            .collect::<Vec<_>>(),
        [
            Some(LocalChange::Both),
            Some(LocalChange::Uncommitted),
            Some(LocalChange::Committed),
            Some(LocalChange::Uncommitted)
        ]
    );
    let mut review = opened.review;
    let opener = review.code_opener(feature.ide());
    assert_eq!(
        line_destination(&mut review, &opener, "README.md", Side::Head),
        feature.in_ide("README.md", 2)
    );
    assert_eq!(
        opened.source.local_clone(),
        Some(feature.subject().local().unwrap().folder().as_path())
    );
}

#[test]
fn opening_a_local_review_again_reads_the_same_snapshot_even_when_the_folder_moved_on() {
    let feature = Feature::with_work_in_progress();
    let first = opened_by_the_app(&feature);
    feature.repo.write("later.txt", "after the snapshot\n");

    let again = opened_by_the_app(&feature);

    assert_eq!(
        changes_on_the_overview(&again.review),
        changes_on_the_overview(&first.review)
    );
    let mut review = again.review;
    let opener = review.code_opener(feature.ide());
    assert!(matches!(
        line_destination(&mut review, &opener, "README.md", Side::Head),
        Destination::Ide {
            caveat: Some(IdeCaveat::CloneNotAtTourHead),
            ..
        }
    ));
}

#[test]
fn a_local_reviews_github_port_refuses_every_call() {
    let feature = Feature::with_work_in_progress();
    let opened = opened_by_the_app(&feature);

    let tips = opened
        .host
        .pull_request_tips(&"https://github.com/acme/shop/pull/7".parse().unwrap());

    assert!(tips.is_err());
}

#[test]
fn an_ask_handed_to_the_agent_of_a_local_review_names_the_change_set_not_a_pr() {
    let feature = Feature::with_work_in_progress();
    let review = feature.open();
    let mut asks = guided_review::viewer::Asks::load(std::sync::Arc::new(
        crate::support::fake_ports::FakeAskStore::default(),
    ))
    .unwrap();
    let id = asks.start(guided_review::viewer::Place::Tour, review.head());
    let ask = asks.get(id).unwrap().clone();

    let text = review.ask_hand_off_text(&ask, "the whole change");

    assert!(text.contains(&feature.subject().to_string()), "{text}");
    assert!(text.contains("guided-review outline"), "{text}");
    assert!(!text.contains("PR"), "{text}");
}
