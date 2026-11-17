use std::time::{Duration, Instant};

use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::work::drive_without_agents;
use crate::support::{FixtureRepo, GuidedReview, file_path};
use guided_review::app_folders::AppFolders;
use guided_review::config::Config;
use guided_review::ide::Ide;
use guided_review::local_work_tree::{LocalWorkTree, snapshot_ref};
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::opened_change_set::{OpenedChangeSet, clean_snapshot_refs};
use guided_review::pull_request::Sha;
use guided_review::review_subject::{ReviewSubject, WorkTreeRoot};
use guided_review::tour::{Tour, ToursOnDisk};
use guided_review::viewer::{
    Destination, DiskNews, DiskOffer, IdeCaveat, PieceDiff, ProblemKind, Review, ReviewStateFile,
    Side, WorkKind,
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

fn changed_on_disk() -> DiskNews {
    DiskNews {
        line: "Changed on disk".to_owned(),
        offer: Some(DiskOffer::Update),
    }
}

#[test]
fn a_folder_left_as_it_was_snapshotted_says_nothing_and_only_it_the_branchs_pr_and_the_local_threads_are_read_on_focus()
 {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();

    let ran = check_on_focus(&mut opened, Instant::now());

    assert_eq!(
        ran,
        [
            WorkKind::CheckWorkingTree,
            WorkKind::LookUpPullOfBranch,
            WorkKind::PullThreads
        ]
    );
    assert_eq!(news(&opened.review), None);
}

#[test]
fn writing_a_file_says_changed_on_disk_with_update() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.write("notes.txt", "a new file\n");

    check_on_focus(&mut opened, Instant::now());

    assert_eq!(news(&opened.review), Some(changed_on_disk()));
}

#[test]
fn committing_says_changed_on_disk_with_update() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.commit("Say world");

    check_on_focus(&mut opened, Instant::now());

    assert_eq!(news(&opened.review), Some(changed_on_disk()));
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

#[test]
fn looking_at_the_folder_writes_nothing_in_its_git_folder() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    feature.repo.write("notes.txt", "a new file\n");
    let git_folder = feature.repo.path().join(".git");
    let modified = || std::fs::metadata(&git_folder).unwrap().modified().unwrap();
    let before = modified();

    check_on_focus(&mut opened, Instant::now());

    assert_eq!(news(&opened.review), Some(changed_on_disk()));
    assert_eq!(modified(), before, "a file came and went in .git");
}

