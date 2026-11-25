use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::support::fake_ports::{FakeHost, SentLog};
use crate::support::fake_tour_agent::{FakeTourAgent, TourAgentCall};
use crate::support::work::drive_without_agents;
use crate::support::{FakeGitHub, FixtureRepo, GuidedReview, file_path};
use guided_review::local_work_tree::{LocalWorkTree, snapshot_ref};
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::opened_subject::OpenedChangeSet;
use guided_review::opened_subject::TourRepinner;
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, GuidedTour, Tour, ToursOnDisk};
use guided_review::viewer::{
    ActivePr, AgentStatus, DiskOffer, OpenDecision, OpenOrigin, OpenOutcome, Review,
    ReviewStateFile, TerminalId, TourAgentPort, TourAgents, TourReloader, TourSwitcher,
    WriteDecision, WriterHandOver,
};
use serde_json::json;

/// `feature` one commit past `main` that adds the cart, with the README changed and not committed.
struct Feature {
    repo: FixtureRepo,
    subject: ReviewSubject,
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
        Self { repo, subject }
    }

    fn key(&self) -> String {
        self.subject.to_string()
    }

    fn opened_change_set(&self) -> OpenedChangeSet {
        OpenedChangeSet::new(self.subject.local().unwrap(), &self.repo.folders())
    }

    /// The basic tour, as `open` makes it.
    fn with_basic_tour(self) -> Self {
        self.opened_change_set()
            .make_basic_tour_if_no_tour()
            .unwrap();
        self
    }

    fn basic_pin_head(&self) -> String {
        BasicTour::load(&Tour::basic_path_for(&self.repo.folders(), &self.subject))
            .unwrap()
            .pin
            .head
            .to_string()
    }

    fn guided_path(&self) -> PathBuf {
        let path = Tour::path_for(&self.repo.folders(), &self.subject);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        path
    }

    /// What the tour writer leaves: a tour with no pin, the tool owns it.
    fn writer_writes_a_tour(&self, steps: serde_json::Value) {
        let tour = json!({"title": "Cart", "summary": ["A cart"], "steps": steps});
        std::fs::write(self.guided_path(), tour.to_string()).unwrap();
    }

    fn writer_writes_a_tour_of_both_files(&self) {
        self.writer_writes_a_tour(json!([
            {"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]},
            {"id": "cart", "title": "Cart", "why": "w", "pieces": [{"rest": ""}]},
        ]));
    }

    fn run(&self, args: &[&str]) -> crate::support::Run {
        GuidedReview::new(&self.repo, &no_github()).run(args)
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(&self.repo.folders(), &self.subject)
    }

    fn open(&self) -> OpenedReview {
        open_review(&self.subject, self.repo.path(), &self.repo.folders()).unwrap()
    }

    fn switcher(&self) -> TourSwitcher {
        TourSwitcher {
            folders: self.repo.folders(),
            subject: self.subject.clone(),
            pick_code_source: self.opened_change_set().code_source_picker(),
        }
    }

    fn reloader(&self) -> TourReloader {
        TourReloader {
            folders: self.repo.folders(),
            subject: self.subject.clone(),
            pick_code_source: self.opened_change_set().code_source_picker(),
        }
    }

    fn repinner(&self) -> TourRepinner {
        TourRepinner {
            folders: self.repo.folders(),
            subject: self.subject.clone(),
            pick_code_source: self.opened_change_set().code_source_picker(),
        }
    }
}

/// Where GitHub would be: nothing listens there, so any call to it fails the run it is made in.
fn no_github() -> FakeGitHub {
    FakeGitHub {
        url: "http://127.0.0.1:1".to_owned(),
    }
}

fn agents(fake: &Arc<FakeTourAgent>) -> TourAgents {
    TourAgents::new(Arc::clone(fake) as Arc<dyn TourAgentPort>)
}

// The writer task goes to the change set's agent.

