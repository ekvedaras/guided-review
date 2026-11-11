use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{NotesButton, Review, ReviewStateFile, StepId, with_diagram_in_notes};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";
const DIAGRAM: &str = "flowchart TD\n  A[Cart] --> B[Checkout]";
const IN_NOTES: &str = "<details>\n<summary>Checkout flow</summary>\n\n```mermaid\nflowchart TD\n  A[Cart] --> B[Checkout]\n```\n\n</details>";

fn step() -> StepId {
    StepId::from("checkout")
}

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("cart.txt", "cart\n");
        let base = repo.commit("base");
        repo.write("cart.txt", "cart\ncheckout\n");
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn github(&self, description: &str) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent).with_description(description)
    }

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Checkout",
            "summary": [],
            "steps": [
                {"id": "checkout", "title": "Checkout flow", "why": "w", "diagram": DIAGRAM, "pieces": [{"file": "cart.txt"}]},
                {"id": "plain", "title": "Plain", "why": "w"},
            ],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let pr: PullRequestUrl = PR.parse().unwrap();
        Review::open(
            tour,
            &source,
            ReviewStateFile::for_pr(self.repo.home(), &pr),
        )
        .unwrap()
    }
}

#[test]
fn the_diagram_goes_at_the_end_of_the_notes_section() {
    let body = "## Why\n\nBecause.\n\n## Notes\n\nKeep the old cart.\n\n## Merge Danger\n\nNone.";

    assert_eq!(
        with_diagram_in_notes(body, "Checkout flow", DIAGRAM).unwrap(),
        format!(
            "## Why\n\nBecause.\n\n## Notes\n\nKeep the old cart.\n\n{IN_NOTES}\n\n## Merge Danger\n\nNone."
        )
    );
}

#[test]
fn a_title_with_html_in_it_shows_as_text_in_the_summary() {
    let changed = with_diagram_in_notes("", "Cart <A> & B", DIAGRAM).unwrap();

    assert!(
        changed.contains("<summary>Cart &lt;A&gt; &amp; B</summary>"),
        "{changed}"
    );
}

#[test]
fn a_description_with_no_notes_section_gets_one_at_its_end() {
    assert_eq!(
        with_diagram_in_notes("## Why\n\nBecause.\n", "Checkout flow", DIAGRAM).unwrap(),
        format!("## Why\n\nBecause.\n\n## Notes\n\n{IN_NOTES}\n")
    );
    assert_eq!(
        with_diagram_in_notes("", "Checkout flow", DIAGRAM).unwrap(),
        format!("## Notes\n\n{IN_NOTES}\n")
    );
}

#[test]
fn the_notes_section_runs_past_deeper_headings_and_headings_in_code() {
    let body = "## Notes\n\n### Later\n\nx\n\n```md\n## Fake\n```\n\n# Top";

    assert_eq!(
        with_diagram_in_notes(body, "Checkout flow", DIAGRAM).unwrap(),
        format!("## Notes\n\n### Later\n\nx\n\n```md\n## Fake\n```\n\n{IN_NOTES}\n\n# Top")
    );
}

#[test]
fn a_description_written_on_the_web_keeps_its_windows_line_ends() {
    let body = "## Notes\r\n\r\nKeep it.\r\n";

    assert_eq!(
        with_diagram_in_notes(body, "Checkout flow", DIAGRAM).unwrap(),
        format!(
            "## Notes\r\n\r\nKeep it.\r\n\r\n{}\r\n",
            IN_NOTES.replace('\n', "\r\n")
        )
    );
}

#[test]
fn a_diagram_already_in_the_description_is_not_added_again() {
    let body = format!("## Notes\n\n{IN_NOTES}\n");

    assert_eq!(with_diagram_in_notes(&body, "Checkout flow", DIAGRAM), None);
}

#[test]
fn the_button_adds_the_diagram_to_the_description_read_just_before() {
    let shop = Shop::new();
    let github = shop.github("## Notes\n\nKeep the old cart.");
    let mut review = shop.open();
    review.set_pr_description("stale").unwrap();
    assert_eq!(review.diagram_notes_button(&step()), Some(NotesButton::Add));
    assert_eq!(review.diagram_notes_button(&StepId::from("plain")), None);

    let request = review.diagram_to_notes(&step()).unwrap();
    assert_eq!(
        review.diagram_notes_button(&step()),
        Some(NotesButton::Adding)
    );
    assert!(
        review.diagram_to_notes(&step()).is_none(),
        "one add at a time"
    );
    review.apply_diagram_to_notes(request.run(&github)).unwrap();

    let expected = format!("## Notes\n\nKeep the old cart.\n\n{IN_NOTES}\n");
    assert_eq!(github.description_now(), expected);
    assert!(
        shop.sent
            .github_calls()
            .contains(&GitHubCall::SetDescription {
                pr: PR.parse().unwrap(),
                body: expected,
            })
    );
    assert_eq!(
        review.diagram_notes_button(&step()),
        Some(NotesButton::InNotes)
    );
}

#[test]
fn a_diagram_found_in_the_fresh_description_is_not_written_again() {
    let shop = Shop::new();
    let github = shop.github(&format!("## Notes\n\n{IN_NOTES}\n"));
    let mut review = shop.open();

    let request = review.diagram_to_notes(&step()).unwrap();
    review.apply_diagram_to_notes(request.run(&github)).unwrap();

    assert!(
        !shop
            .sent
            .github_calls()
            .iter()
            .any(|call| matches!(call, GitHubCall::SetDescription { .. }))
    );
    assert_eq!(
        review.diagram_notes_button(&step()),
        Some(NotesButton::InNotes)
    );
}

#[test]
fn a_failed_add_says_why_and_can_be_tried_again() {
    let shop = Shop::new();
    let github = shop.github("");
    github.inject(GitHubFault::ConnectionLost);
    let mut review = shop.open();

    let request = review.diagram_to_notes(&step()).unwrap();
    review.apply_diagram_to_notes(request.run(&github)).unwrap();

    let Some(NotesButton::Failed(reason)) = review.diagram_notes_button(&step()) else {
        panic!("{:?}", review.diagram_notes_button(&step()));
    };
    assert!(reason.contains("network is down"), "{reason}");
    github.heal(&GitHubFault::ConnectionLost);
    let request = review.diagram_to_notes(&step()).unwrap();
    review.apply_diagram_to_notes(request.run(&github)).unwrap();
    assert_eq!(
        review.diagram_notes_button(&step()),
        Some(NotesButton::InNotes)
    );
}
