use crate::support::FixtureRepo;
use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::work::{drive_until_idle, kinds};
use crate::support::{file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{
    AGENT_START_RETRY, AfterLanding, AgentReplied, AgentReplyRefused, AgentReplyRequest,
    AgentStatus, Audience, CommentGesture, CommentId, CommentPlace, CommentResult, HandOffId,
    HandOffState, LineLocation, Problem, Reaction, ReactionChange, ReplyPlacement, Review,
    ReviewStateFile, ReviewThread, SeenMark, SendMode, Side, TerminalId, TextBox, ThreadAction,
    ThreadActionResult, ThreadComment, ThreadId, ThreadMessage, TourAgentPort, TourAgentState,
    TourAgents, Work, WorkKind, WorkPorts,
};
use serde_json::json;
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

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    /// Runs the review's due work with these ports until none is due at `now`.
    fn drive_at(
        &self,
        review: &mut Review,
        agents: &TourAgents,
        github: &FakeGitHub,
        now: Instant,
    ) -> Vec<(WorkKind, AfterLanding)> {
        let source = self.source();
        let ports = WorkPorts {
            github,
            source: &source,
            agents,
        };
        drive_until_idle(review, &ports, now)
    }

    fn drive(
        &self,
        review: &mut Review,
        agents: &TourAgents,
        github: &FakeGitHub,
    ) -> Vec<(WorkKind, AfterLanding)> {
        self.drive_at(review, agents, github, Instant::now())
    }

    /// With a GitHub other than the PR's, so its calls are not logged with the test's.
    fn deliver(&self, review: &mut Review, agents: &TourAgents) -> Vec<(WorkKind, AfterLanding)> {
        let elsewhere = FakeGitHub::with_pull("main-tip", "head", &SentLog::default());
        self.drive(review, agents, &elsewhere)
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
        let source = self.source();
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
        by_viewer: false,
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
    let place = review
        .open_comment_box(CommentGesture::DragEnded)
        .unwrap()
        .unwrap();
    review
        .type_in(&TextBox::Draft(place.clone()), body)
        .unwrap();
    place
}

fn send(
    review: &mut Review,
    place: &CommentPlace,
    mode: SendMode,
    audience: Audience,
    github: &FakeGitHub,
) -> CommentResult {
    let request = review.start_sending(place, mode, audience).unwrap();
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
        let [hand_off] = review.hand_offs().all() else {
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
    assert_eq!(review.hand_offs().all(), []);
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
    assert_eq!(review.hand_offs().all(), []);
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
        review.hand_offs().all(),
        [],
        "GitHub does not have the comment yet"
    );

    review.pull_from_github();
    pull.drive(
        &mut review,
        &agents_of(&Arc::new(FakeTourAgent::new())),
        &github_with_the_comment_as_a_thread(&pull, "why ten?"),
    );

    let [hand_off] = review.hand_offs().all() else {
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

    let [hand_off] = review.hand_offs().all() else {
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
    assert_eq!(reopened.hand_offs().all().len(), 1);
}

fn reply(
    review: &mut Review,
    text: &str,
    audience: Audience,
    github: &FakeGitHub,
) -> ThreadActionResult {
    let thread = ThreadId::new("T1");
    review.open_box(&TextBox::Reply(thread.clone())).unwrap();
    review
        .type_in(&TextBox::Reply(thread.clone()), text)
        .unwrap();
    review.act_on_thread(
        ThreadAction::Reply {
            thread_id: thread,
            mode: SendMode::AddToReview,
            audience,
        },
        github,
    )
}

fn conversation(last: &str) -> Vec<ThreadComment> {
    vec![
        comment("C1", "me", "why ten?"),
        comment("C2", "alice", "because"),
        comment("C3", "me", last),
    ]
}

#[test]
fn a_reply_sent_with_ask_agent_is_handed_off_with_the_thread_so_far() {
    let pull = Pull::new();
    let mut github = pull.github();
    github.threads = vec![thread_on_line_ten("T1", conversation("and why not nine?"))];
    let mut review = pull.open();

    let result = reply(&mut review, "and why not nine?", Audience::Agent, &github);

    assert_eq!(result, ThreadActionResult::Done);
    let [hand_off] = review.hand_offs().all() else {
        panic!("an asked reply is handed off");
    };
    assert_eq!(hand_off.comment(), "and why not nine?");
    assert_eq!(hand_off.thread(), Some(&ThreadId::new("T1")));
    assert_eq!(
        hand_off.thread_so_far(),
        [
            ThreadMessage {
                author: "me".into(),
                body: "why ten?".into()
            },
            ThreadMessage {
                author: "alice".into(),
                body: "because".into()
            }
        ]
    );
}

#[test]
fn a_plain_reply_is_not_handed_off_even_in_a_thread_that_started_with_ask_agent() {
    let pull = Pull::new();
    let mut github = pull.github();
    github.threads = vec![thread_on_line_ten(
        "T1",
        vec![comment("C1", "me", "why ten?")],
    )];
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    send(
        &mut review,
        &place,
        SendMode::AddToReview,
        Audience::Agent,
        &github,
    );
    assert_eq!(review.hand_offs().all().len(), 1);
    github.threads = vec![thread_on_line_ten("T1", conversation("thanks"))];

    reply(&mut review, "thanks", Audience::Author, &github);

    assert_eq!(
        review.hand_offs().all().len(),
        1,
        "only the first comment asked"
    );
}

fn asked_twice(pull: &Pull, github: &mut FakeGitHub) -> Review {
    github.threads = vec![
        thread_on_line_ten("T1", vec![comment("C1", "me", "why ten?")]),
        thread_on_line_ten("T2", vec![comment("C2", "me", "and eleven?")]),
    ];
    let mut review = pull.open();
    for body in ["why ten?", "and eleven?"] {
        let place = comment_on_line_ten(&mut review, body);
        send(
            &mut review,
            &place,
            SendMode::AddToReview,
            Audience::Agent,
            github,
        );
    }
    review
}

fn agents_of(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
}

/// What the follow of the tour agent reports when its terminal shows `status`.
fn agent_is(review: &mut Review, status: AgentStatus) {
    review.tour_agent_is(&TourAgentState::Agent {
        terminal: TerminalId::new("t1"),
        status,
    });
}

fn replied(review: &mut Review, id: HandOffId, replied: &AgentReplied) {
    review.hand_off_replied(id, replied).unwrap();
}

fn states(review: &Review) -> Vec<HandOffState> {
    review
        .hand_offs()
        .all()
        .iter()
        .map(|hand_off| hand_off.state().clone())
        .collect()
}

#[test]
fn an_idle_agent_gets_the_oldest_hand_off_as_a_self_contained_message() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));

    pull.deliver(&mut review, &agents_of(&fake));

    let calls = fake.calls();
    let [TourAgentCall::Send { terminal, text }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!(terminal, &TerminalId::new("t1"));
    for part in [
        PR,
        "h0",
        "why ten?",
        "notes.txt",
        "guided-review agent-reply",
        "--working",
        "--body -",
        "blank line",
    ] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
    assert!(!text.contains("and eleven?"), "only the oldest goes");
    assert_eq!(
        states(&review),
        [HandOffState::Delivered, HandOffState::Queued]
    );
}

#[test]
fn a_busy_agent_keeps_the_hand_offs_queued_and_the_second_waits_for_the_first_to_be_done() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let busy = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Working));

    pull.deliver(&mut review, &agents_of(&busy));

    assert_eq!(busy.calls(), []);
    assert_eq!(
        states(&review),
        [HandOffState::Queued, HandOffState::Queued]
    );
    let idle = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    for _ in 0..2 {
        agent_is(&mut review, AgentStatus::Idle);
        pull.deliver(&mut review, &agents_of(&idle));
    }
    let sent: Vec<bool> = idle
        .calls()
        .into_iter()
        .map(|call| match call {
            TourAgentCall::Send { text, .. } => text.contains("why ten?"),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        sent,
        [true],
        "the agent works on the first, so the second is refused"
    );
}