#[test]
fn an_idle_change_set_agent_is_typed_the_writer_task_and_not_ended() {
    let feature = Feature::with_work_in_progress();
    let key = feature.key();
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t3", Some(&key), AgentStatus::Idle));

    let handed = agents(&fake)
        .hand_writer_task(&feature.subject, &feature.subject)
        .unwrap();

    assert_eq!(handed, WriterHandOver::Delivered(TerminalId::new("t3")));
    let calls = fake.calls();
    let [TourAgentCall::Send { terminal, text }] = calls.as_slice() else {
        panic!("one send and nothing else, got {calls:?}");
    };
    assert_eq!(terminal, &TerminalId::new("t3"));
    assert!(text.contains("subagent"), "{text}");
    assert!(text.contains("Skill tool"), "{text}");
    assert!(!text.starts_with("/tour"), "{text}");
    assert!(
        text.contains(&format!("guided-review tour-path {key}")),
        "{text}"
    );
    assert!(text.contains("local change set"), "{text}");
    assert!(!text.contains("PR"), "{text}");
}

#[test]
fn a_working_change_set_agent_finishes_first_so_the_task_waits() {
    let feature = Feature::with_work_in_progress();
    let key = feature.key();
    let fake = Arc::new(FakeTourAgent::new().with_terminal("t3", Some(&key), AgentStatus::Working));

    let handed = agents(&fake)
        .hand_writer_task(&feature.subject, &feature.subject)
        .unwrap();

    assert_eq!(handed, WriterHandOver::Waiting(TerminalId::new("t3")));
    assert_eq!(fake.calls(), vec![]);
}

#[test]
fn a_change_set_with_no_agent_gets_one_started_in_its_folder_with_the_task() {
    let feature = Feature::with_work_in_progress();
    let fake = Arc::new(FakeTourAgent::new());

    let handed = agents(&fake)
        .hand_writer_task(&feature.subject, &feature.subject)
        .unwrap();

    assert!(matches!(handed, WriterHandOver::Started(_)), "{handed:?}");
    let calls = fake.calls();
    let [
        TourAgentCall::Start {
            folder, first_task, ..
        },
    ] = calls.as_slice()
    else {
        panic!("one start, got {calls:?}");
    };
    assert_eq!(folder, feature.subject.local().unwrap().folder().as_path());
    assert!(
        first_task.starts_with(&format!("/tour {}", feature.key())),
        "{first_task}"
    );
}

#[test]
fn several_agents_in_the_folder_stop_the_task_until_the_reviewer_picks() {
    let feature = Feature::with_work_in_progress();
    let folder = feature
        .subject
        .local()
        .unwrap()
        .folder()
        .as_path()
        .display()
        .to_string();
    let fake = Arc::new(
        FakeTourAgent::new()
            .claude_in("t2", &folder)
            .claude_in("t3", &folder),
    );

    let error = agents(&fake)
        .hand_writer_task(&feature.subject, &feature.subject)
        .unwrap_err();

    assert!(format!("{error:#}").contains("pick one"), "{error:#}");
    assert_eq!(fake.calls(), vec![]);
}

// The app's writing and ready states work for a local subject as for a PR.

#[test]
fn a_local_writing_and_the_agents_open_of_it_say_guided_ready_and_change_nothing() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    let mut active = ActivePr::default();
    let OpenDecision::Build(ticket) = active.request(feature.subject.clone(), OpenOrigin::Reviewer)
    else {
        panic!("builds");
    };
    active.finished(ticket, Ok::<_, anyhow::Error>(()));

    assert_eq!(
        active.write_in_background(feature.subject.clone()),
        WriteDecision::StartWriter
    );
    feature.writer_writes_a_tour_of_both_files();
    let outcome = active.open(
        feature.subject.clone(),
        OpenOrigin::AppStartedAgent,
        ToursOnDisk::of(&feature.repo.folders(), &feature.subject),
    );

    assert!(matches!(
        outcome,
        OpenOutcome::Tour(OpenDecision::GuidedReady)
    ));
    assert_eq!(active.shown(), Some(&feature.subject));
}

