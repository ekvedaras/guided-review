use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::support::fake_app::{CliEnded, answering_one_request};
use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::work::drive_until_idle;
use crate::support::{FakeGitHub as ServedGitHub, FixtureRepo, GuidedReview, Run, file_path};
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{
    AgentStatus, Audience, CommentGesture, CommentPlace, CommentResult, HandOffState, LineLocation,
    PieceDiff, ReviewStateFile, SendMode, TextBox, ThreadAction, ThreadHome, ThreadView,
    TourAgentPort, TourAgents, WorkPorts,
};

/// `feature` one commit past `main` that adds the cart, with the README's second line written and not
/// committed: a local review of it, with the change set's agent idle in its terminal.
struct Feature {
    repo: FixtureRepo,
    subject: ReviewSubject,
    agent: Arc<FakeTourAgent>,
    sent: SentLog,
}

impl Feature {
    fn with_work_in_progress() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        repo.commit("Start the shop");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("src/cart.rs", "pub fn total() {}\n");
        repo.commit("Add the cart");
        repo.write("README.md", "hello\nworld\n");
        let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
        let subject = ReviewSubject::from(work_tree.change_set().unwrap());
        let agent = Arc::new(FakeTourAgent::new().with_terminal(
            "t1",
            Some(&subject.to_string()),
            AgentStatus::Idle,
        ));
        Self {
            repo,
            subject,
            agent,
            sent: SentLog::default(),
        }
    }

    fn key(&self) -> String {
        self.subject.to_string()
    }

    fn folder(&self) -> String {
        self.subject
            .local()
            .unwrap()
            .folder()
            .as_path()
            .display()
            .to_string()
    }

    fn open(&self) -> OpenedReview {
        let mut opened =
            open_review(&self.subject, self.repo.path(), &self.repo.folders()).unwrap();
        opened.review.go_to_file(&file_path("README.md"));
        opened.review.pull_from_github();
        self.run_work(&mut opened);
        opened
    }

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", "snapshot", &self.sent)
    }

    /// Runs what the review wants done, as the app does after every change: reads, and hand-offs
    /// typed into the agent.
    fn run_work(&self, opened: &mut OpenedReview) {
        let agents = TourAgents::new(Arc::clone(&self.agent) as Arc<dyn TourAgentPort>);
        let github = self.github();
        let ports = WorkPorts {
            github: &github,
            source: opened.source.as_ref(),
            agents: &agents,
        };
        drive_until_idle(&mut opened.review, &ports, Instant::now());
    }

    fn typed_into_agent(&self) -> Vec<String> {
        self.agent
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                TourAgentCall::Send { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }
}

fn row(opened: &OpenedReview, text: &str) -> LineLocation {
    opened
        .review
        .file_page()
        .unwrap()
        .pieces
        .iter()
        .flat_map(PieceDiff::lines)
        .find(|row| row.text == text)
        .unwrap_or_else(|| panic!("no row shows {text:?}"))
        .location
        .clone()
}

fn write_comment(opened: &mut OpenedReview, on: &str, text: &str) -> CommentPlace {
    let at = row(opened, on);
    opened.review.begin_selection(at.clone());
    opened.review.extend_selection(at);
    let place = opened
        .review
        .open_comment_box(CommentGesture::DragEnded)
        .unwrap()
        .unwrap();
    opened
        .review
        .type_in(&TextBox::Draft(place.clone()), text)
        .unwrap();
    place
}

/// The comment box's Send now, as the app runs it.
fn send_now(feature: &Feature, opened: &mut OpenedReview, on: &str, text: &str) -> CommentResult {
    let place = write_comment(opened, on, text);
    let request = opened
        .review
        .start_sending(&place, SendMode::SendNow, Audience::Agent)
        .unwrap();
    let result = opened
        .review
        .apply_comment(request.run(&feature.github()))
        .unwrap();
    feature.run_work(opened);
    result
}

fn threads(opened: &OpenedReview) -> Vec<ThreadView> {
    opened.review.file_page().unwrap().threads
}

fn said(thread: &ThreadView) -> Vec<(String, String)> {
    thread
        .thread
        .comments
        .iter()
        .map(|comment| (comment.shown_author(), comment.body.clone()))
        .collect()
}

#[test]
fn send_now_keeps_a_local_thread_and_hands_the_task_to_the_change_sets_agent_with_nothing_on_github()
 {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();

    let result = send_now(&feature, &mut opened, "world", "Say who the world is");

    assert_eq!(result, CommentResult::Sent);
    let shown = threads(&opened);
    let [thread] = shown.as_slice() else {
        panic!("{shown:?}");
    };
    assert_eq!(
        said(thread),
        [("You".to_owned(), "Say who the world is".to_owned())]
    );
    assert_eq!(
        thread.anchor.as_ref().map(|at| at.line),
        Some(row(&opened, "world").line)
    );
    assert_eq!(
        thread.hand_off_line.as_deref(),
        Some("Delivered to the agent")
    );
    let typed = feature.typed_into_agent();
    let [task] = typed.as_slice() else {
        panic!("{typed:?}");
    };
    let key = feature.key();
    for part in [
        "local review",
        "no PR",
        &feature.folder(),
        "feature",
        "README.md line 2",
        "Say who the world is",
        &format!("guided-review agent-reply {key} h0 --body -"),
        &format!("guided-review agent-reply {key} h0 --working"),
        &format!("guided-review outline {key}"),
    ] {
        assert!(task.contains(part), "{part:?} missing from {task}");
    }
    for gone in ["GitHub", "push to", "PR's branch", "tour file"] {
        assert!(!task.contains(gone), "{gone:?} is in {task}");
    }
    assert_eq!(
        opened.review.hand_offs().all()[0].state(),
        &HandOffState::Delivered
    );
    assert_eq!(feature.sent.github_calls(), []);
}