#[test]
fn an_agent_waiting_for_the_reviewer_is_never_sent_to() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let fake =
        Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::WaitingForYou));

    pull.deliver(&mut review, &agents_of(&fake));

    assert_eq!(fake.calls(), []);
    assert_eq!(states(&review)[0], HandOffState::Queued);
}

#[test]
fn with_no_agent_for_the_pr_one_is_started_and_the_hand_off_goes_once_it_is_idle() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let fake = Arc::new(FakeTourAgent::new());
    let agents = agents_of(&fake).starting_in("/clones/shop".into());

    let ran = pull.deliver(&mut review, &agents);

    assert_eq!(
        ran,
        [(WorkKind::DeliverHandOff, AfterLanding::FollowNewTourAgent)]
    );
    let calls = fake.calls();
    let [
        TourAgentCall::Start {
            name,
            pr,
            folder,
            first_task,
        },
    ] = calls.as_slice()
    else {
        panic!("{calls:?}");
    };
    assert_eq!(name, "tour-acme-shop-7");
    assert_eq!(pr.to_string(), PR);
    assert_eq!(folder, &std::path::PathBuf::from("/clones/shop"));
    for part in [PR, "tour agent", "guided-review tour-path"] {
        assert!(
            first_task.contains(part),
            "{part} missing from {first_task}"
        );
    }
    assert_eq!(states(&review)[0], HandOffState::Queued);
    agent_is(&mut review, AgentStatus::Working);
    pull.deliver(&mut review, &agents);
    assert_eq!(fake.calls().len(), 1, "the new agent is working, not asked");
    fake.set_status("t100", AgentStatus::Idle);
    agent_is(&mut review, AgentStatus::Idle);
    pull.deliver(&mut review, &agents);
    assert_eq!(states(&review)[0], HandOffState::Delivered);
}

