use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
use guided_review::update::InTheWay;
use guided_review::viewer::{Problem, ProblemKind, Read, Review, ReviewStateFile};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn open_review() -> (FixtureRepo, Review) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.txt", "old\n");
    let base = repo.commit("base");
    repo.write("a.txt", "new\n");
    let head = repo.commit("head");
    let tour: Tour = serde_json::from_value(json!({
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
    let state_file = ReviewStateFile::for_pr(repo.home(), &PR.parse().unwrap());
    let review = Review::open(tour, &source, state_file).unwrap();
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

    review.reload_failed(&anyhow::anyhow!("no tour file"));

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
fn a_failed_show_agent_is_listed_with_the_text_the_sidebar_always_gave_it() {
    let (_repo, mut review) = open_review();

    review.raise_problem(Problem::ShowAgentFailed("no terminal".into()));

    let [problem] = review.problems().try_into().unwrap();
    assert_eq!(problem.to_string(), "Could not show the agent: no terminal");

    review.clear_problem(ProblemKind::ShowAgentFailed);
    assert_eq!(review.problems(), []);
}
