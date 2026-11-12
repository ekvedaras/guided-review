//! The four text boxes the reviewer writes in, through one vocabulary: a draft comment on lines,
//! a reply to a thread, an edit of one of the reviewer's comments, and the finished review's body.

use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubFault, SentLog};
use crate::support::{FixtureRepo, file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    Audience, BoxState, CommentGesture, CommentId, CommentPlace, CommentProblem, CommentRefusal,
    PieceDiff, Review, ReviewStateFile, ReviewThread, SendMode, Side, TextBox, ThreadComment,
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
            let place = review
                .open_comment_box(CommentGesture::DragEnded)
                .unwrap()
                .unwrap();
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

fn written_comment_box(review: &mut Review) -> CommentPlace {
    let TextBox::Draft(place) = open_box(review, Kind::Draft) else {
        panic!("a click opens a comment box");
    };
    review
        .type_in(&TextBox::Draft(place.clone()), "why ten?")
        .unwrap();
    place
}

fn state_of(review: &Review, place: &CommentPlace) -> Option<(BoxState, Option<CommentProblem>)> {
    review
        .comment_box(place)
        .map(|comment_box| (comment_box.state, comment_box.problem))
}

#[test]
fn a_comment_box_opened_by_a_click_takes_the_keys_until_it_is_left() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = written_comment_box(&mut review);

    assert_eq!(review.composing(), Some(&place));
    assert_eq!(state_of(&review, &place), Some((BoxState::Composing, None)));

    review.leave(&TextBox::Draft(place.clone())).unwrap();

    assert_eq!(review.composing(), None);
    assert_eq!(state_of(&review, &place), Some((BoxState::Idle, None)));

    review.open_box(&TextBox::Draft(place.clone())).unwrap();

    assert_eq!(review.composing(), Some(&place));
}

#[test]
fn a_comment_shows_as_sending_while_it_is_on_its_way_to_github() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    let place = written_comment_box(&mut review);

    let request = review
        .start_sending(&place, SendMode::AddToReview, Audience::Author)
        .unwrap();

    assert_eq!(state_of(&review, &place), Some((BoxState::Sending, None)));
    review.apply_comment(request.run(&github)).unwrap();
}

#[test]
fn a_sent_or_queued_comment_closes_its_box() {
    for (fault, result) in [
        (None, "sent"),
        (Some(GitHubFault::ConnectionLost), "queued"),
    ] {
        let pull = Pull::new();
        let github = pull.github();
        let mut review = pull.open();
        let place = written_comment_box(&mut review);
        if let Some(fault) = fault {
            github.inject(fault);
        }

        review
            .send_comment(&place, SendMode::AddToReview, &github)
            .unwrap();

        assert_eq!(state_of(&review, &place), None, "{result}");
        assert_eq!(review.composing(), None, "{result}");
    }
}

#[test]
fn a_comment_github_rejects_keeps_its_box_with_the_reason() {
    let pull = Pull::new();
    let github = pull.github();
    github.inject(GitHubFault::NewThreadsAnswerNull);
    let mut review = pull.open();
    let place = written_comment_box(&mut review);

    review
        .send_comment(&place, SendMode::AddToReview, &github)
        .unwrap();

    let Some((BoxState::Composing, Some(CommentProblem::Rejected(_)))) = state_of(&review, &place)
    else {
        panic!("{:?}", state_of(&review, &place));
    };
    assert_eq!(review.text_of(&TextBox::Draft(place)), Some("why ten?"));
}

#[test]
fn a_refused_comment_keeps_its_box_with_the_refusal_until_it_is_sent_again() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = written_comment_box(&mut review);
    let moved_on = FakeGitHub::with_pull("main-tip", "newer-head", &pull.sent);

    review
        .send_comment(&place, SendMode::AddToReview, &moved_on)
        .unwrap();

    assert_eq!(
        state_of(&review, &place),
        Some((
            BoxState::Composing,
            Some(CommentProblem::Refused(CommentRefusal::TourIsOld))
        ))
    );

    review
        .start_sending(&place, SendMode::AddToReview, Audience::Author)
        .unwrap();

    assert_eq!(state_of(&review, &place), Some((BoxState::Sending, None)));
}

#[test]
fn a_comment_box_says_the_tour_is_old_while_the_pr_has_moved_past_it() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = written_comment_box(&mut review);

    review.pr_head_is(Sha::new("newer-head"));

    assert_eq!(
        state_of(&review, &place).and_then(|(_, problem)| problem),
        Some(CommentProblem::Refused(CommentRefusal::TourIsOld))
    );
}

#[test]
fn a_discarded_comment_box_takes_its_problem_with_it() {
    let pull = Pull::new();
    let github = pull.github();
    github.inject(GitHubFault::NewThreadsAnswerNull);
    let mut review = pull.open();
    let place = written_comment_box(&mut review);
    review
        .send_comment(&place, SendMode::AddToReview, &github)
        .unwrap();

    review.discard(&TextBox::Draft(place.clone())).unwrap();
    let place_again = written_comment_box(&mut review);

    assert_eq!(place_again, place);
    assert_eq!(state_of(&review, &place), Some((BoxState::Composing, None)));
}

#[derive(Debug, Clone, Copy)]
enum Gesture {
    ClickOnLineTen,
    CommentOnFile,
}

fn open_by(review: &mut Review, gesture: Gesture) -> Result<Option<CommentPlace>, CommentRefusal> {
    let notes = file_path("notes.txt");
    match gesture {
        Gesture::ClickOnLineTen => {
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
            review.open_comment_box(CommentGesture::DragEnded)
        }
        Gesture::CommentOnFile => review.open_comment_box(CommentGesture::OnFile(&notes)),
    }
}

#[test]
fn a_gesture_that_opens_a_comment_box_gives_it_the_keys() {
    for gesture in [Gesture::ClickOnLineTen, Gesture::CommentOnFile] {
        let pull = Pull::new();
        let mut review = pull.open();

        let place = open_by(&mut review, gesture).unwrap().unwrap();

        assert_eq!(review.composing(), Some(&place), "{gesture:?}");
        assert_eq!(
            review.text_of(&TextBox::Draft(place)),
            Some(""),
            "{gesture:?}"
        );
    }
}

#[test]
fn a_refused_gesture_says_why_and_opens_no_box() {
    for gesture in [Gesture::ClickOnLineTen, Gesture::CommentOnFile] {
        let pull = Pull::new();
        let mut review = pull.open();
        review.pr_head_is(Sha::new("newer-head"));

        let refused = open_by(&mut review, gesture);

        assert_eq!(refused, Err(CommentRefusal::TourIsOld), "{gesture:?}");
        assert_eq!(
            CommentRefusal::TourIsOld.to_string(),
            "Update tour to comment"
        );
        assert_eq!(review.comment_boxes().count(), 0, "{gesture:?}");
        assert_eq!(review.composing(), None, "{gesture:?}");
    }
}

#[test]
fn a_drag_that_ended_on_no_rows_opens_nothing_and_says_nothing() {
    let pull = Pull::new();
    let mut review = pull.open();

    assert_eq!(review.open_comment_box(CommentGesture::DragEnded), Ok(None));
}