#[test]
fn an_agent_still_starting_keeps_the_hand_off_queued_and_is_tried_again_after_the_retry_delay() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    fake.still_starting("t1");
    let agents = agents_of(&fake);
    let start = Instant::now();

    pull.drive_at(&mut review, &agents, &github, start);

    assert_eq!(states(&review)[0], HandOffState::Queued);
    assert_eq!(review.next_due(), Some(start + AGENT_START_RETRY));
    fake.settled("t1");
    let too_soon = start + AGENT_START_RETRY / 2;
    assert_eq!(pull.drive_at(&mut review, &agents, &github, too_soon), []);
    let ran = pull.drive_at(&mut review, &agents, &github, start + AGENT_START_RETRY);
    assert_eq!(kinds(&ran), [WorkKind::DeliverHandOff]);
    assert_eq!(states(&review)[0], HandOffState::Delivered);
    assert_eq!(review.next_due(), None);
}

#[test]
fn a_comment_sent_with_ask_agent_is_delivered_by_the_work_it_makes_due() {
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
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));

    let ran = pull.deliver(&mut review, &agents_of(&fake));

    assert_eq!(kinds(&ran), [WorkKind::DeliverHandOff]);
    assert_eq!(states(&review), [HandOffState::Delivered]);
}

#[test]
fn a_delivery_due_while_one_runs_is_dropped_and_the_next_one_waits_for_a_new_reason() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let busy = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Working));
    let agents = agents_of(&busy);
    let now = Instant::now();
    let [first] = <[Work; 1]>::try_from(review.due_work(now)).ok().unwrap();
    assert!(review.is_delivering_hand_offs());

    agent_is(&mut review, AgentStatus::Working);

    assert!(
        review.due_work(now).is_empty(),
        "the first is still running"
    );
    let source = pull.source();
    let ports = WorkPorts {
        github: &github,
        source: &source,
        agents: &agents,
    };
    review.land(first.run(&ports));
    assert!(!review.is_delivering_hand_offs());
    assert!(
        review.due_work(now).is_empty(),
        "the one asked for meanwhile is gone"
    );
    agent_is(&mut review, AgentStatus::Idle);
    assert_eq!(
        review
            .due_work(now)
            .iter()
            .map(Work::kind)
            .collect::<Vec<_>>(),
        [WorkKind::DeliverHandOff]
    );
}

