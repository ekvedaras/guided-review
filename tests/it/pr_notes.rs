use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    NoteSubject, NotesButton, Review, ReviewStateFile, StepId, with_diagram_in_notes,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";
const DIAGRAM: &str = "flowchart TD\n  A[Cart] --> B[Checkout]";
const IN_NOTES: &str = "<details>\n<summary>Checkout flow</summary>\n\n```mermaid\nflowchart TD\n  A[Cart] --> B[Checkout]\n```\n\n</details>";

fn diagram() -> NoteSubject {
    NoteSubject::StepDiagram(StepId::from("checkout"))
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
        self.open_with_summary("")
    }

    /// The tour's folder holds `before.png` and `explainer.html`.
    fn open_with_summary(&self, reviewer_summary: &str) -> Review {
        let files = self.repo.home().join("tour-files");
        std::fs::create_dir_all(&files).unwrap();
        image::RgbaImage::new(40, 20)
            .save(files.join("before.png"))
            .unwrap();
        std::fs::write(files.join("explainer.html"), "<html></html>").unwrap();
        let mut review = self.open_tour(reviewer_summary);
        review.set_tour_files(files);
        review
    }

    fn open_tour(&self, reviewer_summary: &str) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Checkout",
            "summary": [],
            "reviewer_summary": reviewer_summary,
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
    assert_eq!(review.notes_button(&diagram()), Some(NotesButton::Add));
    assert_eq!(
        review.notes_button(&NoteSubject::StepDiagram(StepId::from("plain"))),
        None
    );

    let request = review.start_adding_to_notes(&diagram()).unwrap();
    assert_eq!(review.notes_button(&diagram()), Some(NotesButton::Adding));
    assert!(
        review.start_adding_to_notes(&diagram()).is_none(),
        "one add at a time"
    );
    review.apply_to_notes(request.run(&github)).unwrap();

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
    assert_eq!(review.notes_button(&diagram()), Some(NotesButton::InNotes));
}

#[test]
fn a_diagram_found_in_the_fresh_description_is_not_written_again() {
    let shop = Shop::new();
    let github = shop.github(&format!("## Notes\n\n{IN_NOTES}\n"));
    let mut review = shop.open();

    let request = review.start_adding_to_notes(&diagram()).unwrap();
    review.apply_to_notes(request.run(&github)).unwrap();

    assert!(
        !shop
            .sent
            .github_calls()
            .iter()
            .any(|call| matches!(call, GitHubCall::SetDescription { .. }))
    );
    assert_eq!(review.notes_button(&diagram()), Some(NotesButton::InNotes));
}

#[test]
fn a_failed_add_says_why_and_can_be_tried_again() {
    let shop = Shop::new();
    let github = shop.github("");
    github.inject(GitHubFault::ConnectionLost);
    let mut review = shop.open();

    let request = review.start_adding_to_notes(&diagram()).unwrap();
    review.apply_to_notes(request.run(&github)).unwrap();

    let Some(NotesButton::Failed(reason)) = review.notes_button(&diagram()) else {
        panic!("{:?}", review.notes_button(&diagram()));
    };
    assert!(reason.contains("network is down"), "{reason}");
    github.heal(&GitHubFault::ConnectionLost);
    let request = review.start_adding_to_notes(&diagram()).unwrap();
    review.apply_to_notes(request.run(&github)).unwrap();
    assert_eq!(review.notes_button(&diagram()), Some(NotesButton::InNotes));
}

const UPLOADED: &str = "https://github.com/user-attachments/assets/before.png";

fn before_png(shop: &Shop) -> NoteSubject {
    NoteSubject::SummaryPicture(shop.repo.home().join("tour-files/before.png"))
}

fn uploads(shop: &Shop) -> Vec<String> {
    shop.sent
        .github_calls()
        .into_iter()
        .filter_map(|call| match call {
            GitHubCall::UploadPicture { name, .. } => Some(name),
            _ => None,
        })
        .collect()
}

/// The hidden comment of `text` that begins with `start`, whose digest the test does not compute.
fn hidden_mark(text: &str, start: &str) -> String {
    let (_, after) = text
        .split_once(start)
        .unwrap_or_else(|| panic!("no {start} in {text}"));
    let (digest, _) = after.split_once("-->").unwrap();
    format!("{start}{digest}-->")
}

