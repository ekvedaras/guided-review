use std::path::Path;

use crate::support::fake_ports::{FakeHost, SentLog};
use crate::support::fake_tour_agent::FakeTourAgent;
use crate::support::{FixtureRepo, folders_in};
use guided_review::app_folders::AppFolders;
use guided_review::config::Config;
use guided_review::local_change_sets::find_local_rows;
use guided_review::local_work_tree::{LocalWorkTree, OnDisk};
use guided_review::opened_subject::OpenedChangeSet;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{Tour, ToursOnDisk};
use guided_review::viewer::{
    ActivePr, NoHost, Pick, PrAuthor, PrSwitcher, RelevantPullRequest, RelevantPullRequests,
    ReviewStateFile, SwitcherRow, WhyListed,
};

const SHOP_7: &str = "https://github.com/acme/shop/pull/7";
const BLOG_3: &str = "https://github.com/acme/blog/pull/3";

/// `main`, then `feature` one commit ahead with one file written and not committed.
fn shop_on_feature() -> FixtureRepo {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("src/cart.rs", "pub fn total() {}\n");
    repo.commit("Add the cart");
    repo.write("notes.txt", "draft\n");
    repo
}

fn subject_of(repo: &FixtureRepo) -> ReviewSubject {
    let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
    ReviewSubject::from(work_tree.change_set().unwrap())
}

fn clone_named_in_config(folder: &Path) -> Config {
    Config::from_text(&format!(
        "[clones]\n\"acme/shop\" = '{}'\n",
        folder.display()
    ))
    .unwrap()
}

/// The switcher after the local rows were found from the sources the app has.
fn found(folders: &AppFolders, config: &Config, agents: &FakeTourAgent) -> PrSwitcher {
    let mut switcher = PrSwitcher::default();
    switcher.local_found(
        find_local_rows(folders, config, agents, &NoHost::default()),
        folders,
    );
    switcher
}

fn subjects(switcher: &PrSwitcher) -> Vec<String> {
    switcher
        .visible()
        .map(|row| row.subject.to_string())
        .collect()
}

fn the_only_row(switcher: &PrSwitcher) -> &SwitcherRow {
    let mut rows = switcher.visible();
    let row = rows.next().expect("a row");
    assert!(rows.next().is_none(), "one row");
    row
}

fn make_the_review(repo: &FixtureRepo) {
    let change_set = LocalWorkTree::containing(repo.path())
        .unwrap()
        .change_set()
        .unwrap();
    OpenedChangeSet::new(&change_set, &repo.folders())
        .make_basic_tour_if_no_tour()
        .unwrap();
}

fn nothing_listed_from(home: &Path) -> (AppFolders, Config, FakeTourAgent) {
    (folders_in(home), Config::default(), FakeTourAgent::new())
}

#[test]
fn a_clone_settings_names_is_listed_with_its_repo_branch_commits_and_uncommitted_files() {
    let repo = shop_on_feature();
    let config = clone_named_in_config(repo.path());

    let switcher = found(&repo.folders(), &config, &FakeTourAgent::new());

    let row = the_only_row(&switcher);
    assert_eq!(row.subject, subject_of(&repo));
    let local = row.local().expect("a local row");
    assert_eq!(
        local.change_set.repo().map(ToString::to_string),
        Some("acme/shop".to_owned())
    );
    assert_eq!(local.change_set.branch().as_str(), "feature");
    assert_eq!(local.commits_ahead, Some(1));
    assert_eq!(local.uncommitted_files, Some(1));
    assert_eq!(local.on_disk, None, "no review yet");
}

#[test]
fn a_clone_whose_branch_has_no_change_set_is_not_listed() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    let config = clone_named_in_config(repo.path());

    let switcher = found(&repo.folders(), &config, &FakeTourAgent::new());

    assert_eq!(subjects(&switcher), Vec::<String>::new());
}

#[test]
fn a_folder_on_a_detached_head_is_not_listed() {
    let repo = shop_on_feature();
    repo.git(&["checkout", "-q", "--detach"]);
    let config = clone_named_in_config(repo.path());

    let switcher = found(&repo.folders(), &config, &FakeTourAgent::new());

    assert_eq!(subjects(&switcher), Vec::<String>::new());
}

#[test]
fn a_folder_a_pull_request_was_opened_from_is_listed() {
    let repo = shop_on_feature();
    let pr: ReviewSubject = SHOP_7.parse().unwrap();
    ReviewStateFile::for_subject(&repo.folders(), &pr)
        .remember_folder(repo.path())
        .unwrap();

    let switcher = found(&repo.folders(), &Config::default(), &FakeTourAgent::new());

    assert_eq!(subjects(&switcher), [subject_of(&repo).to_string()]);
}