#[test]
fn a_delivered_hand_off_is_working_while_the_agent_works_and_ends_unanswered_when_it_goes_idle() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    pull.deliver(&mut review, &agents_of(&fake));

    agent_is(&mut review, AgentStatus::Idle);
    assert_eq!(
        states(&review)[0],
        HandOffState::Delivered,
        "not seen working yet"
    );
    agent_is(&mut review, AgentStatus::Working);
    assert_eq!(states(&review)[0], HandOffState::Working);
    agent_is(&mut review, AgentStatus::Idle);

    assert_eq!(states(&review)[0], HandOffState::EndedWithoutAnswer);
    assert_eq!(states(&review)[1], HandOffState::Queued);
}

fn lines_under_the_threads(review: &Review) -> Vec<Option<String>> {
    review
        .current_step()
        .unwrap()
        .threads
        .iter()
        .map(|view| view.hand_off_line.as_deref().map(str::to_owned))
        .collect()
}

#[test]
fn a_thread_sent_with_ask_agent_shows_each_hand_off_state_as_a_line_under_it() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    let waiting = Some("Waiting for the agent to be free".to_owned());

    assert_eq!(
        lines_under_the_threads(&review),
        [waiting.clone(), waiting.clone()]
    );
    pull.deliver(&mut review, &agents_of(&fake));
    assert_eq!(
        lines_under_the_threads(&review)[0].as_deref(),
        Some("Delivered to the agent")
    );
    agent_is(&mut review, AgentStatus::Working);
    assert_eq!(
        lines_under_the_threads(&review)[0].as_deref(),
        Some("The agent is working on it")
    );
    agent_is(&mut review, AgentStatus::Idle);
    assert_eq!(
        lines_under_the_threads(&review),
        [
            Some("The agent finished without answering".to_owned()),
            waiting
        ]
    );
}

fn first_thread_shows_agent_working(review: &Review) -> bool {
    review
        .current_step()
        .unwrap()
        .threads
        .first()
        .unwrap()
        .agent_is_working
}

#[test]
fn a_threads_line_shows_the_agent_at_work_only_while_it_works() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    assert!(!first_thread_shows_agent_working(&review), "waiting");

    pull.deliver(&mut review, &agents_of(&fake));
    assert!(!first_thread_shows_agent_working(&review), "delivered");
    agent_is(&mut review, AgentStatus::Working);
    assert!(first_thread_shows_agent_working(&review), "working");
    agent_is(&mut review, AgentStatus::Idle);
    assert!(!first_thread_shows_agent_working(&review), "ended");
}

#[test]
fn a_hand_off_that_failed_in_its_thread_can_be_tried_again_and_then_goes_to_the_agent() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let no_room = Arc::new(FakeTourAgent::new());
    pull.deliver(&mut review, &agents_of(&no_room));
    assert!(matches!(states(&review)[0], HandOffState::Failed(_)));
    let offered: Vec<Option<HandOffId>> = review
        .current_step()
        .unwrap()
        .threads
        .iter()
        .map(|view| view.failed_hand_off)
        .collect();
    let first = review.hand_offs().all()[0].id();
    assert_eq!(offered, [Some(first), None]);

    review.try_hand_off_again(first).unwrap();

    assert_eq!(states(&review)[0], HandOffState::Queued);
    let idle = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    pull.deliver(&mut review, &agents_of(&idle));
    assert_eq!(states(&review)[0], HandOffState::Delivered);
    assert_eq!(
        pull.open().hand_offs().all()[0].state(),
        &HandOffState::Delivered,
        "kept in the state file"
    );
}