#[test]
fn a_folder_that_cannot_be_looked_at_is_one_problem_however_often_it_is_looked_at() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let change_set = feature.subject().local().unwrap().clone();
    feature
        .repo
        .git(&["update-ref", "-d", &snapshot_ref(&change_set)]);
    feature
        .repo
        .git(&["reflog", "expire", "--expire=now", "--all"]);
    feature.repo.git(&["gc", "-q", "--prune=now"]);
    let now = Instant::now();

    check_on_focus(&mut opened, now);
    check_on_focus(&mut opened, now + Duration::from_secs(31));

    let shown: Vec<String> = opened
        .review
        .problems()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(
        shown[0].starts_with(
            "The folder could not be looked at, so the sidebar may not say how it changed: "
        ),
        "{shown:?}"
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
    assert_eq!(news(&opened.review), Some(changed_on_disk()));
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

fn tour_head(feature: &Feature) -> Sha {
    let path = Tour::shown_path_for(&feature.repo.folders(), &feature.subject());
    Tour::load_of(&path, &feature.subject())
        .unwrap()
        .pin()
        .head
        .clone()
}

fn update_in_the_background(feature: &Feature) -> std::thread::JoinHandle<()> {
    let change_set = feature.subject().local().unwrap().clone();
    let folders = feature.repo.folders();
    std::thread::spawn(move || {
        OpenedChangeSet::new(&change_set, &folders)
            .repin_to_new_snapshot()
            .unwrap();
    })
}

/// The CLI's `repin` and the app's Update of one review, at once: the second saves its tour while the
/// first has not moved the ref yet, as both wait on the state file someone else holds.
#[test]
fn two_updates_at_once_leave_the_ref_on_the_snapshot_the_tour_reads() {
    let feature = Feature::with_work_in_progress();
    feature.open();
    let state_file = ReviewStateFile::for_subject(&feature.repo.folders(), &feature.subject());
    for round in 0..6 {
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (held, is_held) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn({
            let state_file = state_file.clone();
            move || {
                state_file
                    .update(|_| {
                        held.send(()).unwrap();
                        released.recv().ok();
                    })
                    .unwrap();
            }
        });
        is_held.recv().unwrap();
        let before = tour_head(&feature);
        feature
            .repo
            .write("notes.txt", &format!("round {round}, first\n"));
        let first = update_in_the_background(&feature);
        let waited = Instant::now();
        while tour_head(&feature) == before {
            assert!(waited.elapsed() < Duration::from_secs(20), "no tour saved");
            std::thread::sleep(Duration::from_millis(10));
        }
        feature
            .repo
            .write("notes.txt", &format!("round {round}, second\n"));
        let second = update_in_the_background(&feature);
        std::thread::sleep(Duration::from_millis(500));

        drop(release);
        for update in [holder, first, second] {
            update.join().unwrap();
        }

        assert_eq!(
            snapshot_ref_target(&feature),
            tour_head(&feature),
            "round {round}"
        );
    }
}

/// Held as Update, `repin` or Drop review of the review hold it, from another process.
fn hold_pin_lock(feature: &Feature) -> std::fs::File {
    let path =
        Tour::path_for(&feature.repo.folders(), &feature.subject()).with_extension("json.pin.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = std::fs::File::create(&path).unwrap();
    lock.lock().unwrap();
    lock
}

/// Two opens of one folder at once, as `guided-review open .` from two terminals: each takes a
/// snapshot, and the one that saves its tour last must also be the one whose ref stays.
#[test]
fn an_open_waits_for_whoever_moves_the_tour_or_ref_so_the_ref_ends_on_the_snapshot_the_tour_reads()
{
    let feature = Feature::with_work_in_progress();
    let held = hold_pin_lock(&feature);
    let change_set = feature.subject().local().unwrap().clone();
    let folders = feature.repo.folders();
    let open = std::thread::spawn(move || {
        OpenedChangeSet::new(&change_set, &folders)
            .make_basic_tour_if_no_tour()
            .unwrap()
    });
    std::thread::sleep(Duration::from_millis(500));

    assert_eq!(refs_kept(&feature), Vec::<String>::new());
    assert_eq!(
        ToursOnDisk::of(&feature.repo.folders(), &feature.subject()),
        ToursOnDisk::None
    );

    drop(held);
    assert!(open.join().unwrap());
    assert_eq!(snapshot_ref_target(&feature), tour_head(&feature));
}

#[test]
fn update_refuses_a_tour_file_of_another_review_at_its_path_and_moves_nothing() {
    let feature = Feature::with_work_in_progress();
    let opened = feature.open();
    let head = opened.review.head().clone();
    let path = Tour::shown_path_for(&feature.repo.folders(), &feature.subject());
    let key = feature.subject().to_string();
    let other = "local:/somewhere/else@feature";
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace(&key, other)).unwrap();
    feature.repo.write("notes.txt", "a new file\n");

    let refused = OpenedChangeSet::new(feature.subject().local().unwrap(), &feature.repo.folders())
        .repin_to_new_snapshot()
        .unwrap_err();

    assert!(format!("{refused:#}").contains(other), "{refused:#}");
    assert_eq!(snapshot_ref_target(&feature), head);
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

fn refs_kept(feature: &Feature) -> Vec<String> {
    feature
        .repo
        .git(&["for-each-ref", "--format=%(refname)", "refs/guided-review/"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A second local review in the same clone, of the branch `second`, and the fetch ref of a PR.
fn with_a_second_review_and_a_pr_ref(feature: &Feature) -> ReviewSubject {
    feature.repo.git(&["checkout", "-q", "-b", "second"]);
    let second = ReviewSubject::from(
        LocalWorkTree::containing(feature.repo.path())
            .unwrap()
            .change_set()
            .unwrap(),
    );
    open_review(&second, feature.repo.path(), &feature.repo.folders()).unwrap();
    feature.repo.git(&["checkout", "-q", "feature"]);
    feature
        .repo
        .git(&["update-ref", "refs/guided-review/7", "HEAD"]);
    second
}

fn local_ref(subject: &ReviewSubject) -> String {
    snapshot_ref(subject.local().unwrap())
}

#[test]
fn drop_review_deletes_its_tours_state_file_and_ref_and_nothing_of_other_reviews() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    opened.review.tick_file(&file_path("README.md")).unwrap();
    let second = with_a_second_review_and_a_pr_ref(&feature);
    let folders = feature.repo.folders();

    OpenedChangeSet::new(feature.subject().local().unwrap(), &folders)
        .drop_review()
        .unwrap();

    assert_eq!(
        ToursOnDisk::of(&folders, &feature.subject()),
        ToursOnDisk::None
    );
    assert!(
        !ReviewStateFile::for_subject(&folders, &feature.subject()).exists(),
        "the state file is still there"
    );
    assert_eq!(
        refs_kept(&feature),
        ["refs/guided-review/7".to_owned(), local_ref(&second)]
    );
    assert_ne!(ToursOnDisk::of(&folders, &second), ToursOnDisk::None);
}

#[test]
fn a_snapshot_ref_git_cannot_reach_is_an_error_and_not_a_ref_already_gone() {
    let not_a_clone = tempfile::TempDir::new().unwrap();
    let work_tree = LocalWorkTree::at(WorkTreeRoot::new(not_a_clone.path().to_path_buf()).unwrap());

    let deleted = work_tree.delete_snapshot_ref("refs/guided-review/local-0123456789ab");

    assert!(deleted.is_err(), "{deleted:?}");
}

#[test]
fn dropping_the_review_of_a_deleted_folder_deletes_its_files() {
    let feature = Feature::with_work_in_progress();
    feature.open();
    let folders = feature.repo.folders();
    std::fs::remove_dir_all(feature.repo.path()).unwrap();

    OpenedChangeSet::new(feature.subject().local().unwrap(), &folders)
        .drop_review()
        .unwrap();

    assert_eq!(
        ToursOnDisk::of(&folders, &feature.subject()),
        ToursOnDisk::None
    );
}

/// Makes deleting the review's state file fail, as a crash part way through a drop would leave it: a
/// folder in its place cannot be deleted as a file.
fn state_file_undeletable(folders: &AppFolders, subject: &ReviewSubject) {
    let path = folders
        .state_files()
        .join(subject.files_folder())
        .join(format!("{}.json", subject.file_stem()));
    std::fs::remove_file(&path).ok();
    std::fs::create_dir_all(path.join("in-the-way")).unwrap();
}

#[test]
fn a_drop_that_stops_part_way_leaves_no_snapshot_ref_once_the_app_starts_again() {
    let feature = Feature::with_work_in_progress();
    feature.open();
    let folders = feature.repo.folders();
    state_file_undeletable(&folders, &feature.subject());

    let dropped = OpenedChangeSet::new(feature.subject().local().unwrap(), &folders).drop_review();
    clean_snapshot_refs(&folders);

    assert!(dropped.is_err());
    assert_eq!(refs_kept(&feature), Vec::<String>::new());
}

#[test]
fn a_review_being_dropped_takes_no_change_so_nothing_writes_its_state_file_again() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let folders = feature.repo.folders();

    assert!(opened.review.start_drop());
    OpenedChangeSet::new(feature.subject().local().unwrap(), &folders)
        .drop_review()
        .unwrap();
    let ticked = opened.review.tick_file(&file_path("README.md"));

    assert!(ticked.is_err());
    assert!(!ReviewStateFile::for_subject(&folders, &feature.subject()).exists());
}

#[test]
fn a_review_dropped_from_elsewhere_never_writes_its_state_file_again() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let folders = feature.repo.folders();
    OpenedChangeSet::new(feature.subject().local().unwrap(), &folders)
        .drop_review()
        .unwrap();

    let ticked = opened.review.tick_file(&file_path("README.md"));

    assert!(ticked.is_err());
    assert!(!ReviewStateFile::for_subject(&folders, &feature.subject()).exists());
    assert_eq!(
        ToursOnDisk::of(&folders, &feature.subject()),
        ToursOnDisk::None
    );
}

#[test]
fn on_start_the_refs_of_local_reviews_no_longer_on_disk_are_deleted() {
    let feature = Feature::with_work_in_progress();
    feature.open();
    let second = with_a_second_review_and_a_pr_ref(&feature);
    let folders = feature.repo.folders();
    for tour in [
        Tour::basic_path_for(&folders, &feature.subject()),
        Tour::path_for(&folders, &feature.subject()),
    ] {
        std::fs::remove_file(tour).ok();
    }

    clean_snapshot_refs(&folders);

    assert_eq!(
        refs_kept(&feature),
        ["refs/guided-review/7".to_owned(), local_ref(&second)]
    );
}

/// A review of the branch `other` in a linked worktree of the feature's clone, which shares its refs.
/// The worktree lives as long as the returned folder.
fn with_a_review_in_a_linked_worktree(feature: &Feature) -> (tempfile::TempDir, ReviewSubject) {
    let parent = tempfile::TempDir::new().unwrap();
    let linked = parent.path().canonicalize().unwrap().join("linked");
    feature.repo.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "other",
        linked.to_str().unwrap(),
        "main",
    ]);
    std::fs::write(linked.join("README.md"), "hello\nthere\n").unwrap();
    let other = ReviewSubject::from(
        LocalWorkTree::containing(&linked)
            .unwrap()
            .change_set()
            .unwrap(),
    );
    open_review(&other, &linked, &feature.repo.folders()).unwrap();
    (parent, other)
}

