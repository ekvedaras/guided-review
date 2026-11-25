use crate::support::FixtureRepo;
use guided_review::app_folders::NotRead;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::GuidedTour;
use guided_review::update::InTheWay;
use guided_review::viewer::{
    JobOutcome, Problem, ProblemKind, Read, ReplacingJob, Review, ReviewStateFile,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn open_review() -> (FixtureRepo, Review) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.txt", "old\n");
    let base = repo.commit("base");
    repo.write("a.txt", "new\n");
    let head = repo.commit("head");
    let tour: GuidedTour = serde_json::from_value(json!({
        "pr": PR,
        "base": base,
        "head": head,
        "title": "A change",
        "steps": [{"id": "a", "title": "A", "pieces": [{"file": "a.txt"}]}],
    }))
    .unwrap();
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_subject(&repo.folders(), &PR.parse().unwrap());
    let review = Review::open(
        tour,
        &source,
        state_file,
        crate::support::fake_ports::every_capability(),
    )
    .unwrap();
    (repo, review)
}

#[test]
fn a_review_with_nothing_wrong_lists_no_problem() {
    let (_repo, review) = open_review();

    assert_eq!(review.problems(), []);
}

#[test]
fn a_raised_problem_is_listed_until_its_kind_is_cleared() {
    let (_repo, mut review) = open_review();

    review.raise_problem(Problem::IdeNotStarted("no such command".into()));
    assert_eq!(
        review.problems(),
        [Problem::IdeNotStarted("no such command".into())]
    );

    review.clear_problem(ProblemKind::IdeNotStarted);
    assert_eq!(review.problems(), []);
}

#[test]
fn raising_a_kind_again_replaces_the_earlier_problem_of_that_kind() {
    let (_repo, mut review) = open_review();

    review.raise_problem(Problem::SwitchFailed("first".into()));
    review.raise_problem(Problem::SwitchFailed("second".into()));

    assert_eq!(review.problems(), [Problem::SwitchFailed("second".into())]);
}

#[test]
fn clearing_a_kind_leaves_the_other_kinds_listed() {
    let (_repo, mut review) = open_review();
    review.raise_problem(Problem::NotRead(Read::PrHead, "offline".into()));
    review.raise_problem(Problem::NotRead(Read::Authorship, "offline".into()));
    review.raise_problem(Problem::SwitchFailed("no tour".into()));

    review.clear_problem(ProblemKind::NotRead(Read::PrHead));

    assert_eq!(
        review.problems(),
        [
            Problem::NotRead(Read::Authorship, "offline".into()),
            Problem::SwitchFailed("no tour".into()),
        ]
    );
}

#[test]
fn problems_are_listed_in_the_order_of_their_kinds_not_of_when_they_were_raised() {
    let (_repo, mut review) = open_review();

    review.raise_problem(Problem::SwitchFailed("no tour".into()));
    review.raise_problem(Problem::IdeNotStarted("no such command".into()));
    review.raise_problem(Problem::ThreadsNotRead("offline".into()));

    assert_eq!(
        review.problems(),
        [
            Problem::ThreadsNotRead("offline".into()),
            Problem::IdeNotStarted("no such command".into()),
            Problem::SwitchFailed("no tour".into()),
        ]
    );
}

#[test]
fn a_tour_that_could_not_be_read_again_is_a_problem_that_says_the_screen_shows_it_as_it_was() {
    let (_repo, mut review) = open_review();

    review.start_job(ReplacingJob::Reload).unwrap();
    review.land_job(JobOutcome::Reload(Err(anyhow::anyhow!("no tour file"))));

    let [problem] = review.problems().try_into().unwrap();
    assert_eq!(problem, Problem::TourNotReloaded("no tour file".into()));
    assert_eq!(
        problem.to_string(),
        "The tour could not be read again, so it shows as it was: no tour file"
    );
}

#[test]
fn a_guided_review_before_the_release_install_on_path_names_the_binary_to_remove() {
    let (_repo, mut review) = open_review();

    review.raise_problem(Problem::ReleaseInstallInTheWay(InTheWay::FirstOnPath {
        binary: "/Users/me/.cargo/bin/guided-review".into(),
        release_install: "/Users/me/.local/bin/guided-review".into(),
    }));

    let [problem] = review.problems().try_into().unwrap();
    assert_eq!(
        problem.to_string(),
        "/Users/me/.cargo/bin/guided-review comes first on PATH, so the tour skill misses updates"
    );
}

#[test]
fn files_left_beside_the_app_folders_are_named_each_with_the_file_read_instead() {
    let (_repo, mut review) = open_review();
    let not_read = |file: &str, read_instead: &str| NotRead {
        file: file.into(),
        read_instead: read_instead.into(),
    };

    review.raise_problem(Problem::FilesNotRead(vec![
        not_read(
            "/home/me/.guided-review/config.toml",
            "/home/me/.config/guided-review/config.toml",
        ),
        not_read(
            "/home/me/.guided-review/tours/a/b/1.json",
            "/home/me/.local/state/guided-review/tours/a/b/1.json",
        ),
        not_read(
            "/home/me/.guided-review/tours/a/b/2.json",
            "/home/me/.local/state/guided-review/tours/a/b/2.json",
        ),
        not_read(
            "/home/me/.guided-review/tours/a/b/3.json",
            "/home/me/.local/state/guided-review/tours/a/b/3.json",
        ),
    ]));

    let [problem] = review.problems().try_into().unwrap();
    assert_eq!(
        problem.to_string(),
        "4 files are not read, as the app reads another file in their place; nothing was merged, overwritten or deleted, so keep what you need by hand: /home/me/.guided-review/config.toml (read instead: /home/me/.config/guided-review/config.toml), /home/me/.guided-review/tours/a/b/1.json (read instead: /home/me/.local/state/guided-review/tours/a/b/1.json), /home/me/.guided-review/tours/a/b/2.json (read instead: /home/me/.local/state/guided-review/tours/a/b/2.json) and 1 more"
    );
}

#[test]
fn a_failed_show_agent_is_listed_with_the_text_the_sidebar_always_gave_it() {
    let (_repo, mut review) = open_review();

    review.raise_problem(Problem::ShowAgentFailed("no terminal".into()));

    let [problem] = review.problems().try_into().unwrap();
    assert_eq!(problem.to_string(), "Could not show the agent: no terminal");

    review.clear_problem(ProblemKind::ShowAgentFailed);
    assert_eq!(review.problems(), []);
}

#[test]
fn a_failed_use_this_agent_is_listed_and_the_next_one_that_works_clears_it() {
    let (_repo, mut review) = open_review();

    review.use_agent_result(Some(&anyhow::anyhow!("no terminal t2")));

    let [problem] = review.problems().try_into().unwrap();
    assert_eq!(
        problem.to_string(),
        "Could not use that agent: no terminal t2"
    );

    review.use_agent_result(None);
    assert_eq!(review.problems(), []);
}