#[test]
fn every_hand_off_state_has_its_own_line() {
    let lines: Vec<String> = [
        HandOffState::Queued,
        HandOffState::Delivered,
        HandOffState::Working,
        HandOffState::Answered,
        HandOffState::EndedWithoutAnswer,
        HandOffState::Failed("no agent".into()),
    ]
    .iter()
    .map(HandOffState::line)
    .collect();

    assert_eq!(
        lines,
        [
            "Waiting for the agent to be free",
            "Delivered to the agent",
            "The agent is working on it",
            "The agent answered",
            "The agent finished without answering",
            "Hand-off failed: no agent",
        ]
    );
}

#[test]
fn a_thread_never_handed_off_shows_no_line() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.set_threads(&[thread_on_line_ten(
        "T1",
        vec![comment("C1", "me", "why ten?")],
    )]);

    assert_eq!(lines_under_the_threads(&review), [None]);
}

#[test]
fn hand_off_lines_survive_a_restart() {
    let pull = Pull::new();
    let mut github = pull.github();
    let review = asked_twice(&pull, &mut github);
    drop(review);

    let mut reopened = pull.open();
    reopened.set_threads(&[thread_on_line_ten(
        "T1",
        vec![comment("C1", "me", "why ten?")],
    )]);

    assert_eq!(
        lines_under_the_threads(&reopened),
        [Some("Waiting for the agent to be free".to_owned())]
    );
}

#[test]
fn a_hand_off_that_found_no_thread_is_a_sidebar_problem_until_the_next_one_goes_through() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    send(
        &mut review,
        &place,
        SendMode::AddToReview,
        Audience::Agent,
        &github,
    );

    assert_eq!(
        review.problems(),
        [Problem::HandOffNotSent {
            comment: "why ten?".into(),
            reason: "GitHub shows no thread with the comment".into()
        }]
    );

    github.threads = vec![thread_on_line_ten(
        "T1",
        vec![comment("C1", "me", "and eleven?")],
    )];
    let place = comment_on_line_ten(&mut review, "and eleven?");
    send(
        &mut review,
        &place,
        SendMode::AddToReview,
        Audience::Agent,
        &github,
    );
    assert_eq!(review.problems(), []);
}

fn asked_one(pull: &Pull, github: &FakeGitHub, mode: SendMode) -> HandOffId {
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    send(&mut review, &place, mode, Audience::Agent, github);
    review.hand_offs().all().first().unwrap().id()
}

fn reply_request(
    pull: &Pull,
    id: HandOffId,
    body: &str,
) -> Result<AgentReplyRequest, AgentReplyRefused> {
    let pr: PullRequestUrl = PR.parse().unwrap();
    let state = ReviewStateFile::for_pr(pull.repo.home(), &pr)
        .load()
        .unwrap();
    state.hand_offs().agent_reply(&pr, id, body)
}

fn replies(sent: &SentLog) -> Vec<(String, SendMode)> {
    sent.github_calls()
        .into_iter()
        .filter_map(|call| match call {
            GitHubCall::Reply { body, mode, .. } => Some((body, mode)),
            _ => None,
        })
        .collect()
}

#[test]
fn an_agent_reply_joins_the_pending_review_and_does_not_send_it() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let id = asked_one(&pull, &github, SendMode::AddToReview);
    assert!(github.has_pending_review_now());

    let posted = reply_request(&pull, id, "Because of the loop.")
        .unwrap()
        .run(&github)
        .unwrap();

    assert_eq!(posted.placement(), ReplyPlacement::JoinedPendingReview);
    assert_eq!(
        replies(&pull.sent),
        [("🤖 Because of the loop.".to_owned(), SendMode::AddToReview)]
    );
    assert!(
        github.has_pending_review_now(),
        "the review is still the reviewer's to send"
    );
    assert!(
        !pull
            .sent
            .github_calls()
            .iter()
            .any(|call| matches!(call, GitHubCall::Submit { .. }))
    );
}

