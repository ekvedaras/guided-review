use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::RepoName;
use guided_review::tour::Tour;
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
fn a_failed_show_agent_is_listed_with_the_text_the_sidebar_always_gave_it() {
    let (_repo, mut review) = open_review();

    review.raise_problem(Problem::ShowAgentFailed("no terminal".into()));

    let [problem] = review.problems().try_into().unwrap();
    assert_eq!(problem.to_string(), "Could not show the agent: no terminal");

    review.clear_problem(ProblemKind::ShowAgentFailed);
    assert_eq!(review.problems(), []);
}