#[test]
fn a_folder_a_claude_terminal_works_in_is_listed_and_one_a_shell_works_in_is_not() {
    let repo = shop_on_feature();
    let other = shop_on_feature();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    let agents = FakeTourAgent::new()
        .claude_in("t1", &repo.path().join("src").display().to_string())
        .shell_in("t2", &other.path().display().to_string());

    let switcher = found(&repo.folders(), &Config::default(), &agents);

    assert_eq!(subjects(&switcher), [subject_of(&repo).to_string()]);
}

#[test]
fn a_folder_that_several_sources_name_is_one_row() {
    let repo = shop_on_feature();
    make_the_review(&repo);
    let pr: ReviewSubject = SHOP_7.parse().unwrap();
    ReviewStateFile::for_subject(&repo.folders(), &pr)
        .remember_folder(repo.path())
        .unwrap();
    let agents = FakeTourAgent::new().claude_in("t1", &repo.path().display().to_string());

    let switcher = found(
        &repo.folders(),
        &clone_named_in_config(repo.path()),
        &agents,
    );

    assert_eq!(subjects(&switcher), [subject_of(&repo).to_string()]);
}

#[test]
fn a_review_on_disk_is_listed_though_its_folder_is_on_another_branch_now() {
    let repo = shop_on_feature();
    make_the_review(&repo);
    let review = subject_of(&repo);
    repo.git(&["checkout", "-q", "main"]);
    let (folders, config, agents) = nothing_listed_from(repo.home());

    let switcher = found(&folders, &config, &agents);

    let row = the_only_row(&switcher);
    assert_eq!(row.subject, review);
    let local = row.local().unwrap();
    assert_eq!(
        local.on_disk,
        Some(OnDisk::OnAnotherBranch(Some(
            guided_review::review_subject::BranchName::new("main").unwrap()
        )))
    );
    assert_eq!(local.disk_label(), Some("on another branch"));
}

#[test]
fn a_review_of_a_folder_that_changed_says_changed_on_disk() {
    let repo = shop_on_feature();
    make_the_review(&repo);
    repo.write("another.txt", "more\n");
    let (folders, config, agents) = nothing_listed_from(repo.home());

    let switcher = found(&folders, &config, &agents);

    let local = the_only_row(&switcher).local().unwrap();
    assert_eq!(local.disk_label(), Some("changed on disk"));
    assert!(!local.offers_drop_review());
}

#[test]
fn a_review_whose_branch_is_gone_offers_drop_review_and_dropping_removes_its_row() {
    let repo = shop_on_feature();
    make_the_review(&repo);
    let review = subject_of(&repo);
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["branch", "-D", "feature"]);
    let (folders, config, agents) = nothing_listed_from(repo.home());
    let mut switcher = found(&folders, &config, &agents);

    assert_eq!(switcher.drop_offer(0), Some(review.clone()));
    assert_eq!(
        the_only_row(&switcher).local().unwrap().disk_label(),
        Some("branch gone")
    );
    assert_eq!(
        switcher.pick(0).unwrap(),
        Pick::Show(review.clone()),
        "it still opens"
    );

    switcher.dropped(&review);

    assert_eq!(subjects(&switcher), Vec::<String>::new());
}

#[test]
fn a_row_whose_branch_is_there_offers_no_drop_review() {
    let repo = shop_on_feature();
    let switcher = found(
        &repo.folders(),
        &clone_named_in_config(repo.path()),
        &FakeTourAgent::new(),
    );

    assert_eq!(switcher.drop_offer(0), None);
}

#[test]
fn a_row_labels_the_review_it_opens_like_a_pull_request_row_does() {
    let repo = shop_on_feature();
    let config = clone_named_in_config(repo.path());
    let folders = repo.folders();
    let mut switcher = found(&folders, &config, &FakeTourAgent::new());
    let label = |switcher: &PrSwitcher| switcher.tour_label(the_only_row(switcher));
    assert_eq!(label(&switcher), None);

    make_the_review(&repo);
    let subject = subject_of(&repo);
    let guided = Tour::path_for(&folders, &subject);
    std::fs::create_dir_all(guided.parent().unwrap()).unwrap();
    std::fs::write(guided, "{}").unwrap();
    switcher.check_tours(&folders, &ActivePr::default());

    assert_eq!(label(&switcher), Some("guided ready"));
    assert_eq!(
        the_only_row(&switcher).tour.on_disk,
        ToursOnDisk::BasicWithGuided
    );
}

#[test]
fn a_row_says_when_the_app_is_writing_its_guided_tour() {
    let repo = shop_on_feature();
    let folders = repo.folders();
    let mut switcher = found(
        &folders,
        &clone_named_in_config(repo.path()),
        &FakeTourAgent::new(),
    );
    let mut active = ActivePr::default();
    active.write(subject_of(&repo));

    switcher.check_tours(&folders, &active);

    assert_eq!(
        switcher.tour_label(the_only_row(&switcher)),
        Some("writing")
    );
}

