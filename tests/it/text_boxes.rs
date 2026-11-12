//! The four text boxes the reviewer writes in, through one vocabulary: a draft comment on lines,
//! a reply to a thread, an edit of one of the reviewer's comments, and the finished review's body.

use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::{FixtureRepo, file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    CommentId, PieceDiff, Review, ReviewStateFile, ReviewThread, Side, TextBox, ThreadComment,
    ThreadId,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

/// `notes.txt` changes line ten; GitHub has one thread on it, started by the reviewer.
struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        let numbered = |ten: &str| -> String {
            (1..=20)
                .map(|number| match number {
                    10 => format!("{ten}\n"),
                    _ => format!("line {number}\n"),
                })
                .collect()
        };
        repo.write("notes.txt", &numbered("line 10"));
        let base = repo.commit("base");
        repo.write("notes.txt", &numbered("line ten"));
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn github(&self) -> FakeGitHub {
        let mut github = FakeGitHub::with_pull("main-tip", &self.head, &self.sent);
        github.threads = vec![my_thread_on_line_ten()];
        github
    }

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Notes",
            "summary": [],
            "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": [{"file": "notes.txt"}], "children": []}],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let pr: PullRequestUrl = PR.parse().unwrap();
        let mut review = Review::open(
            tour,
            &source,
            ReviewStateFile::for_pr(self.repo.home(), &pr),
        )
        .unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review.set_threads(&self.github().threads);
        review
    }
}

fn my_thread_on_line_ten() -> ReviewThread {
    ReviewThread {
        id: ThreadId::new("T1"),
        is_resolved: false,
        is_outdated: false,
        path: file_path("notes.txt"),
        line: Some(line(10)),
        start_line: None,
        side: Side::Head,
        is_file_comment: false,
        original_line: Some(line(10)),
        original_start_line: None,
        original_commit: None,
        comments: vec![ThreadComment {
            id: CommentId::new("C1"),
            author: "me".into(),
            is_bot: false,
            by_viewer: true,
            body: "why?".into(),
            url: format!("{PR}#discussion_C1"),
            reactions: Vec::new(),
        }],
    }
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Draft,
    Reply,
    Edit,
    FinishBody,
}

const EVERY_KIND: [Kind; 4] = [Kind::Draft, Kind::Reply, Kind::Edit, Kind::FinishBody];

/// Opens the box of `kind` the way the reviewer does: a draft from a click on line ten, the others
/// from their button.
fn open_box(review: &mut Review, kind: Kind) -> TextBox {
    let text_box = match kind {
        Kind::Draft => {
            let row = review
                .current_step()
                .unwrap()
                .pieces
                .iter()
                .flat_map(PieceDiff::lines)
                .find(|row| row.text == "line ten")
                .unwrap()
                .location
                .clone();
            review.begin_selection(row);
            let place = review.finish_selection().unwrap().unwrap();
            return TextBox::Draft(place);
        }
        Kind::Reply => TextBox::Reply(ThreadId::new("T1")),
        Kind::Edit => TextBox::Edit(CommentId::new("C1")),
        Kind::FinishBody => TextBox::FinishBody,
    };
    review.open_box(&text_box).unwrap();
    text_box
}

/// An edit is not saved, so there may be nothing to save.
fn save_what_was_typed(review: &mut Review) {
    if let Some(save) = review.typing_save() {
        save.run().unwrap();
    }
}

#[test]
fn what_is_typed_in_a_box_is_its_text() {
    for kind in EVERY_KIND {
        let pull = Pull::new();
        let mut review = pull.open();
        let text_box = open_box(&mut review, kind);

        review.type_in(&text_box, "why ten?").unwrap();

        assert_eq!(review.text_of(&text_box), Some("why ten?"), "{kind:?}");
    }
}

#[test]
fn an_edit_opens_on_the_text_of_the_comment() {
    let pull = Pull::new();
    let mut review = pull.open();

    let edit = open_box(&mut review, Kind::Edit);

    assert_eq!(review.text_of(&edit), Some("why?"));
}

#[test]
fn a_saved_box_comes_back_with_its_text_after_a_restart_and_an_edit_does_not() {
    for (kind, kept) in [
        (Kind::Draft, Some("why ten?")),
        (Kind::Reply, Some("why ten?")),
        (Kind::FinishBody, Some("why ten?")),
        (Kind::Edit, None),
    ] {
        let pull = Pull::new();
        let mut review = pull.open();
        let text_box = open_box(&mut review, kind);
        review.type_in(&text_box, "why ten?").unwrap();
        save_what_was_typed(&mut review);

        assert_eq!(pull.open().text_of(&text_box), kept, "{kind:?}");
    }
}

#[test]
fn a_discarded_box_is_gone_with_its_text_also_from_the_state_file() {
    for kind in EVERY_KIND {
        let pull = Pull::new();
        let mut review = pull.open();
        let text_box = open_box(&mut review, kind);
        review.type_in(&text_box, "why ten?").unwrap();
        save_what_was_typed(&mut review);

        review.discard(&text_box).unwrap();

        assert_eq!(review.text_of(&text_box), None, "{kind:?}");
        assert_eq!(pull.open().text_of(&text_box), None, "{kind:?} in the file");
    }
}

#[test]
fn leaving_a_box_drops_it_when_nothing_was_written_and_keeps_it_when_something_was() {
    for kind in [Kind::Draft, Kind::Reply, Kind::Edit] {
        let pull = Pull::new();
        let mut review = pull.open();
        let blank = open_box(&mut review, kind);
        review.type_in(&blank, "  \n ").unwrap();

        review.leave(&blank).unwrap();

        assert_eq!(review.text_of(&blank), None, "blank {kind:?}");
        let written = open_box(&mut review, kind);
        review.type_in(&written, "why ten?").unwrap();

        review.leave(&written).unwrap();

        assert_eq!(review.text_of(&written), Some("why ten?"), "{kind:?}");
    }
}
