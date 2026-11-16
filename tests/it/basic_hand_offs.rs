use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::work::drive_until_idle;
use crate::support::{FixtureRepo, file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::{BasicTour, TourPin};
use guided_review::viewer::{
    AgentStatus, Audience, CommentGesture, CommentId, CommentPlace, CommentResult, HandOffState,
    Review, ReviewStateFile, ReviewThread, Screen, SendMode, Side, TerminalId, TextBox,
    ThreadComment, ThreadId, TourAgentPort, TourAgents, WorkPorts,
};
use std::sync::Arc;
use std::time::Instant;

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
        repo.write("notes.txt", "one\ntwo\nthree\n");
        let base = repo.commit("base");
        repo.write("notes.txt", "one\nTWO\nthree\n");
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour = BasicTour::new(
            TourPin {
                version: 1,
                pr: PR.to_owned(),
                base: Sha::new(&self.base),
                head: Sha::new(&self.head),
                previous_head: None,
            },
            "Orders cost two".to_owned(),
        );
        let state_file =
            ReviewStateFile::for_pr(&self.repo.folders(), &PR.parse::<PullRequestUrl>().unwrap());
        let mut review = Review::open_basic(tour, &self.source(), state_file).unwrap();
        while review.screen() != &Screen::File(file_path("notes.txt")) {
            assert!(review.go_forward());
        }
        review
    }

    fn deliver(&self, review: &mut Review, agents: &TourAgents) {
        let source = self.source();
        let elsewhere = FakeGitHub::with_pull("main-tip", "head", &SentLog::default());
        let ports = WorkPorts {
            github: &elsewhere,
            source: &source,
            agents,
        };
        drive_until_idle(review, &ports, Instant::now());
    }
}

fn thread_on_line_two(comments: Vec<ThreadComment>) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new("T1"),
        is_resolved: false,
        is_outdated: false,
        path: file_path("notes.txt"),
        line: Some(line(2)),
        start_line: None,
        side: Side::Head,
        is_file_comment: false,
        original_line: Some(line(2)),
        original_start_line: None,
        original_commit: None,
        comments,
    }
}

fn comment(body: &str) -> ThreadComment {
    ThreadComment {
        avatar_url: None,
        id: CommentId::new("C1"),
        reactions: Vec::new(),
        author: "me".into(),
        is_bot: false,
        by_viewer: true,
        body: body.into(),
        url: "https://github.com/acme/shop/pull/7#discussion_C1".into(),
    }
}

fn handed_off(pull: &Pull) -> Review {
    let mut github = FakeGitHub::with_pull("main-tip", &pull.head, &pull.sent);
    github.threads = vec![thread_on_line_two(vec![comment("why caps?")])];
    let mut review = pull.open();
    let at = review
        .file_page()
        .unwrap()
        .pieces
        .iter()
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .find(|row| row.text == "TWO")
        .unwrap()
        .location
        .clone();
    review.begin_selection(at.clone());
    review.extend_selection(at);
    let place: CommentPlace = review
        .open_comment_box(CommentGesture::DragEnded)
        .unwrap()
        .unwrap();
    review
        .type_in(&TextBox::Draft(place.clone()), "why caps?")
        .unwrap();
    let request = review
        .start_sending(&place, SendMode::AddToReview, Audience::Agent)
        .unwrap();
    assert_eq!(
        review.apply_comment(request.run(&github)).unwrap(),
        CommentResult::Sent
    );
    review
}

fn agents_of(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
}

#[test]
fn a_hand_off_from_a_file_screen_names_the_file_and_says_the_tour_is_basic() {
    let pull = Pull::new();
    let mut review = handed_off(&pull);
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));

    pull.deliver(&mut review, &agents_of(&fake));

    let calls = fake.calls();
    let [TourAgentCall::Send { terminal, text }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!(terminal, &TerminalId::new("t1"));
    for part in [
        PR,
        "why caps?",
        "File: notes.txt",
        "basic tour",
        "guided-review outline",
        "guided-review show",
        "guided-review agent-reply",
    ] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
    for gone in ["Step:", "fix the tour file", "guided-review check"] {
        assert!(!text.contains(gone), "{gone} is in {text}");
    }
    assert_eq!(
        review.hand_offs().all()[0].state(),
        &HandOffState::Delivered
    );
}

#[test]
fn with_no_agent_one_is_started_whose_first_task_says_the_tour_is_basic_and_points_at_outline_and_show()
 {
    let pull = Pull::new();
    let mut review = handed_off(&pull);
    let fake = Arc::new(FakeTourAgent::new());
    let agents = agents_of(&fake).starting_in("/clones/shop".into());

    pull.deliver(&mut review, &agents);

    let calls = fake.calls();
    let [TourAgentCall::Start { first_task, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    for part in [
        PR,
        "tour agent",
        "basic tour",
        "guided-review outline",
        "guided-review show",
    ] {
        assert!(
            first_task.contains(part),
            "{part} missing from {first_task}"
        );
    }
    for gone in ["tour-path", "fix the tour"] {
        assert!(!first_task.contains(gone), "{gone} is in {first_task}");
    }
}

#[test]
fn a_hand_off_from_a_file_screen_survives_a_restart_still_naming_its_file() {
    let pull = Pull::new();
    let review = handed_off(&pull);

    let reopened = pull.open();

    assert_eq!(reopened.hand_offs().all().len(), 1);
    assert_eq!(reopened.hand_offs(), review.hand_offs());
}
