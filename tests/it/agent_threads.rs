//! A private thread to the tour agent in a PR's review: kept in the review's state file, answered by the
//! tour agent into the same thread, and never sent to the host.

use crate::support::fake_ports::{FakeHost, HostCall, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::graphql::serve_graphql;
use crate::support::work::drive_until_idle;
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview};
use crate::support::{file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::PullRequestUrl;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    AgentReplyRequest, AgentStatus, Audience, CommentGesture, CommentId, CommentPlace,
    CommentResult, HandOffId, HandOffState, HandToAgentRefused, LineLocation, LocalThreadStore,
    ReplyPlacement, Review, ReviewStateFile, ReviewThread, SendMode, Side, TerminalId, TextBox,
    ThreadAction, ThreadActionResult, ThreadComment, ThreadHomes, ThreadId, TourAgentPort,
    TourAgents, WorkPorts,
};
use serde_json::json;
use std::path::Path;
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

    fn host(&self) -> FakeHost {
        FakeHost::with_pull("main-tip", &self.head, &self.sent)
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn subject(&self) -> ReviewSubject {
        ReviewSubject::from(PR.parse::<PullRequestUrl>().unwrap())
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(&self.repo.folders(), &self.subject())
    }

    fn drive(&self, review: &mut Review, agents: &TourAgents, host: &FakeHost) {
        let source = self.source();
        let ports = WorkPorts {
            host,
            source: &source,
            agents,
        };
        drive_until_idle(review, &ports, Instant::now());
    }

    fn open(&self) -> Review {
        let tour: GuidedTour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Notes",
            "summary": [],
            "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": [{"file": "notes.txt"}], "children": []}],
        }))
        .unwrap();
        let source = self.source();
        let mut review = Review::open(
            tour,
            &source,
            self.state_file(),
            crate::support::fake_ports::every_capability(),
        )
        .unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review
    }

    /// The threads the review's state file keeps privately, as the host never sees them.
    fn private_threads(&self) -> Vec<ReviewThread> {
        LocalThreadStore::in_file(self.state_file())
            .threads_at(&guided_review::pull_request::Sha::new(&self.head))
            .unwrap()
    }

    fn host_writes(&self) -> Vec<HostCall> {
        self.sent
            .host_calls()
            .into_iter()
            .filter(|call| {
                matches!(
                    call,
                    HostCall::AddThread { .. }
                        | HostCall::CommentNow { .. }
                        | HostCall::Reply { .. }
                        | HostCall::SetResolved { .. }
                        | HostCall::SetReaction { .. }
                        | HostCall::EditComment { .. }
                        | HostCall::DeleteComment { .. }
                )
            })
            .collect()
    }
}

fn h0() -> HandOffId {
    "h0".parse().unwrap()
}

fn agents_of(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
}

fn an_idle_agent() -> Arc<FakeTourAgent> {
    Arc::new(FakeTourAgent::new().with_terminal("t1", Some(PR), AgentStatus::Idle))
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

fn ask_agent(review: &mut Review, place: &CommentPlace, host: &FakeHost) -> CommentResult {
    let request = review
        .start_sending(place, SendMode::SendNow, Audience::Agent)
        .expect("a PR's box sends privately to the agent");
    review.apply_comment(request.run(host)).unwrap()
}

fn github_thread(id: &str, body: &str) -> ReviewThread {
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
        comments: vec![ThreadComment {
            avatar_url: None,
            id: CommentId::new(&format!("{id}-c1")),
            author: "reviewer-friend".into(),
            is_bot: false,
            by_agent: None,
            body: body.into(),
            url: String::new(),
            reactions: Vec::new(),
            by_viewer: false,
        }],
    }
}

fn threads_on_the_step(review: &Review) -> Vec<ReviewThread> {
    review
        .current_step()
        .unwrap()
        .threads
        .iter()
        .map(|view| view.thread.clone())
        .collect()
}

fn reply_request(pull: &Pull, id: HandOffId, body: &str) -> AgentReplyRequest {
    let file = pull.state_file();
    let state = file.load().unwrap();
    ThreadHomes::of(&pull.subject(), &file)
        .agent_reply(state.hand_offs(), id, body)
        .unwrap()
}

#[test]
fn a_prs_boxes_send_privately_to_the_agent_with_send_now_and_never_queue_tasks() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");

    assert!(review.send_modes().contains(&SendMode::SendNow));
    assert!(!review.send_modes().contains(&SendMode::AddToTasks));
    assert!(
        review
            .start_sending(&place, SendMode::AddToTasks, Audience::Agent)
            .is_none()
    );
}