#[test]
fn a_summary_picture_is_uploaded_and_added_folded_under_its_alt_text() {
    let shop = Shop::new();
    let github = shop.github("## Notes\n\nKeep it.");
    let mut review = shop.open_with_summary("Look:\n\n![before](before.png)");
    assert_eq!(
        review.notes_button(&before_png(&shop)),
        Some(NotesButton::Add)
    );

    let request = review.start_adding_to_notes(&before_png(&shop)).unwrap();
    review.apply_to_notes(request.run(&github)).unwrap();

    let description = github.description_now();
    let mark = hidden_mark(&description, "<!-- guided-review picture ");
    assert_eq!(
        description,
        format!(
            "## Notes\n\nKeep it.\n\n<details>\n<summary>before</summary>\n{mark}\n\n![before]({UPLOADED})\n\n</details>\n"
        )
    );
    assert_eq!(uploads(&shop), ["before.png"]);
    assert_eq!(
        review.notes_button(&before_png(&shop)),
        Some(NotesButton::InNotes)
    );
}

#[test]
fn a_picture_already_in_the_description_is_not_uploaded_again() {
    let shop = Shop::new();
    let github = shop.github("");
    let mut review = shop.open_with_summary("![before](before.png)");
    let request = review.start_adding_to_notes(&before_png(&shop)).unwrap();
    review.apply_to_notes(request.run(&github)).unwrap();
    let mut reopened = shop.open_with_summary("![before](before.png)");
    reopened.set_pr_description("stale").unwrap();

    let request = reopened.start_adding_to_notes(&before_png(&shop)).unwrap();
    reopened.apply_to_notes(request.run(&github)).unwrap();

    assert_eq!(uploads(&shop), ["before.png"]);
    assert_eq!(
        reopened.notes_button(&before_png(&shop)),
        Some(NotesButton::InNotes)
    );
}

#[test]
fn the_whole_summary_goes_in_folded_with_its_pictures_uploaded_and_its_pages_as_plain_text() {
    let shop = Shop::new();
    let github = shop.github("## Why\n\nBecause.");
    let summary = "## Like I'm 5\n\nA cart.\n\n![before](before.png)\n\nSee [the explainer](explainer.html) or [docs](https://x.test/d).";
    let mut review = shop.open_with_summary(summary);
    assert_eq!(
        review.notes_button(&NoteSubject::ReviewerSummary),
        Some(NotesButton::Add)
    );

    let request = review
        .start_adding_to_notes(&NoteSubject::ReviewerSummary)
        .unwrap();
    review.apply_to_notes(request.run(&github)).unwrap();

    let description = github.description_now();
    let mark = hidden_mark(&description, "<!-- guided-review summary ");
    assert_eq!(
        description,
        format!(
            "## Why\n\nBecause.\n\n## Notes\n\n<details>\n<summary>Reviewer summary</summary>\n{mark}\n\n\
             ## Like I'm 5\n\nA cart.\n\n![before]({UPLOADED})\n\nSee the explainer or [docs](https://x.test/d).\n\n</details>\n"
        )
    );
    assert_eq!(
        review.notes_button(&NoteSubject::ReviewerSummary),
        Some(NotesButton::InNotes)
    );
}

#[test]
fn a_tour_without_a_reviewer_summary_offers_no_summary_button() {
    let shop = Shop::new();
    let review = shop.open_with_summary("");

    assert_eq!(review.notes_button(&NoteSubject::ReviewerSummary), None);
    assert_eq!(review.notes_button(&before_png(&shop)), None);
}

#[test]
fn a_failed_upload_leaves_the_description_alone_and_says_why() {
    let shop = Shop::new();
    let github = shop.github("");
    github.inject(GitHubFault::ConnectionLost);
    let mut review = shop.open_with_summary("![before](before.png)");

    let request = review.start_adding_to_notes(&before_png(&shop)).unwrap();
    review.apply_to_notes(request.run(&github)).unwrap();

    let Some(NotesButton::Failed(reason)) = review.notes_button(&before_png(&shop)) else {
        panic!("{:?}", review.notes_button(&before_png(&shop)));
    };
    assert!(reason.contains("network is down"), "{reason}");
    assert_eq!(github.description_now(), "");
}

#[test]
fn a_note_added_after_a_folded_summary_goes_after_its_fold_not_into_its_headings() {
    let body = "## Notes\n\n<details>\n<summary>Reviewer summary</summary>\n\n## Like I'm 5\n\nA cart.\n\n</details>\n\n## Merge Danger\n\nNone.";

    assert_eq!(
        with_diagram_in_notes(body, "Checkout flow", DIAGRAM).unwrap(),
        format!(
            "## Notes\n\n<details>\n<summary>Reviewer summary</summary>\n\n## Like I'm 5\n\nA cart.\n\n</details>\n\n{IN_NOTES}\n\n## Merge Danger\n\nNone."
        )
    );
}