// `check`, `refresh` and `tour-path` take the local key.

#[test]
fn check_pins_the_writers_tour_to_the_snapshot_the_reviewer_reads_and_reports() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.writer_writes_a_tour_of_both_files();

    let run = feature.run(&["check", &feature.key()]);

    assert!(run.success, "{}{}", run.stdout, run.stderr);
    let tour = GuidedTour::load(&feature.guided_path()).unwrap();
    assert_eq!(tour.pin.head, Sha::new(&feature.basic_pin_head()));
    assert_eq!(tour.pin.local.as_deref(), Some(feature.key().as_str()));
    assert_eq!(tour.pin.pr, "");
}

#[test]
fn check_names_the_repo_of_the_clone_so_a_pr_of_the_repo_finds_the_guided_tour_without_git() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.writer_writes_a_tour_of_both_files();

    assert!(feature.run(&["check", &feature.key()]).success);
    std::fs::remove_file(Tour::basic_path_for(
        &feature.repo.folders(),
        &feature.subject,
    ))
    .unwrap();

    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    assert_eq!(
        guided_review::viewer::local_reviews_of_repo(&feature.repo.folders(), &pr),
        [feature.subject.local().unwrap().clone()]
    );
}

#[test]
fn check_after_an_update_finds_the_guided_tour_on_the_new_snapshot_and_warns_of_nothing() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);
    feature.repo.write("notes.txt", "more\n");
    assert!(feature.run(&["repin", &feature.key()]).success);

    let run = feature.run(&["check", &feature.key()]);

    assert!(run.success, "{}{}", run.stdout, run.stderr);
    assert!(!run.stdout.contains("new snapshot"), "{}", run.stdout);
}

#[test]
fn repin_moves_the_waiting_guided_tour_with_the_basic_one_before_the_ref_moves() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);
    let old_head = feature.basic_pin_head();
    feature.repo.write("notes.txt", "more\n");

    let run = feature.run(&["repin", &feature.key()]);

    assert!(run.success, "{}{}", run.stdout, run.stderr);
    let new_head = feature.basic_pin_head();
    assert_ne!(new_head, old_head);
    let guided = GuidedTour::load(&feature.guided_path()).unwrap();
    assert_eq!(guided.pin.head.to_string(), new_head);
    assert_eq!(guided.pin.previous_head, Some(Sha::new(&old_head)));
    assert_eq!(
        feature
            .repo
            .git(&["rev-parse", &snapshot_ref(feature.subject.local().unwrap())]),
        new_head
    );
}

#[test]
fn refresh_prints_the_brief_of_the_local_tour_and_done_clears_the_marks() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);

    let brief = feature.run(&["refresh", &feature.key()]);
    let done = feature.run(&["refresh", &feature.key(), "--done"]);

    assert!(brief.success, "{}", brief.stderr);
    assert!(
        brief.stdout.contains("nothing to refresh"),
        "{}",
        brief.stdout
    );
    assert!(done.success, "{}", done.stderr);
    assert!(done.stdout.contains("marks cleared"), "{}", done.stdout);
}

#[test]
fn tour_path_prints_the_local_guided_tour_path() {
    let feature = Feature::with_work_in_progress().with_basic_tour();

    let run = feature.run(&["tour-path", &feature.key()]);

    assert!(run.success, "{}", run.stderr);
    assert_eq!(
        run.stdout.trim(),
        feature.guided_path().display().to_string()
    );
}

// Switch to guided tour keeps the local review whole.

fn on_focus(review: &mut Review, source: &dyn guided_review::code_source::CodeSource) {
    let sent = SentLog::default();
    let host = FakeHost::with_pull("main-tip", "snapshot", &sent);
    review.pull_from_host();
    drive_without_agents(review, &host, source, Instant::now());
    assert_eq!(sent.host_calls(), []);
}