#[test]
fn a_private_question_becomes_a_thread_in_the_state_file_and_nothing_goes_to_github() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");

    let result = ask_agent(&mut review, &place, &host);

    assert_eq!(result, CommentResult::Sent);
    assert_eq!(pull.host_writes(), []);
    let [thread] = threads_on_the_step(&review).try_into().unwrap();
    assert!(thread.is_private());
    let [asked] = thread.comments.as_slice() else {
        panic!("{:?}", thread.comments);
    };
    assert_eq!(
        (asked.body.as_str(), asked.author.as_str()),
        ("why ten?", "You")
    );
    assert_eq!(pull.private_threads().len(), 1);
}

#[test]
fn the_private_question_is_handed_off_to_the_agent_on_its_own_thread() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");

    ask_agent(&mut review, &place, &host);

    let [hand_off] = review.hand_offs().all() else {
        panic!("one hand-off expected");
    };
    assert_eq!(hand_off.mode(), SendMode::SendNow);
    assert_eq!(hand_off.state(), &HandOffState::Queued);
    assert_eq!(hand_off.thread(), Some(&threads_on_the_step(&review)[0].id));
}

#[test]
fn the_agent_is_told_the_question_is_private_where_it_is_and_how_to_answer_it() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);
    let fake = an_idle_agent();

    pull.drive(&mut review, &agents_of(&fake), &pull.host());

    let calls = fake.calls();
    let [TourAgentCall::Send { terminal, text }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!(terminal, &TerminalId::new("t1"));
    for part in [
        "notes.txt",
        "line ten",
        "why ten?",
        "Not on GitHub",
        &format!("guided-review agent-reply {PR} h0 --body -"),
    ] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
    for part in ["pending review", "posted", "author", "Sent:"] {
        assert!(!text.contains(part), "{part} found in {text}");
    }
    assert_eq!(
        pull.host_writes(),
        [],
        "no 👀 on a comment the host does not have"
    );
}

#[test]
fn the_agents_answer_goes_into_the_private_thread_and_not_onto_github() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);

    let done = reply_request(&pull, h0(), "Because of the loop.")
        .run(&host)
        .unwrap();
    review.hand_off_replied(h0(), done.replied()).unwrap();
    pull.drive(&mut review, &agents_of(&an_idle_agent()), &host);

    assert_eq!(done.placement(), ReplyPlacement::KeptOnThread);
    assert_eq!(pull.host_writes(), []);
    let [thread] = threads_on_the_step(&review).try_into().unwrap();
    let said: Vec<(&str, &str)> = thread
        .comments
        .iter()
        .map(|comment| (comment.author.as_str(), comment.body.as_str()))
        .collect();
    assert_eq!(
        said,
        [("You", "why ten?"), ("Your agent", "Because of the loop.")]
    );
    assert_eq!(review.hand_offs().all()[0].state(), &HandOffState::Answered);
}

#[test]
fn github_threads_and_private_threads_show_together_and_a_read_of_one_keeps_the_other() {
    let pull = Pull::new();
    let mut host = pull.host();
    host.threads = vec![github_thread("T1", "nit")];
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);
    review.pull_from_host();
    pull.drive(&mut review, &agents_of(&an_idle_agent()), &host);

    let ids = |review: &Review| -> Vec<(String, bool)> {
        let mut threads: Vec<(String, bool)> = threads_on_the_step(review)
            .iter()
            .map(|thread| (thread.id.to_string(), thread.is_private()))
            .collect();
        threads.sort();
        threads
    };
    assert_eq!(
        ids(&review),
        [
            ("T1".to_owned(), false),
            ("local-thread-1".to_owned(), true)
        ]
    );

    let place = comment_on_line_ten(&mut review, "and eleven?");
    ask_agent(&mut review, &place, &host);
    assert_eq!(ids(&review).len(), 3, "a new private thread keeps GitHub's");
    assert_eq!(ids(&review)[0], ("T1".to_owned(), false));
}

#[test]
fn a_private_thread_comes_back_after_a_restart_beside_githubs() {
    let pull = Pull::new();
    let mut host = pull.host();
    host.threads = vec![github_thread("T1", "nit")];
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);
    drop(review);

    let mut reopened = pull.open();
    reopened.pull_from_host();
    pull.drive(&mut reopened, &agents_of(&an_idle_agent()), &host);

    let mut ids: Vec<String> = threads_on_the_step(&reopened)
        .iter()
        .map(|thread| thread.id.to_string())
        .collect();
    ids.sort();
    assert_eq!(ids, ["T1", "local-thread-1"]);
}

fn reply_in(
    review: &mut Review,
    thread: &ThreadId,
    text: &str,
    host: &FakeHost,
) -> ThreadActionResult {
    let reply = TextBox::Reply(thread.clone());
    review.open_box(&reply).unwrap();
    review.type_in(&reply, text).unwrap();
    let request = review.thread_action_request(ThreadAction::Reply {
        thread_id: thread.clone(),
        mode: SendMode::SendNow,
        audience: Audience::Agent,
    });
    let outcome = request.run(host);
    review.apply_thread_action(outcome)
}