#[test]
fn typing_filters_local_rows_by_repo_branch_or_folder() {
    let repo = shop_on_feature();
    let mut switcher = found(
        &repo.folders(),
        &clone_named_in_config(repo.path()),
        &FakeTourAgent::new(),
    );
    let one = [subject_of(&repo).to_string()];

    for typed in ["acme/shop", "FEATURE", "local", "shop feature"] {
        switcher.filter(typed);
        assert_eq!(subjects(&switcher), one, "{typed}");
    }
    for typed in ["develop", "acme/blog"] {
        switcher.filter(typed);
        assert_eq!(subjects(&switcher), Vec::<String>::new(), "{typed}");
    }
}

fn github_row() -> RelevantPullRequests {
    RelevantPullRequests {
        pull_requests: vec![RelevantPullRequest {
            url: SHOP_7.parse().unwrap(),
            title: "Add checkout".to_owned(),
            author: PrAuthor {
                login: "alice".into(),
                avatar_url: None,
            },
            is_draft: false,
            updated_at: "2026-11-06T07:00:00Z".to_owned(),
            why_listed: WhyListed::Involved,
        }],
        teams_left_out: Vec::new(),
    }
}

#[test]
fn local_rows_come_after_the_pull_requests_and_a_new_github_list_keeps_them() {
    let repo = shop_on_feature();
    let folders = repo.folders();
    let mut switcher = PrSwitcher::default();
    switcher.loaded(Ok(github_row()), &folders);
    assert_eq!(
        switcher.visible().count(),
        1,
        "PR rows show before the disk is read"
    );

    switcher.local_found(
        find_local_rows(
            &folders,
            &clone_named_in_config(repo.path()),
            &FakeTourAgent::new(),
            &NoHost::default(),
        ),
        &folders,
    );
    switcher.loaded(Ok(github_row()), &folders);

    assert_eq!(
        subjects(&switcher),
        [SHOP_7.to_owned(), subject_of(&repo).to_string()]
    );
    let kinds: Vec<_> = switcher
        .visible()
        .map(|row| row.pull_request().is_some())
        .collect();
    assert_eq!(kinds, [true, false]);
}

#[test]
fn a_local_row_sits_with_the_pull_requests_of_its_repo() {
    let repo = shop_on_feature();
    let folders = repo.folders();
    let mut switcher = PrSwitcher::default();
    let mut listed = github_row();
    let mut blog = listed.pull_requests[0].clone();
    blog.url = BLOG_3.parse().unwrap();
    listed.pull_requests.push(blog);
    switcher.loaded(Ok(listed), &folders);

    switcher.local_found(
        find_local_rows(
            &folders,
            &clone_named_in_config(repo.path()),
            &FakeTourAgent::new(),
            &NoHost::default(),
        ),
        &folders,
    );

    assert_eq!(
        subjects(&switcher),
        [
            SHOP_7.to_owned(),
            subject_of(&repo).to_string(),
            BLOG_3.to_owned()
        ]
    );
}

#[test]
fn picking_a_local_row_opens_its_review() {
    let repo = shop_on_feature();
    let switcher = found(
        &repo.folders(),
        &clone_named_in_config(repo.path()),
        &FakeTourAgent::new(),
    );

    assert_eq!(switcher.pick(0).unwrap(), Pick::Show(subject_of(&repo)));
}

#[test]
fn a_local_row_whose_branch_has_an_open_pr_says_so_and_one_without_says_nothing() {
    let repo = shop_on_feature();
    let config = clone_named_in_config(repo.path());
    let sent = SentLog::default();
    let host = FakeHost::with_pull("main-tip", "head", &sent).with_open_pull_from(
        "feature",
        SHOP_7,
        "Add the cart",
    );

    let with_a_pr = {
        let mut switcher = PrSwitcher::default();
        switcher.local_found(
            find_local_rows(&repo.folders(), &config, &FakeTourAgent::new(), &host),
            &repo.folders(),
        );
        switcher
    };
    let without = found(&repo.folders(), &config, &FakeTourAgent::new());

    assert_eq!(
        the_only_row(&with_a_pr).local().unwrap().open_pr,
        Some(SHOP_7.parse().unwrap())
    );
    assert_eq!(the_only_row(&without).local().unwrap().open_pr, None);
}

#[test]
fn a_local_review_that_declined_the_move_to_its_open_pr_shows_no_chip_of_it() {
    let repo = shop_on_feature();
    let sent = SentLog::default();
    let host = FakeHost::with_pull("main-tip", "head", &sent).with_open_pull_from(
        "feature",
        SHOP_7,
        "Add the cart",
    );
    let mut opened =
        guided_review::open_review::open_review(&subject_of(&repo), repo.path(), &repo.folders())
            .unwrap();
    opened.review.pull_from_host();
    crate::support::work::drive_without_agents(
        &mut opened.review,
        &host,
        opened.source.as_ref(),
        std::time::Instant::now(),
    );
    opened.review.decline_the_move();
    assert_eq!(opened.review.problems(), []);

    let rows = find_local_rows(
        &repo.folders(),
        &Config::default(),
        &FakeTourAgent::new(),
        &host,
    );

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].open_pr, None);
}
