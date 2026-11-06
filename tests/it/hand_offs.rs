use crate::support::FixtureRepo;
use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubFault, SentLog};
use crate::support::{file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    Audience, CommentId, CommentPlace, CommentResult, HandOffState, LineLocation, Review,
    ReviewStateFile, ReviewThread, SendMode, Side, ThreadComment, ThreadId,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        let numbered = |change: Option<&str>| -> String {
            (1..=20)
                .map(|number| match (number, change) {
                    (10, Some(text)) => format!("{text}\n"),
                    _ => format!("line {number}\n"),
                })
                .collect()
        };
        repo.write("notes.txt", &numbered(None));
        let base = repo.commit("base");
        repo.write("notes.txt", &numbered(Some("line ten")));
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent)
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
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &pr);
        let mut review = Review::open(tour, &source, state_file).unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review
    }
}

fn comment(id: &str, author: &str, body: &str) -> ThreadComment {
    ThreadComment {
        id: CommentId::new(id),
        author: author.into(),
        is_bot: false,
        body: body.into(),
        url: String::new(),
        reactions: Vec::new(),
    }
}

fn thread_on_line_ten(id: &str, comments: Vec<ThreadComment>) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new(id),
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
        comments,
    }
}

fn comment_on_line_ten(review: &mut Review, body: &str) -> CommentPlace {
    let row: LineLocation = review
        .current_step()
        .unwrap()
        .pieces
        .iter()
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .find(|row| row.text == "line ten")
        .unwrap()
        .location
        .clone();
    review.begin_selection(row.clone());
    review.extend_selection(row);
    let place = review.finish_selection().unwrap().unwrap();
    review.edit_draft(&place, body).unwrap();
    place
}

fn send(
    review: &mut Review,
    place: &CommentPlace,
    mode: SendMode,
    audience: Audience,
    github: &FakeGitHub,
) -> CommentResult {
    let request = review.comment_request_for(place, mode, audience).unwrap();
    review.apply_comment(request.run(github)).unwrap()
}

#[test]
fn a_comment_sent_with_ask_agent_is_handed_off_whichever_way_it_is_posted() {
    for mode in [SendMode::AddToReview, SendMode::CommentNow] {
        let pull = Pull::new();
        let mut github = pull.github();
        github.threads = vec![thread_on_line_ten(
            "T1",
            vec![comment("C1", "me", "why ten?")],
        )];
        let mut review = pull.open();
        let place = comment_on_line_ten(&mut review, "why ten?");

        let result = send(&mut review, &place, mode, Audience::Agent, &github);

        assert_eq!(result, CommentResult::Sent);
        let [hand_off] = review.hand_offs() else {
            panic!("one hand-off expected for {mode:?}");
        };
        assert_eq!(hand_off.comment(), "why ten?");
        assert_eq!(hand_off.thread(), Some(&ThreadId::new("T1")));
        assert_eq!(hand_off.mode(), mode);
        assert_eq!(hand_off.state(), &HandOffState::Queued);
    }
}

fn github_with_the_comment_as_a_thread(pull: &Pull, body: &str) -> FakeGitHub {
    let mut github = pull.github();
    github.threads = vec![thread_on_line_ten("T1", vec![comment("C1", "me", body)])];
    github
}

#[test]
fn a_comment_sent_with_the_plain_action_is_never_handed_off() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "nit");
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "nit");

    let result = send(
        &mut review,
        &place,
        SendMode::AddToReview,
        Audience::Author,
        &github,
    );

    assert_eq!(result, CommentResult::Sent);
    assert_eq!(review.hand_offs(), []);
}

#[test]
fn a_comment_github_refused_is_not_handed_off() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    github.inject(GitHubFault::PendingReviewAppearsOnCommentNow);
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");

    let result = send(
        &mut review,
        &place,
        SendMode::CommentNow,
        Audience::Agent,
        &github,
    );

    assert!(matches!(result, CommentResult::Refused(_)));
    assert_eq!(review.hand_offs(), []);
}

#[test]
fn an_asked_comment_waiting_in_the_queue_is_handed_off_once_the_queue_gets_it_to_github() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    let offline = pull.github();
    offline.inject(GitHubFault::ConnectionLost);
    let result = send(
        &mut review,
        &place,
        SendMode::AddToReview,
        Audience::Agent,
        &offline,
    );
    assert!(matches!(result, CommentResult::Queued(_)));
    assert_eq!(
        review.hand_offs(),
        [],
        "GitHub does not have the comment yet"
    );

    review
        .send_queued_comments(&github_with_the_comment_as_a_thread(&pull, "why ten?"))
        .unwrap();

    let [hand_off] = review.hand_offs() else {
        panic!("the queue sent the comment, so it is handed off");
    };
    assert_eq!(hand_off.thread(), Some(&ThreadId::new("T1")));
}

#[test]
fn a_hand_off_whose_thread_cannot_be_read_fails_and_says_why() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    github.inject(GitHubFault::ThreadReadsFail);
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");

    send(
        &mut review,
        &place,
        SendMode::AddToReview,
        Audience::Agent,
        &github,
    );

    let [hand_off] = review.hand_offs() else {
        panic!("the comment is on GitHub, so it is handed off");
    };
    let HandOffState::Failed(reason) = hand_off.state() else {
        panic!("{:?}", hand_off.state());
    };
    assert!(reason.contains("network is down"), "{reason}");
}

#[test]
fn hand_offs_survive_a_restart() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    send(
        &mut review,
        &place,
        SendMode::AddToReview,
        Audience::Agent,
        &github,
    );

    let reopened = pull.open();

    assert_eq!(reopened.hand_offs(), review.hand_offs());
    assert_eq!(reopened.hand_offs().len(), 1);
}