#[test]
fn a_reply_in_a_private_thread_stays_in_the_state_file_and_goes_to_the_agent() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);
    let thread = threads_on_the_step(&review)[0].id.clone();

    let result = reply_in(&mut review, &thread, "and why not nine?", &host);

    assert_eq!(result, ThreadActionResult::Done);
    assert_eq!(pull.host_writes(), []);
    assert_eq!(pull.private_threads()[0].comments.len(), 2);
    assert_eq!(
        review.hand_offs().all().len(),
        2,
        "the follow-up is handed off too"
    );
}

#[test]
fn resolving_a_private_thread_is_kept_in_the_state_file_and_github_is_not_asked() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);
    let thread = threads_on_the_step(&review)[0].id.clone();

    let request = review.thread_action_request(ThreadAction::Resolve { thread_id: thread });
    let result = review.apply_thread_action(request.run(&host));

    assert_eq!(result, ThreadActionResult::Done);
    assert_eq!(pull.host_writes(), []);
    assert!(pull.private_threads()[0].is_resolved);
}

#[test]
fn a_comment_sent_the_github_way_still_goes_to_github_beside_private_threads() {
    let pull = Pull::new();
    let host = pull.host().naming_new_threads(&[("T9", "C9")]);
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);
    let place = comment_on_line_ten(&mut review, "nit");

    let request = review
        .start_sending(&place, SendMode::AddToReview, Audience::Author)
        .unwrap();
    let result = review.apply_comment(request.run(&host)).unwrap();

    assert_eq!(result, CommentResult::Sent);
    assert_eq!(pull.host_writes().len(), 1);
    assert_eq!(threads_on_the_step(&review).len(), 2);
}

/// The `guided-review agent-reply` commands a message gives, each without the program name, in order.
fn agent_reply_commands(message: &str) -> Vec<Vec<String>> {
    message
        .split('`')
        .filter_map(|quoted| quoted.strip_prefix("guided-review agent-reply "))
        .map(|args| {
            std::iter::once("agent-reply")
                .chain(args.split_whitespace())
                .map(str::to_owned)
                .collect()
        })
        .collect()
}

#[test]
fn the_commands_the_private_question_gives_the_agent_write_its_answer_into_the_private_thread() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);
    let fake = an_idle_agent();
    pull.drive(&mut review, &agents_of(&fake), &host);
    drop(review);
    let calls = fake.calls();
    let [TourAgentCall::Send { text, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    let commands = agent_reply_commands(text);
    let working = commands
        .iter()
        .find(|args| args.last().is_some_and(|last| last == "--working"))
        .expect("the message gives --working");
    let answer = commands
        .iter()
        .find(|args| args.ends_with(&["--body".to_owned(), "-".to_owned()]))
        .expect("the message gives --body -");
    let (url, asked) = serve_graphql(Vec::new());
    let github = FakeGitHub { url };
    let run = |args: &[String], stdin: Option<&str>| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let runner = GuidedReview::new(&pull.repo, &github)
            .with_env("GUIDED_REVIEW_BUILD", Path::new("test-build"));
        let runner = match stdin {
            Some(text) => runner.with_stdin(text),
            None => runner,
        };
        runner.run(&args)
    };

    let said_working = run(working, None);
    let answered = run(answer, Some("Because of the loop.\n"));

    assert!(said_working.success, "stderr: {}", said_working.stderr);
    assert!(answered.success, "stderr: {}", answered.stderr);
    assert_eq!(
        *asked.lock().unwrap(),
        Vec::<String>::new(),
        "nothing asked of GitHub"
    );
    let [thread] = pull.private_threads().try_into().unwrap();
    let said: Vec<&str> = thread
        .comments
        .iter()
        .map(|comment| comment.body.as_str())
        .collect();
    assert_eq!(said, ["why ten?", "Because of the loop."]);
    let reopened = pull.open();
    assert_eq!(
        reopened.hand_offs().all()[0].state(),
        &HandOffState::Answered
    );
}

#[test]
fn the_agents_answer_in_a_private_thread_is_not_handed_to_the_agent_as_someone_elses_comment() {
    let pull = Pull::new();
    let host = pull.host();
    let mut review = pull.open();
    let place = comment_on_line_ten(&mut review, "why ten?");
    ask_agent(&mut review, &place, &host);
    let done = reply_request(&pull, h0(), "Because of the loop.")
        .run(&host)
        .unwrap();
    review.hand_off_replied(h0(), done.replied()).unwrap();
    pull.drive(&mut review, &agents_of(&an_idle_agent()), &host);
    let [view] = review.current_step().unwrap().threads.try_into().unwrap();
    let answer = view.thread.comments[1].id.clone();

    let refused = review.hand_to_agent(&answer);

    assert!(
        view.can_hand_to_agent.is_empty(),
        "{:?}",
        view.can_hand_to_agent
    );
    assert_eq!(refused, Err(HandToAgentRefused::PrivateThread));
    assert_eq!(review.hand_offs().all().len(), 1);
}
