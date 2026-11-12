use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::{FixtureRepo, drafts, file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentReplied, Audience, CommentGesture, CommentId, CommentPlace, HandOffId, HandOffState,
    LineLocation, PieceDiff, Problem, Review, ReviewStateFile, ReviewThread, SendMode, Side,
    StepId, TextBox, ThreadComment, ThreadId,
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

    fn state_file(&self) -> ReviewStateFile {
        let pr: PullRequestUrl = PR.parse().unwrap();
        ReviewStateFile::for_pr(self.repo.home(), &pr)
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
        let mut review = Review::open(tour, &source, self.state_file()).unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review
    }

    /// The draft on line ten, open and empty.
    fn draft_on_line_ten(&self, review: &mut Review) -> CommentPlace {
        let row: LineLocation = review
            .current_step()
            .unwrap()
            .pieces
            .iter()
            .flat_map(PieceDiff::lines)
            .find(|row| row.text == "line ten")
            .unwrap()
            .location
            .clone();
        review.begin_selection(row.clone());
        review.extend_selection(row);
        review
            .open_comment_box(CommentGesture::DragEnded)
            .unwrap()
            .unwrap()
    }

    /// A comment on line ten sent with Ask agent, so one hand-off is queued.
    fn hand_off(&self, review: &mut Review) -> HandOffId {
        let mut github = FakeGitHub::with_pull("main-tip", &self.head, &self.sent);
        github.threads = vec![thread_on_line_ten()];
        let place = self.draft_on_line_ten(review);
        review
            .type_in(&TextBox::Draft(place.clone()), "why ten?")
            .unwrap();
        let request = review
            .start_sending(&place, SendMode::CommentNow, Audience::Agent)
            .unwrap();
        review.apply_comment(request.run(&github)).unwrap();
        review.hand_offs().all().first().unwrap().id()
    }

    fn saved_hand_off_states(&self) -> Vec<HandOffState> {
        let saved = self.state_file().load().unwrap();
        saved
            .hand_offs()
            .all()
            .iter()
            .map(|hand_off| hand_off.state().clone())
            .collect()
    }
}

fn thread_on_line_ten() -> ReviewThread {
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
            body: "why ten?".into(),
            url: String::new(),
            reactions: Vec::new(),
            by_viewer: false,
        }],
    }
}

fn hand_off_states(review: &Review) -> Vec<HandOffState> {
    review
        .hand_offs()
        .all()
        .iter()
        .map(|hand_off| hand_off.state().clone())
        .collect()
}

fn notes() -> StepId {
    "notes".into()
}

#[test]
fn a_change_is_saved_at_once() {
    let pull = Pull::new();
    let mut review = pull.open();
    let hand_off = pull.hand_off(&mut review);

    review.tick(&notes()).unwrap();
    review
        .record_hand_offs(|hand_offs| hand_offs.replied(hand_off, &AgentReplied::default()))
        .unwrap()
        .unwrap();

    assert!(pull.open().is_ticked(&notes()));
    assert_eq!(pull.saved_hand_off_states(), [HandOffState::Answered]);
}

#[test]
fn while_a_re_pin_runs_the_reviewer_s_changes_are_refused_and_nothing_changes() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = pull.draft_on_line_ten(&mut review);
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();

    review.start_repin();
    let refused = [
        review.tick(&notes()),
        review.type_in(&TextBox::Draft(place), "why ten?"),
        review.type_in(&TextBox::Reply(ThreadId::new("T1")), "because"),
        review.type_in(&TextBox::FinishBody, "looks good"),
    ];

    assert!(refused.iter().all(Result::is_err));
    assert!(!review.is_ticked(&notes()));
    assert_eq!(drafts(&review).first().unwrap().body, "");
    assert_eq!(
        review.text_of(&TextBox::Reply(ThreadId::new("T1"))),
        Some("")
    );
    assert_eq!(review.text_of(&TextBox::FinishBody), None);
    assert!(review.typing_save().is_none(), "nothing typed to save");
    assert_eq!(review.problems(), [Problem::ChangeNotKeptWhileRepinning]);
    assert!(!pull.open().is_ticked(&notes()));
}

#[test]
fn while_a_re_pin_runs_what_the_tour_agent_did_is_kept_in_memory_but_not_saved() {
    let pull = Pull::new();
    let mut review = pull.open();
    let hand_off = pull.hand_off(&mut review);

    review.start_repin();
    let recorded =
        review.record_hand_offs(|hand_offs| hand_offs.replied(hand_off, &AgentReplied::default()));

    assert!(recorded.is_err());
    assert_eq!(hand_off_states(&review), [HandOffState::Answered]);
    assert_eq!(pull.saved_hand_off_states(), [HandOffState::Queued]);
    assert_eq!(review.problems(), [Problem::ChangeNotKeptWhileRepinning]);
}

#[test]
fn once_a_re_pin_failed_changes_are_saved_again_and_nothing_is_named_as_not_kept() {
    let pull = Pull::new();
    let mut review = pull.open();
    let hand_off = pull.hand_off(&mut review);
    review.start_repin();
    review.tick(&notes()).unwrap_err();

    review.repin_failed();
    review.tick(&notes()).unwrap();
    review
        .record_hand_offs(|hand_offs| hand_offs.replied(hand_off, &AgentReplied::default()))
        .unwrap()
        .unwrap();

    assert_eq!(review.problems(), []);
    assert!(pull.open().is_ticked(&notes()));
    assert_eq!(pull.saved_hand_off_states(), [HandOffState::Answered]);
}