#[test]
fn on_start_the_refs_of_a_review_in_a_linked_worktree_of_the_same_clone_are_kept() {
    let feature = Feature::with_work_in_progress();
    feature.open();
    let (_linked, other) = with_a_review_in_a_linked_worktree(&feature);

    clean_snapshot_refs(&feature.repo.folders());

    let mut kept = vec![local_ref(&other), local_ref(&feature.subject())];
    kept.sort();
    assert_eq!(refs_kept(&feature), kept);
}

#[test]
fn on_start_a_linked_worktree_whose_only_tour_cannot_be_read_keeps_its_ref() {
    let feature = Feature::with_work_in_progress();
    feature.open();
    let (_linked, other) = with_a_review_in_a_linked_worktree(&feature);
    let folders = feature.repo.folders();
    std::fs::write(Tour::shown_path_for(&folders, &other), "not a tour").unwrap();

    clean_snapshot_refs(&folders);

    let mut kept = vec![local_ref(&other), local_ref(&feature.subject())];
    kept.sort();
    assert_eq!(refs_kept(&feature), kept);
}

#[test]
fn on_start_a_clone_with_a_tour_that_cannot_be_read_keeps_every_ref_and_names_the_tour() {
    let feature = Feature::with_work_in_progress();
    feature.open();
    let second = with_a_second_review_and_a_pr_ref(&feature);
    let stale = "refs/guided-review/local-0123456789ab";
    feature.repo.git(&["update-ref", stale, "HEAD"]);
    let folders = feature.repo.folders();
    let unreadable = Tour::shown_path_for(&folders, &feature.subject());
    std::fs::write(&unreadable, "not a tour").unwrap();

    let not_cleaned = clean_snapshot_refs(&folders);

    let mut kept = vec![
        "refs/guided-review/7".to_owned(),
        stale.to_owned(),
        local_ref(&second),
        local_ref(&feature.subject()),
    ];
    kept.sort();
    assert_eq!(refs_kept(&feature), kept);
    assert_eq!(not_cleaned.len(), 1, "{not_cleaned:?}");
    assert!(
        not_cleaned[0].contains(&unreadable.display().to_string()),
        "{not_cleaned:?}"
    );
}

