#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]
#![allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned arguments so a case reads as a plain call"
)]

#[allow(
    dead_code,
    reason = "each test binary uses only part of the shared test support"
)]
mod support;

use std::cell::RefCell;

use guided_review::eval::{
    Eval, EvalStore, EvalWriter, JudgeInput, Rule, Score, Skill, TourJudge, TourSource, Verdict,
    Verdicts, WriterBrief,
};
use guided_review::pull_request::PullRequestUrl;
use guided_review::tour::Tour;
use support::{FakeGitHub, FixtureRepo, GuidedReview};
use tempfile::TempDir;

const PR: &str = "https://github.com/acme/shop.app/pull/7";
const NOTE: &str = "Flow is cart to checkout to receipt. Needs a sequence diagram on checkout.";

struct FakeWriter<'a> {
    tours_dirs: RefCell<Vec<std::path::PathBuf>>,
    secret: &'a str,
    briefs: RefCell<Vec<String>>,
}

impl EvalWriter for FakeWriter<'_> {
    fn write_tour(&self, brief: &WriterBrief) -> anyhow::Result<()> {
        let seen = format!("{brief:?} {} {}", brief.pr, brief.skill);
        assert!(!seen.contains(self.secret), "the writer saw the note");
        self.briefs.borrow_mut().push(seen);
        self.tours_dirs
            .borrow_mut()
            .push(brief.tours_dir.to_path_buf());
        let path = Tour::path_in(brief.tours_dir, brief.pr);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, r#"{"title":"written by the fake writer","steps":[]}"#).unwrap();
        Ok(())
    }
}

struct FakeJudge {
    scores: RefCell<Vec<Score>>,
    inputs: RefCell<Vec<(String, String)>>,
}

impl FakeJudge {
    fn answering(order_scores: Vec<Score>) -> Self {
        Self {
            scores: RefCell::new(order_scores),
            inputs: RefCell::default(),
        }
    }
}

impl TourJudge for FakeJudge {
    fn judge(&self, input: &JudgeInput) -> anyhow::Result<Verdicts> {
        self.inputs
            .borrow_mut()
            .push((input.note.to_owned(), input.tour_json.to_owned()));
        let order = self.scores.borrow_mut().remove(0);
        Ok(Verdicts::from_pairs(Rule::ALL.map(|rule| {
            let score = if rule == Rule::Order {
                order
            } else {
                Score::Pass
            };
            (
                rule,
                Verdict {
                    score,
                    reason: format!("{rule} reason"),
                },
            )
        })))
    }
}

fn setup() -> (TempDir, EvalStore) {
    let home = TempDir::new().unwrap();
    let store = EvalStore::in_home(home.path());
    store
        .add(&PR.parse::<PullRequestUrl>().unwrap(), NOTE)
        .unwrap();
    (home, store)
}

fn skill(text: &str) -> Skill {
    Skill::new(text)
}

fn run_eval(
    home: &TempDir,
    store: &EvalStore,
    judge: &FakeJudge,
    skill: &Skill,
    source: TourSource,
    date: &str,
) -> (String, FakeWriter<'static>) {
    let writer = FakeWriter {
        tours_dirs: RefCell::default(),
        secret: NOTE,
        briefs: RefCell::default(),
    };
    let report = Eval {
        store,
        home: home.path(),
        writer: &writer,
        judge,
        skill,
    }
    .run(&source, date)
    .unwrap();
    (report.to_string(), writer)
}

#[test]
fn eval_add_stores_the_note_for_the_pr_under_the_evals_folder() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let github = FakeGitHub::serving_pull("acme/shop", 7, "a", "b");

    let run = GuidedReview::new(&repo, &github).run(&["eval", "add", PR, "--note", NOTE]);

    assert!(run.success, "{}", run.stderr);
    let stored = std::fs::read_to_string(
        repo.home()
            .join(".guided-review/evals/notes/acme/shop.app/7.md"),
    )
    .unwrap();
    assert_eq!(stored.trim(), NOTE);
}

#[test]
fn eval_without_a_test_set_says_how_to_add_a_pr() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let github = FakeGitHub::serving_pull("acme/shop", 7, "a", "b");

    let run = GuidedReview::new(&repo, &github).run(&["eval"]);

    assert!(!run.success);
    assert!(run.stderr.contains("eval add"), "{}", run.stderr);
}