#[test]
fn an_agent_reply_is_posted_at_once_when_the_reviewer_has_no_pending_review() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let id = asked_one(&pull, &github, SendMode::CommentNow);
    assert!(!github.has_pending_review_now());

    let posted = reply_request(&pull, id, "Because of the loop.")
        .unwrap()
        .run(&github)
        .unwrap();

    assert_eq!(posted.placement(), ReplyPlacement::PostedAtOnce);
    assert_eq!(
        replies(&pull.sent),
        [("🤖 Because of the loop.".to_owned(), SendMode::CommentNow)]
    );
}

#[test]
fn an_agent_reply_starts_with_the_robot_exactly_once() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let id = asked_one(&pull, &github, SendMode::AddToReview);

    for body in ["🤖 already marked", "🤖🤖  twice", "  plain  "] {
        reply_request(&pull, id, body)
            .unwrap()
            .run(&github)
            .unwrap();
    }

    let bodies: Vec<String> = replies(&pull.sent)
        .into_iter()
        .map(|(body, _)| body)
        .collect();
    assert_eq!(bodies, ["🤖 already marked", "🤖 twice", "🤖 plain"]);
}

#[test]
fn an_agent_reply_to_a_thread_that_is_gone_fails_with_githubs_reason() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let id = asked_one(&pull, &github, SendMode::AddToReview);
    github.inject(GitHubFault::ThreadIsGone);

    let error = reply_request(&pull, id, "Because.")
        .unwrap()
        .run(&github)
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("Could not resolve to a PullRequestReviewThread"),
        "{error:#}"
    );
}

#[test]
fn an_agent_reply_to_an_unknown_hand_off_is_refused_with_the_known_ids() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    asked_one(&pull, &github, SendMode::AddToReview);

    let refused = reply_request(&pull, "h9".parse().unwrap(), "Because.")
        .err()
        .unwrap();

    assert_eq!(
        refused.to_string(),
        "no hand-off h9 on this PR; the known ones are h0"
    );
}

#[test]
fn an_answered_hand_off_is_answered_in_the_review_and_in_the_state_file() {
    let pull = Pull::new();
    let mut github = pull.github();
    let mut review = asked_twice(&pull, &mut github);
    let first = review.hand_offs().all().first().unwrap().id();

    replied(&mut review, first, &AgentReplied::default());

    assert_eq!(
        states(&review),
        [HandOffState::Answered, HandOffState::Queued]
    );
    let pr: PullRequestUrl = PR.parse().unwrap();
    let saved = ReviewStateFile::for_pr(pull.repo.home(), &pr)
        .load()
        .unwrap();
    assert_eq!(
        saved.hand_offs().all().first().unwrap().state(),
        &HandOffState::Answered
    );
}

#[test]
fn a_hand_off_answered_in_the_state_file_alone_is_answered_and_an_unknown_one_is_refused() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let id = asked_one(&pull, &github, SendMode::AddToReview);
    let pr: PullRequestUrl = PR.parse().unwrap();
    let file = ReviewStateFile::for_pr(pull.repo.home(), &pr);

    let mut result = Ok(());
    file.update(|state| result = state.hand_off_replied(id, &AgentReplied::default()))
        .unwrap();

    result.unwrap();
    assert_eq!(
        file.load()
            .unwrap()
            .hand_offs()
            .all()
            .first()
            .unwrap()
            .state(),
        &HandOffState::Answered
    );
    let refused = file
        .load()
        .unwrap()
        .hand_off_replied("h9".parse().unwrap(), &AgentReplied::default())
        .unwrap_err();
    assert!(matches!(refused, AgentReplyRefused::Unknown { .. }));
}

fn reactions(sent: &SentLog) -> Vec<(CommentId, ReactionChange)> {
    sent.github_calls()
        .into_iter()
        .filter_map(|call| match call {
            GitHubCall::SetReaction {
                comment_id,
                reaction: Reaction::Eyes,
                change,
            } => Some((comment_id, change)),
            _ => None,
        })
        .collect()
}

