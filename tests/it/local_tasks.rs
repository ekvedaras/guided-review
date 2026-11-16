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
    AgentLinkOff, AgentStatus, Audience, CommentGesture, CommentPlace, CommentProblem,
    CommentRefusal, CommentResult, HandOffState, LineLocation, PieceDiff, ReviewStateFile,
    SendMode, Side, TerminalId, TextBox, ThreadAction, ThreadActionResult, ThreadHome,
    ThreadMessage, ThreadView, TourAgentPort, TourAgentState, TourAgents, WorkPorts,
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

/// A thread action, as the app runs it.
fn act(feature: &Feature, opened: &mut OpenedReview, action: ThreadAction) -> ThreadActionResult {
    let request = opened.review.thread_action_request(action);
    let result = opened
        .review
        .apply_thread_action(request.run(&feature.github()));
    feature.run_work(opened);
    result
}

#[test]
fn a_reply_is_a_follow_up_task_and_replies_edits_deletes_and_resolve_last_across_a_restart() {
    let feature = Feature::with_work_in_progress();
    let mut opened = task_sent(&feature);
    let thread_id = threads(&opened)[0].thread.id.clone();
    let task = threads(&opened)[0].thread.comments[0].id.clone();
    let reply_box = TextBox::Reply(thread_id.clone());
    opened.review.open_box(&reply_box).unwrap();
    opened.review.type_in(&reply_box, "And the cart?").unwrap();

    let replied = act(
        &feature,
        &mut opened,
        ThreadAction::Reply {
            thread_id: thread_id.clone(),
            mode: SendMode::SendNow,
            audience: Audience::Author,
        },
    );
    let follow_up = opened.review.hand_offs().all()[1].clone();
    let reply = threads(&opened)[0].thread.comments[1].id.clone();
    opened
        .review
        .open_box(&TextBox::Edit(task.clone()))
        .unwrap();
    opened
        .review
        .type_in(&TextBox::Edit(task.clone()), "Say who the world is, please")
        .unwrap();
    let edited = act(
        &feature,
        &mut opened,
        ThreadAction::EditComment { comment_id: task },
    );
    opened.review.ask_to_delete(&reply);
    let deleted = act(
        &feature,
        &mut opened,
        ThreadAction::DeleteComment { comment_id: reply },
    );
    let resolved = act(
        &feature,
        &mut opened,
        ThreadAction::Resolve {
            thread_id: thread_id.clone(),
        },
    );

    assert_eq!(
        [replied, edited, deleted, resolved],
        [
            ThreadActionResult::Done,
            ThreadActionResult::Done,
            ThreadActionResult::Done,
            ThreadActionResult::Done
        ]
    );
    assert_eq!(follow_up.thread(), Some(&thread_id));
    assert_eq!(follow_up.comment(), "And the cart?");
    assert_eq!(
        follow_up.thread_so_far(),
        [ThreadMessage {
            author: "You".to_owned(),
            body: "Say who the world is".to_owned(),
        }]
    );
    let reopened = feature.reopened();
    let shown = threads(&reopened);
    assert_eq!(
        said(&shown[0]),
        [("You".to_owned(), "Say who the world is, please".to_owned())]
    );
    assert!(shown[0].thread.is_resolved);
    assert!(shown[0].is_folded);
    assert_eq!(reopened.review.hand_offs().all().len(), 2);
    assert_eq!(feature.sent.github_calls(), []);
}

/// Update, as the sidebar's button runs it, then the reads the re-pinned review wants.
fn update(feature: &Feature, opened: &mut OpenedReview) {
    let updated = opened.repinner.run(&feature.github()).unwrap();
    let replaced = std::mem::replace(&mut opened.review, updated.review);
    opened.review.carry_on_from(replaced);
    opened.source = updated.source;
    opened.review.go_to_file(&file_path("README.md"));
    opened.review.pull_from_github();
    feature.run_work(opened);
}

#[test]
fn after_update_a_local_thread_follows_its_line_and_one_whose_line_changed_shows_as_outdated_with_what_it_was()
 {
    let feature = Feature::with_work_in_progress();
    let mut opened = task_sent(&feature);
    feature.repo.write("README.md", "intro\nhello\nworld\n");

    update(&feature, &mut opened);

    let moved = threads(&opened).remove(0);
    assert!(moved.thread.is_outdated);
    assert_eq!(
        moved.anchor.as_ref().map(|at| (at.side, at.line)),
        Some((Side::Head, row(&opened, "world").line))
    );
    assert_eq!(row(&opened, "world").line, crate::support::line(3));

    feature.repo.write("README.md", "hello\nthere\n");
    update(&feature, &mut opened);

    let changed = threads(&opened).remove(0);
    assert!(changed.thread.is_outdated);
    let followed = changed.followed.clone().unwrap();
    assert!(followed.were_changed);
    assert_eq!(followed.before, ["world"]);
    assert_eq!(said(&changed)[0].1, "Say who the world is");
    assert_eq!(feature.sent.github_calls(), []);
}

#[test]
fn while_the_agent_link_is_off_a_comment_says_why_and_stays_a_draft_until_it_is_back() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let place = write_comment(&mut opened, "world", "Say who the world is");

    opened
        .review
        .tour_agent_is(&TourAgentState::LinkOff(AgentLinkOff::ServerNotRunning));

    assert_eq!(
        opened.review.comment_box(&place).unwrap().problem,
        Some(CommentProblem::Refused(CommentRefusal::AgentLinkOff))
    );
    assert!(
        opened
            .review
            .start_sending(&place, SendMode::SendNow, Audience::Agent)
            .is_none()
    );
    assert_eq!(
        opened.review.text_of(&TextBox::Draft(place.clone())),
        Some("Say who the world is")
    );

    opened.review.tour_agent_is(&TourAgentState::Agent {
        terminal: TerminalId::new("t1"),
        status: AgentStatus::Idle,
    });

    assert_eq!(opened.review.comment_box(&place).unwrap().problem, None);
    assert!(
        opened
            .review
            .start_sending(&place, SendMode::SendNow, Audience::Agent)
            .is_some()
    );
}

#[test]
fn a_local_review_offers_only_send_now_and_a_pr_comment_cannot_be_sent_as_a_task() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let place = write_comment(&mut opened, "world", "Say who the world is");

    assert_eq!(opened.review.send_modes(), [SendMode::SendNow]);
    for github_mode in [SendMode::AddToReview, SendMode::CommentNow] {
        assert!(
            opened
                .review
                .start_sending(&place, github_mode, Audience::Author)
                .is_none(),
            "{github_mode:?}"
        );
    }
}

#[test]
fn suggest_change_puts_the_lines_in_the_task_the_agent_gets() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();
    let place = write_comment(&mut opened, "world", "Say it like this:");

    let suggested = opened.review.suggest_change(&place).unwrap();
    let request = opened
        .review
        .start_sending(&place, SendMode::SendNow, Audience::Agent)
        .unwrap();
    opened
        .review
        .apply_comment(request.run(&feature.github()))
        .unwrap();
    feature.run_work(&mut opened);

    assert!(suggested.is_some());
    let typed = feature.typed_into_agent();
    assert!(
        typed[0].contains("```suggestion\nworld\n```"),
        "{}",
        typed[0]
    );
}

#[test]
fn the_task_carries_the_code_the_comment_is_on() {
    let feature = Feature::with_work_in_progress();
    let mut opened = feature.open();

    send_now(&feature, &mut opened, "world", "Say who the world is");

    let typed = feature.typed_into_agent();
    assert!(
        typed[0].contains("Code:\n+ README.md:head:2: world"),
        "{}",
        typed[0]
    );
}
