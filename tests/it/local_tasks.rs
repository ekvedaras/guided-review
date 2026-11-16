use std::sync::Arc;
use std::time::Instant;

use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::work::drive_until_idle;
use crate::support::{FixtureRepo, file_path};
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::review_subject::ReviewSubject;
use guided_review::viewer::{
    AgentStatus, Audience, CommentGesture, CommentPlace, CommentResult, HandOffState, LineLocation,
    PieceDiff, SendMode, TextBox, ThreadView, TourAgentPort, TourAgents, WorkPorts,
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