fn seen_marks(review: &Review) -> Vec<SeenMark> {
    review
        .hand_offs()
        .all()
        .iter()
        .map(|hand_off| hand_off.seen_mark().clone())
        .collect()
}

/// One hand-off of "why ten?" in thread T1, comment C1, delivered to an idle agent.
fn delivered_one(pull: &Pull, github: &FakeGitHub) -> (Review, HandOffId) {
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    send(
        &mut review,
        &place,
        SendMode::CommentNow,
        Audience::Agent,
        github,
    );
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    pull.drive(&mut review, &agents_of(&fake), github);
    let id = review.hand_offs().all().first().unwrap().id();
    (review, id)
}

fn working_on_it_request(pull: &Pull, id: HandOffId) -> AgentReplyRequest {
    let pr: PullRequestUrl = PR.parse().unwrap();
    ReviewStateFile::for_pr(pull.repo.home(), &pr)
        .load()
        .unwrap()
        .hand_offs()
        .agent_working_on_it(&pr, id)
        .unwrap()
}

#[test]
fn a_delivered_hand_off_puts_eyes_on_the_reviewers_comment() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");

    let (review, _) = delivered_one(&pull, &github);

    assert_eq!(
        reactions(&pull.sent),
        [(CommentId::new("C1"), ReactionChange::Add)]
    );
    assert_eq!(seen_marks(&review), [SeenMark::On]);
}

#[test]
fn eyes_github_refuses_are_named_under_the_thread_and_the_hand_off_still_goes() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    send(
        &mut review,
        &place,
        SendMode::CommentNow,
        Audience::Agent,
        &github,
    );
    github.inject(GitHubFault::ConnectionLost);

    let fake = Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle));
    pull.drive(&mut review, &agents_of(&fake), &github);

    assert_eq!(states(&review), [HandOffState::Delivered]);
    let line = lines_under_the_threads(&review)[0].clone().unwrap();
    assert!(
        line.starts_with("Delivered to the agent")
            && line.contains("👀")
            && line.contains("network is down"),
        "{line}"
    );
}

#[test]
fn the_agents_answer_takes_the_eyes_off() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let (mut review, id) = delivered_one(&pull, &github);

    let done = reply_request(&pull, id, "Because of the loop.")
        .unwrap()
        .run(&github)
        .unwrap();
    replied(&mut review, id, done.replied());

    assert_eq!(
        reactions(&pull.sent),
        [
            (CommentId::new("C1"), ReactionChange::Add),
            (CommentId::new("C1"), ReactionChange::Remove)
        ]
    );
    assert_eq!(states(&review), [HandOffState::Answered]);
    assert_eq!(seen_marks(&review), [SeenMark::Off]);
}

#[test]
fn working_on_it_is_posted_at_once_and_the_answer_replaces_it() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let (mut review, id) = delivered_one(&pull, &github);

    let working = working_on_it_request(&pull, id).run(&github).unwrap();
    replied(&mut review, id, working.replied());

    assert_eq!(
        replies(&pull.sent),
        [("🤖 Working on it…".to_owned(), SendMode::CommentNow)]
    );
    let progress = CommentId::new("R1");
    assert_eq!(
        states(&review),
        [HandOffState::WorkingOnIt {
            reply: progress.clone()
        }]
    );
    assert_eq!(seen_marks(&review), [SeenMark::Off]);
    assert_eq!(
        lines_under_the_threads(&review)[0].as_deref(),
        Some("The agent said it is working on it")
    );

    let answered = reply_request(&pull, id, "Done in abc123.")
        .unwrap()
        .run(&github)
        .unwrap();
    replied(&mut review, id, answered.replied());

    assert_eq!(answered.placement(), ReplyPlacement::ReplacedWorkingOnIt);
    assert_eq!(replies(&pull.sent).len(), 1, "no second reply");
    assert!(pull.sent.github_calls().contains(&GitHubCall::EditComment {
        comment_id: progress,
        body: "🤖 Done in abc123.".to_owned(),
    }));
    assert_eq!(states(&review), [HandOffState::Answered]);
}