#[test]
fn a_local_tour_that_cannot_be_read_is_a_problem_naming_the_file_in_any_review_opened() {
    let feature = Feature::with_work_in_progress();
    let second = with_a_second_review_and_a_pr_ref(&feature);
    let unreadable = Tour::shown_path_for(&feature.repo.folders(), &second);
    std::fs::write(&unreadable, "not a tour").unwrap();

    let opened = feature.open();

    let named: Vec<String> = opened
        .review
        .problems()
        .iter()
        .filter(|problem| problem.kind() == ProblemKind::LocalToursNotRead)
        .map(ToString::to_string)
        .collect();
    let [problem] = named.try_into().unwrap();
    assert!(
        problem.starts_with(&format!(
            "A local review's tour could not be read, so the snapshot refs of its clone are kept until it can: {} (",
            unreadable.display()
        )),
        "{problem}"
    );
}

#[test]
fn an_update_keeps_the_more_lines_setting_the_repo_and_the_problem_of_an_unreadable_local_tour() {
    let feature = Feature::with_work_in_progress();
    let second = with_a_second_review_and_a_pr_ref(&feature);
    std::fs::write(
        Tour::shown_path_for(&feature.repo.folders(), &second),
        "not a tour",
    )
    .unwrap();
    let config = feature.repo.folders().config_file();
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(config, "more_lines = 5\n").unwrap();
    let as_the_cli_names_it: ReviewSubject = feature.subject().to_string().parse().unwrap();
    let mut opened = open_review(
        &as_the_cli_names_it,
        feature.repo.path(),
        &feature.repo.folders(),
    )
    .unwrap();
    let repo = opened.review.local_facts().unwrap().repo.clone();
    assert!(repo.is_some());
    feature.repo.write("README.md", "hello\nworld\nagain\n");

    update(&mut opened);

    assert_eq!(opened.review.local_facts().unwrap().repo, repo);
    assert_eq!(opened.review.display_options().more_lines().get(), 5);
    assert!(
        opened
            .review
            .problems()
            .iter()
            .any(|problem| problem.kind() == ProblemKind::LocalToursNotRead)
    );
}

#[test]
fn a_review_opened_with_every_local_tour_readable_has_no_such_problem() {
    let feature = Feature::with_work_in_progress();
    with_a_second_review_and_a_pr_ref(&feature);

    let opened = feature.open();

    assert!(
        !opened
            .review
            .problems()
            .iter()
            .any(|problem| problem.kind() == ProblemKind::LocalToursNotRead)
    );
}

#[test]
fn repin_with_a_local_key_updates_the_review_to_a_new_snapshot_from_the_cli() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let old_head = opened.review.head().clone();
    for path in ["README.md", "src/cart.rs"] {
        opened.review.tick_file(&file_path(path)).unwrap();
    }
    feature.repo.write("README.md", "hello\nworld\nagain\n");
    let github = crate::support::FakeGitHub::serving_pull("acme/shop", 7, "aaa", "bbb");

    let run =
        GuidedReview::new(&feature.repo, &github).run(&["repin", &feature.subject().to_string()]);

    assert!(run.success, "{}", run.stderr);
    let reopened = feature.open();
    assert_ne!(reopened.review.head(), &old_head);
    assert_eq!(&snapshot_ref_target(&feature), reopened.review.head());
    assert_eq!(ticked(&reopened.review), ["src/cart.rs"]);
}