#[test]
fn the_judge_gets_the_note_and_the_tour_while_the_writer_never_sees_the_note() {
    let (home, store) = setup();
    let judge = FakeJudge::answering(vec![Score::Pass]);

    let (_, writer) = run_eval(
        &home,
        &store,
        &judge,
        &skill("skill v1"),
        TourSource::WriteNew,
        "2026-10-22",
    );

    assert_eq!(writer.briefs.borrow().len(), 1);
    let inputs = judge.inputs.borrow();
    assert_eq!(inputs[0].0.trim(), NOTE);
    assert!(inputs[0].1.contains("written by the fake writer"));
}

#[test]
fn the_writer_is_pointed_at_a_run_folder_and_the_real_tour_is_never_touched() {
    let (home, store) = setup();
    let pr: PullRequestUrl = PR.parse().unwrap();
    let tour_path = Tour::path_for(home.path(), &pr);
    std::fs::create_dir_all(tour_path.parent().unwrap()).unwrap();
    std::fs::write(&tour_path, r#"{"title":"my real tour"}"#).unwrap();

    let (_, writer) = run_eval(
        &home,
        &store,
        &FakeJudge::answering(vec![Score::Pass]),
        &skill("skill v1"),
        TourSource::WriteNew,
        "2026-10-22",
    );

    let run_folder = home.path().join(".guided-review/evals/tours/2026-10-22");
    assert_eq!(*writer.tours_dirs.borrow(), vec![run_folder.clone()]);
    assert!(Tour::path_in(&run_folder, &pr).exists());
    assert_eq!(
        std::fs::read_to_string(&tour_path).unwrap(),
        r#"{"title":"my real tour"}"#
    );
    assert!(!tour_path.with_extension("json.eval-aside").exists());
}

#[test]
fn existing_tours_are_judged_without_running_the_writer() {
    let (home, store) = setup();
    let pr: PullRequestUrl = PR.parse().unwrap();
    let tour_path = Tour::path_for(home.path(), &pr);
    std::fs::create_dir_all(tour_path.parent().unwrap()).unwrap();
    std::fs::write(&tour_path, r#"{"title":"my real tour"}"#).unwrap();
    let judge = FakeJudge::answering(vec![Score::Pass]);

    let (_, writer) = run_eval(
        &home,
        &store,
        &judge,
        &skill("skill v1"),
        TourSource::ExistingTours,
        "2026-10-22",
    );

    assert!(writer.briefs.borrow().is_empty());
    assert!(judge.inputs.borrow()[0].1.contains("my real tour"));
}

#[test]
fn a_run_prints_its_scores_and_is_saved_with_date_and_skill_version() {
    let (home, store) = setup();

    let (report, _) = run_eval(
        &home,
        &store,
        &FakeJudge::answering(vec![Score::Partial]),
        &skill("skill v1"),
        TourSource::WriteNew,
        "2026-10-22",
    );

    assert!(report.contains("acme/shop.app#7"), "{report}");
    assert!(report.contains("order: partial - order reason"), "{report}");
    assert!(report.contains("skim: pass - skim reason"), "{report}");
    assert!(report.contains("first run"), "{report}");
    let saved: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            home.path()
                .join(".guided-review/evals/results/2026-10-22.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(saved["date"], "2026-10-22");
    assert_eq!(saved["skill_version"], skill("skill v1").version());
    assert_eq!(saved["prs"][PR]["order"]["score"], "partial");
}

#[test]
fn the_next_run_prints_what_changed_since_the_last_one() {
    let (home, store) = setup();
    run_eval(
        &home,
        &store,
        &FakeJudge::answering(vec![Score::Pass]),
        &skill("skill v1"),
        TourSource::WriteNew,
        "2026-10-22",
    );

    let (report, _) = run_eval(
        &home,
        &store,
        &FakeJudge::answering(vec![Score::Fail]),
        &skill("skill v2"),
        TourSource::WriteNew,
        "2026-10-23",
    );

    assert!(report.contains("since 2026-10-22"), "{report}");
    assert!(
        report.contains("acme/shop.app#7 order: pass -> fail - order reason"),
        "{report}"
    );
    assert!(!report.contains("grouping: pass -> "), "{report}");

    let (unchanged, _) = run_eval(
        &home,
        &store,
        &FakeJudge::answering(vec![Score::Fail]),
        &skill("skill v2"),
        TourSource::WriteNew,
        "2026-10-24",
    );
    assert!(unchanged.contains("no changes"), "{unchanged}");
}