#[test]
fn an_agent_that_stops_after_working_on_it_ended_without_an_answer() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let (mut review, id) = delivered_one(&pull, &github);
    let working = working_on_it_request(&pull, id).run(&github).unwrap();
    replied(&mut review, id, working.replied());

    agent_is(&mut review, AgentStatus::Working);
    agent_is(&mut review, AgentStatus::Idle);

    assert_eq!(states(&review), [HandOffState::EndedWithoutAnswer]);
}

#[test]
fn a_hand_off_the_agent_ends_without_answering_has_its_eyes_taken_off() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let (mut review, _) = delivered_one(&pull, &github);
    let agents = agents_of(&Arc::new(FakeTourAgent::new()));
    agent_is(&mut review, AgentStatus::Working);
    assert_eq!(
        pull.drive(&mut review, &agents, &github),
        [],
        "still working on it"
    );

    agent_is(&mut review, AgentStatus::Idle);
    let ran = pull.drive(&mut review, &agents, &github);

    assert_eq!(kinds(&ran), [WorkKind::TakeSeenMarksOff]);
    assert_eq!(
        reactions(&pull.sent),
        [
            (CommentId::new("C1"), ReactionChange::Add),
            (CommentId::new("C1"), ReactionChange::Remove)
        ]
    );
    assert_eq!(seen_marks(&review), [SeenMark::Off]);
    agent_is(&mut review, AgentStatus::Idle);
    assert_eq!(
        pull.drive(&mut review, &agents, &github),
        [],
        "taken off once"
    );
}

#[test]
fn a_reply_of_the_agent_reads_the_threads_and_the_pending_review_again() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let (mut review, id) = delivered_one(&pull, &github);
    let agents = agents_of(&Arc::new(FakeTourAgent::new()));
    review.viewer_authorship_is(false);
    pull.drive(&mut review, &agents, &github);
    let done = reply_request(&pull, id, "Because of the loop.")
        .unwrap()
        .run(&github)
        .unwrap();
    let calls_before = pull.sent.github_calls().len();

    replied(&mut review, id, done.replied());
    let ran = pull.drive(&mut review, &agents, &github);

    assert_eq!(
        kinds(&ran),
        [WorkKind::PullThreads, WorkKind::PullPendingReview]
    );
    assert_eq!(
        pull.sent.github_calls()[calls_before..],
        [
            GitHubCall::ThreadsOf(PR.parse().unwrap()),
            GitHubCall::PendingReviewOf(PR.parse().unwrap()),
            GitHubCall::AuthorOf(PR.parse().unwrap()),
        ]
    );
}

#[test]
fn the_agents_working_on_it_reply_is_left_out_of_the_thread_while_the_line_under_it_says_so() {
    let pull = Pull::new();
    let github = github_with_the_comment_as_a_thread(&pull, "why ten?");
    let (mut review, id) = delivered_one(&pull, &github);
    let working = working_on_it_request(&pull, id).run(&github).unwrap();
    replied(&mut review, id, working.replied());
    let thread_with = |reply: &str| {
        thread_on_line_ten(
            "T1",
            vec![comment("C1", "me", "why ten?"), comment("R1", "me", reply)],
        )
    };
    let shown_comments = |review: &Review| -> Vec<String> {
        review.current_step().unwrap().threads[0]
            .thread
            .comments
            .iter()
            .map(|comment| comment.body.clone())
            .collect()
    };

    review.set_threads(&[thread_with("🤖 Working on it…")]);

    assert_eq!(shown_comments(&review), ["why ten?"]);

    let answered = reply_request(&pull, id, "Done in abc123.")
        .unwrap()
        .run(&github)
        .unwrap();
    replied(&mut review, id, answered.replied());
    review.set_threads(&[thread_with("🤖 Done in abc123.")]);

    assert_eq!(shown_comments(&review), ["why ten?", "🤖 Done in abc123."]);
}