const BUILD: &str = "test-build";

impl Feature {
    /// `guided-review agent-reply <local key> h0 …`, as the agent runs it in its terminal.
    fn agent_reply(&self, args: &[&str]) -> Run {
        let github = ServedGitHub::serving_pull("acme/shop", 7, "aaa", "bbb");
        let key = self.key();
        let run = GuidedReview::new(&self.repo, &github)
            .with_env("GUIDED_REVIEW_BUILD", Path::new(BUILD))
            .run(&[&["agent-reply", key.as_str(), "h0"], args].concat());
        assert!(run.success, "stderr: {}", run.stderr);
        run
    }

    /// The review as the app reads it after a restart.
    fn reopened(&self) -> OpenedReview {
        self.open()
    }

    fn hand_off_state_on_disk(&self) -> HandOffState {
        ReviewStateFile::for_subject(&self.repo.folders(), &self.subject)
            .load()
            .unwrap()
            .hand_offs()
            .all()[0]
            .state()
            .clone()
    }
}

fn task_sent(feature: &Feature) -> OpenedReview {
    let mut opened = feature.open();
    send_now(feature, &mut opened, "world", "Say who the world is");
    opened
}

#[test]
fn agent_reply_with_the_local_key_says_working_on_it_then_answers_in_its_place_on_the_local_thread()
{
    let feature = Feature::with_work_in_progress();
    drop(task_sent(&feature));

    let working = feature.agent_reply(&["--working"]);
    let working_shown = threads(&feature.reopened()).remove(0);
    let answered = feature.agent_reply(&["--body", "It is the cart's world now."]);

    assert!(
        working.stdout.contains("on its thread in the local review"),
        "{}",
        working.stdout
    );
    assert_eq!(
        working_shown.hand_off_line.as_deref(),
        Some("The agent said it is working on it"),
        "the line under the thread says it, in place of the reply"
    );
    assert!(working_shown.agent_is_working);
    assert!(
        answered.stdout.contains("in place of"),
        "{}",
        answered.stdout
    );
    let reopened = feature.reopened();
    let shown = threads(&reopened);
    assert_eq!(
        said(&shown[0]),
        [
            ("You".to_owned(), "Say who the world is".to_owned()),
            (
                "Your agent".to_owned(),
                "It is the cart's world now.".to_owned()
            ),
        ]
    );
    assert!(shown[0].thread.comments[1].is_by_agent());
    assert_eq!(feature.hand_off_state_on_disk(), HandOffState::Answered);
    assert_eq!(
        shown[0].hand_off_line.as_deref(),
        Some("The agent answered")
    );
}

#[test]
fn a_running_app_showing_the_local_review_is_told_and_marks_the_hand_off_while_the_answer_is_on_the_thread()
 {
    let feature = Feature::with_work_in_progress();
    drop(task_sent(&feature));
    let cli_ended = CliEnded::default();
    let key = feature.key();
    let (app, seen) = answering_one_request(
        feature.repo.home(),
        &cli_ended,
        BUILD,
        &format!(r#"{{"hand_off_answered":{{"pr":"{key}"}}}}"#),
    );

    feature.agent_reply(&["--body", "Done."]);

    cli_ended.mark();
    app.join().unwrap();
    let request: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(request["request"]["agent_replied"]["pr"], key.as_str());
    assert_eq!(
        feature.hand_off_state_on_disk(),
        HandOffState::Delivered,
        "the app holds the hand-offs and marks them itself"
    );
    assert_eq!(
        said(&threads(&feature.reopened())[0]).last().unwrap().1,
        "Done."
    );
}

/// The app's copy of the review and `agent-reply` both write the state file. Each writes only the
/// newest thread, so neither loses the other's words, whatever order they come in.
#[test]
fn the_app_and_agent_reply_never_write_a_stale_thread_over_each_others_words() {
    let feature = Feature::with_work_in_progress();
    let mut app = task_sent(&feature);

    // The app shows the review, so the command writes the thread and the app takes the hand-off.
    let home = ThreadHome::of(&feature.subject, &app.review.state_file());
    let state = app.review.state_file().load().unwrap();
    let first = app.review.hand_offs().all()[0].id();
    let request = state.hand_offs().agent_working_on_it(&home, first).unwrap();
    let done = request.run(&feature.github()).unwrap();
    app.review.hand_off_replied(first, done.replied()).unwrap();
    feature.run_work(&mut app);
    // The app saves its own copy of the review after the command wrote.
    app.review.tick_file(&file_path("README.md")).unwrap();
    let thread_id = threads(&app)[0].thread.id.clone();
    app.review
        .open_box(&TextBox::Reply(thread_id.clone()))
        .unwrap();
    app.review
        .type_in(&TextBox::Reply(thread_id.clone()), "And the cart?")
        .unwrap();
    let reply = app.review.thread_action_request(ThreadAction::Reply {
        thread_id,
        mode: SendMode::SendNow,
        audience: Audience::Agent,
    });
    app.review.apply_thread_action(reply.run(&feature.github()));
    // The command answers once more, the app not showing the review now.
    feature.agent_reply(&["--body", "The world is fine."]);

    let shown = threads(&feature.reopened());
    assert_eq!(
        said(&shown[0]),
        [
            ("You".to_owned(), "Say who the world is".to_owned()),
            ("Your agent".to_owned(), "The world is fine.".to_owned()),
            ("You".to_owned(), "And the cart?".to_owned()),
        ]
    );
    assert_eq!(threads(&app)[0].thread.comments.len(), 3);
    assert_eq!(feature.sent.github_calls(), []);
}