#[test]
fn the_switched_guided_review_carries_ticks_and_keeps_its_local_facts() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    let head = Sha::new(&feature.basic_pin_head());
    feature
        .state_file()
        .update(|state| state.tick_file(&file_path("README.md"), &head))
        .unwrap();
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);

    let switched = feature.switcher().run().unwrap();

    assert!(switched.review.is_ticked(&"readme".into()));
    assert!(!switched.review.is_ticked(&"cart".into()));
    let facts = switched.review.local_facts().expect("the facts are kept");
    assert_eq!(facts.commit_subjects, ["Add the cart"]);
}

#[test]
fn the_switched_guided_review_goes_on_checking_its_folder() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);
    let mut switched = feature.switcher().run().unwrap();
    feature.repo.write("notes.txt", "a new file\n");

    on_focus(&mut switched.review, switched.source.as_ref());

    let news = switched.review.disk_news().expect("changed on disk");
    assert_eq!(news.offer, Some(DiskOffer::Update));
}

#[test]
fn files_read_while_the_guided_review_is_built_are_carried_and_not_overwritten() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    let head = Sha::new(&feature.basic_pin_head());
    let mut basic = feature.open().review;
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);
    let switched = feature.switcher().run().unwrap();
    basic.tick_file(&file_path("README.md")).unwrap();
    let mut guided = switched.review;

    guided.carry_on_from(basic);

    assert!(guided.is_ticked(&"readme".into()));
    let state = feature.state_file().load().unwrap();
    assert_eq!(state.read_tick(&"readme".into()).unwrap().head, head);
}

#[test]
fn a_reloaded_guided_local_review_keeps_its_facts() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);

    let reloaded = feature.reloader().run().unwrap();

    assert!(reloaded.review.local_facts().is_some());
}

#[test]
fn after_update_a_guided_local_tour_offers_refresh_as_after_new_commits_on_a_pr() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);
    feature.switcher().run().unwrap();
    feature
        .repo
        .write("src/cart.rs", "pub fn total() -> u32 { 1 }\n");

    let updated = feature
        .repinner()
        .run(&FakeHost::with_pull(
            "main-tip",
            "snapshot",
            &SentLog::default(),
        ))
        .unwrap();

    assert_eq!(updated.review.refresh_brief().stale_steps.len(), 1);
    assert!(updated.review.local_facts().is_some());
}

#[test]
fn a_tour_written_after_the_local_review_was_moved_or_dropped_is_refused_by_check_and_deleted() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    feature.opened_change_set().drop_review().unwrap();
    feature.writer_writes_a_tour_of_both_files();

    let run = feature.run(&["check", &feature.key()]);

    assert!(!run.success, "{}", run.stdout);
    assert!(
        run.stderr
            .contains("was dropped or moved to a PR, so its tour is not kept"),
        "{}",
        run.stderr
    );
    assert!(!feature.guided_path().exists());
    assert_eq!(
        ToursOnDisk::of(&feature.repo.folders(), &feature.subject),
        ToursOnDisk::None
    );
}

#[test]
fn a_move_declined_while_the_guided_review_is_built_stays_declined_in_it() {
    let feature = Feature::with_work_in_progress().with_basic_tour();
    let mut basic = feature.open().review;
    basic.pull_of_branch_is(Some(guided_review::viewer::PullOfBranch {
        pr: "https://github.com/acme/shop/pull/7".parse().unwrap(),
        title: "Add the cart".to_owned(),
    }));
    feature.writer_writes_a_tour_of_both_files();
    assert!(feature.run(&["check", &feature.key()]).success);
    let switched = feature.switcher().run().unwrap();
    basic.decline_the_move();
    let mut guided = switched.review;

    guided.carry_on_from(basic);

    assert_eq!(guided.branch_offer(), None);
}
