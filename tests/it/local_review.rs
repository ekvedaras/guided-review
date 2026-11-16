use std::time::Instant;

use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::work::drive_without_agents;
use crate::support::{FixtureRepo, file_path};
use guided_review::code_source::CodeSource;
use guided_review::config::Config;
use guided_review::ide::{Ide, NoIde};
use guided_review::local_facts::LocalChange;
use guided_review::local_git::LocalGit;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::opened_change_set::OpenedChangeSet;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::review_subject::ReviewSubject;
use guided_review::review_subject::{BranchName, WorkTreeRoot};
use guided_review::tour::{BasicTour, Tour, TourPin};
use guided_review::viewer::{
    CodeOpener, Destination, Effect, IdeCaveat, Intent, LocalOverview, NotInIde, Opened, PieceDiff,
    Review, ReviewStateFile, ReviewVerdict, Side, WindowMoment, WorkKind, WorkingCopy,
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
        let mut review = Review::open_basic(tour, &source, self.state_file()).unwrap();
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
                base: Sha::new(&self.base),
                head: Sha::new(&self.head),
                previous_head: None,
            },
            "Orders cost two".to_owned(),
        );
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_subject(
            &self.repo.folders(),
            &PR.parse::<ReviewSubject>().unwrap(),
        );
        Review::open_basic(tour, &source, state_file).unwrap()
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
    assert_eq!(
        page.local,
        Some(LocalOverview {
            repo: Some(RepoName::new("acme", "shop")),
            folder: WorkTreeRoot::new(feature.repo.path().canonicalize().unwrap()).unwrap(),
            branch: BranchName::new("feature").unwrap(),
            default_branch: "main".to_owned(),
            commit_subjects: vec!["Add the cart".to_owned()],
            uncommitted_files: 3,
        })
    );
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
    let mut opened_urls = Vec::new();
    assert_eq!(
        destination.open(|url| opened_urls.push(url.to_owned())),
        Opened::Nowhere
    );
    assert!(opened_urls.is_empty());
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

    assert!(!local.can_open_on_github());
    assert_eq!(
        local.decide(Intent::OpenOnGitHub, WindowMoment::default()),
        Effect::Nothing
    );
    assert!(pr.can_open_on_github());
}

#[test]
fn a_local_review_reads_and_sends_nothing_on_github_whatever_the_reviewer_does() {
    let feature = Feature::with_work_in_progress();
    let sent = SentLog::default();
    let github = FakeGitHub::with_pull("main-tip", "snapshot", &sent);
    let source = feature.source();
    let mut review = feature.open();

    review.pull_from_github();
    let ran = drive_without_agents(&mut review, &github, &source, Instant::now());
    review.tick_file(&file_path("README.md")).unwrap();
    review.untick_file(&file_path("README.md")).unwrap();
    review.tick_file(&file_path("notes.txt")).unwrap();
    review.pull_from_github();
    let ran_after = drive_without_agents(&mut review, &github, &source, Instant::now());
    let finishing = review.finish_review_request(ReviewVerdict::Comment, "");

    assert_eq!(ran, [WorkKind::CheckWorkingCopy]);
    assert_eq!(ran_after, [WorkKind::CheckWorkingCopy]);
    assert_eq!(review.next_due(), None);
    assert!(!review.not_synced());
    assert_eq!(review.sync_banner(), None);
    assert!(finishing.is_err());
    assert_eq!(sent.github_calls(), []);
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
