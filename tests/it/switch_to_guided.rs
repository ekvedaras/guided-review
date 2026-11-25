use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeAskProcesses, FakeHost, HostFault, SentLog};
use crate::support::{FixtureRepo, drafts, file_path, line, save_typing};
use guided_review::config::AskSettings;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, Sha};
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, GuidedTour, Tour, TourPin};
use guided_review::viewer::{
    Asks, Audience, CommentGesture, CommentId, CommentPlace, CommentResult, GuidedTourProgress,
    GuidedTourRefusal, PageRow, Place, Review, ReviewStateFile, ReviewThread, Screen, SendMode,
    ShownOn, Side, TextBox, ThreadComment, ThreadId, TourSwitcher, repin_tour,
};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

const PR: &str = "https://github.com/acme/shop/pull/7";

/// base, then `first` changes a.txt and `second` b.txt (the basic tour's head).
struct Shop {
    repo: FixtureRepo,
    base: String,
    first: String,
    second: String,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        for path in ["a.txt", "b.txt", "c.txt"] {
            repo.write(path, "old\n");
        }
        let base = repo.commit("base");
        repo.write("a.txt", "first\n");
        let first = repo.commit("first");
        repo.write("b.txt", "second\n");
        let second = repo.commit("second");
        let shop = Self {
            repo,
            base,
            first,
            second,
        };
        BasicTour::new(
            TourPin {
                version: 1,
                pr: PR.to_owned(),
                local: None,
                repo: None,
                host: None,
                base: Sha::new(&shop.base),
                head: Sha::new(&shop.second),
                previous_head: None,
            },
            "Orders cost two".to_owned(),
        )
        .save(&shop.basic_path())
        .unwrap();
        shop
    }

    fn pr(&self) -> PullRequestUrl {
        PR.parse().unwrap()
    }

    fn basic_path(&self) -> PathBuf {
        let path = Tour::basic_path_for(&self.repo.folders(), &ReviewSubject::from(self.pr()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        path
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(&self.repo.folders(), &ReviewSubject::from(self.pr()))
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn basic_review(&self) -> Review {
        let tour = BasicTour::load(&self.basic_path()).unwrap();
        Review::open_basic(
            tour,
            &self.source(),
            self.state_file(),
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }

    /// The guided tour the writer wrote at `head`, beside the basic file.
    fn write_guided_tour_at(&self, head: &str) {
        self.write_guided_tour_with_steps(
            head,
            json!([
                {"id": "ab", "title": "A and B", "why": "w",
                 "pieces": [{"file": "a.txt"}, {"file": "b.txt"}]},
                {"id": "ac", "title": "A and C", "why": "w",
                 "pieces": [{"file": "a.txt"}, {"file": "c.txt"}]},
                {"id": "words", "title": "Only words", "why": "w", "pieces": []},
            ]),
        );
    }

    fn write_guided_tour_with_steps(&self, head: &str, steps: serde_json::Value) {
        let path = Tour::path_for(&self.repo.folders(), &ReviewSubject::from(self.pr()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let tour = json!({
            "pr": PR, "base": self.base, "head": head, "title": "T", "summary": [],
            "steps": steps,
        });
        std::fs::write(path, tour.to_string()).unwrap();
    }

    fn commit_third(&self, files: &[&str]) -> String {
        for path in files {
            self.repo.write(path, "third\n");
        }
        self.repo.commit("third")
    }

    fn read_in_basic_mode(&self, path: &str, head: &str) {
        self.state_file()
            .update(|state| state.tick_file(&file_path(path), &Sha::new(head)))
            .unwrap();
    }

    fn read_a_and_b_in_basic_mode(&self) {
        self.read_in_basic_mode("a.txt", &self.first);
        self.read_in_basic_mode("b.txt", &self.second);
    }

    fn switcher(&self) -> TourSwitcher {
        let clone = self.repo.path().to_path_buf();
        TourSwitcher {
            host: crate::support::fake_ports::every_capability(),
            folders: self.repo.folders(),
            subject: self.pr().into(),
            pick_code_source: Arc::new(move |_| {
                Ok(Box::new(
                    LocalGit::find(&crate::support::shop_on_github(), &clone)?.unwrap(),
                ))
            }),
        }
    }
}

fn started_read_line(review: &Review) -> Option<String> {
    review.page().rows().iter().find_map(|row| match row {
        PageRow::SummaryColumns(columns) => columns.summary.iter().find_map(|row| match row {
            PageRow::SummaryStats(stats) => stats.started_read.as_ref().map(ToString::to_string),
            _ => None,
        }),
        _ => None,
    })
}

#[test]
fn a_step_whose_files_were_all_read_starts_ticked_at_the_oldest_head() {
    let shop = Shop::new();
    shop.read_a_and_b_in_basic_mode();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);

    let switched = shop.switcher().run().unwrap().review;

    assert!(switched.is_ticked(&"ab".into()));
    let state = shop.state_file().load().unwrap();
    assert_eq!(
        state.read_tick(&"ab".into()).unwrap().head,
        Sha::new(&shop.first)
    );
}

#[test]
fn a_step_with_an_unread_file_and_a_step_with_no_pieces_start_unread() {
    let shop = Shop::new();
    shop.read_a_and_b_in_basic_mode();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);

    let switched = shop.switcher().run().unwrap().review;

    assert!(!switched.is_ticked(&"ac".into()));
    assert!(!switched.is_ticked(&"words".into()));
}

#[test]
fn a_file_whose_tick_a_re_pin_took_away_still_counts_as_read() {
    let shop = Shop::new();
    shop.read_a_and_b_in_basic_mode();
    // b.txt changes and goes back: the re-pin takes its tick, and nothing differs since it was read
    let changed = shop.commit_third(&["b.txt"]);
    repin_tour(
        &shop.repo.folders(),
        &ReviewSubject::from(shop.pr()),
        &shop.basic_path(),
        &shop.state_file(),
        &shop.source(),
        &Sha::new(&shop.base),
        &Sha::new(&changed),
    )
    .unwrap();
    shop.repo.write("b.txt", "second\n");
    let fourth = shop.repo.commit("back again");
    shop.write_guided_tour_at(&fourth);

    let switched = shop.switcher().run().unwrap().review;

    assert!(switched.is_ticked(&"ab".into()));
}

#[test]
fn a_step_with_a_piece_changed_since_it_was_read_shows_what_changed_instead_of_a_tick() {
    let shop = Shop::new();
    shop.read_a_and_b_in_basic_mode();
    let third = shop.commit_third(&["b.txt"]);
    shop.write_guided_tour_at(&third);

    let mut switched = shop.switcher().run().unwrap().review;

    assert!(!switched.is_ticked(&"ab".into()));
    switched.go_to(&"ab".into());
    let read_at = switched.current_step().unwrap().since_read.unwrap().read_at;
    assert_eq!(read_at, Sha::new(&shop.first));
}

#[test]
fn the_summary_says_how_many_steps_start_read() {
    let shop = Shop::new();
    shop.read_a_and_b_in_basic_mode();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);

    let switched = shop.switcher().run().unwrap().review;

    assert_eq!(switched.screen(), &Screen::Summary);
    assert_eq!(
        started_read_line(&switched).as_deref(),
        Some("1 step starts read")
    );
}

#[test]
fn a_switch_run_again_after_a_crash_before_the_basic_file_went_still_counts_the_steps_that_start_read()
 {
    let shop = Shop::new();
    shop.read_a_and_b_in_basic_mode();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);
    let basic_file = std::fs::read(shop.basic_path()).unwrap();
    shop.switcher().run().unwrap();
    std::fs::write(shop.basic_path(), basic_file).unwrap();

    let switched = shop.switcher().run().unwrap().review;

    assert_eq!(
        started_read_line(&switched).as_deref(),
        Some("1 step starts read")
    );
}

#[test]
fn the_summary_has_no_such_line_when_no_step_starts_read() {
    let shop = Shop::new();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);

    let switched = shop.switcher().run().unwrap().review;

    assert_eq!(started_read_line(&switched), None);
}

#[test]
fn the_file_ticks_and_the_re_pin_record_are_cleared_and_the_basic_file_is_deleted() {
    let shop = Shop::new();
    shop.read_a_and_b_in_basic_mode();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);

    shop.switcher().run().unwrap();

    let state = shop.state_file().load().unwrap();
    assert!(state.file_read_tick(&file_path("a.txt")).is_none());
    assert!(state.file_read_before(&file_path("a.txt")).is_none());
    assert!(state.last_repin().is_none());
    assert!(!shop.basic_path().exists());
}

#[test]
fn the_state_is_saved_before_the_basic_file_goes() {
    let shop = Shop::new();
    shop.read_a_and_b_in_basic_mode();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);
    // a basic file that cannot be deleted: a folder with something in it
    std::fs::remove_file(shop.basic_path()).unwrap();
    std::fs::create_dir(shop.basic_path()).unwrap();
    std::fs::write(shop.basic_path().join("stuck"), "").unwrap();

    assert!(shop.switcher().run().is_err());

    let state = shop.state_file().load().unwrap();
    assert!(state.read_tick(&"ab".into()).is_some());
}

#[test]
fn a_switch_that_finds_no_guided_tour_changes_nothing() {
    let shop = Shop::new();
    shop.read_in_basic_mode("a.txt", &shop.first);

    assert!(shop.switcher().run().is_err());

    assert!(shop.basic_path().exists());
    let state = shop.state_file().load().unwrap();
    assert!(state.file_read_tick(&file_path("a.txt")).is_some());
}

#[test]
fn a_ready_guided_tour_can_be_switched_to_and_nothing_else_can() {
    let shop = Shop::new();
    let mut review = shop.basic_review();
    assert_eq!(
        review.switch_to_guided_tour(),
        Err(GuidedTourRefusal::NothingReady)
    );

    review.guided_tour_is(GuidedTourProgress::Ready);

    assert_eq!(review.switch_to_guided_tour(), Ok(()));
    assert_eq!(review.guided_tour_banner().unwrap().switch_refusal, None);
}

fn on_file_screen(review: &mut Review, path: &str) {
    while review.go_back() {}
    let target = Screen::File(file_path(path));
    while review.screen() != &target {
        assert!(review.go_forward(), "{path} is not in the reading order");
    }
}

/// A basic review with an Ask about the line `first` of a.txt, asked on its file screen.
fn basic_review_with_an_ask_on_a_txt(shop: &Shop) -> Review {
    let mut review = shop.basic_review();
    on_file_screen(&mut review, "a.txt");
    let row = review
        .file_page()
        .unwrap()
        .pieces
        .iter()
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .find(|line| line.text == "first")
        .unwrap()
        .location
        .clone();
    review.begin_selection(row.clone());
    review.extend_selection(row);
    let place = review.place_of_lines(&review.selected_lines().unwrap());
    review.cancel_selection();
    let mut processes = FakeAskProcesses::default();
    review.open_ask(
        place,
        "a.txt line 1".to_owned(),
        &AskSettings::default(),
        Instant::now(),
        &mut processes,
    );
    review.change_asks(|desk| desk.send("Why?", Instant::now(), &mut processes));
    review
}

fn where_the_ask_is_shown(review: &Review) -> ShownOn {
    let Place::Lines { shown_on, .. } = &review.asks().all()[0].place else {
        panic!("the Ask is about lines");
    };
    shown_on.clone()
}

#[test]
fn an_ask_made_on_a_file_screen_moves_to_the_first_step_that_shows_its_lines() {
    let shop = Shop::new();
    let old = basic_review_with_an_ask_on_a_txt(&shop);
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);
    let mut switched = shop.switcher().run().unwrap().review;

    switched.carry_on_from(old);

    assert_eq!(
        where_the_ask_is_shown(&switched),
        ShownOn::Step { step: "ab".into() }
    );
    let saved = Asks::load(Arc::new(shop.state_file())).unwrap();
    assert_eq!(
        saved.all()[0].place,
        switched.asks().all()[0].place,
        "the move is saved"
    );
}

#[test]
fn an_ask_made_on_a_file_screen_no_step_shows_moves_to_the_safety_net() {
    let shop = Shop::new();
    let old = basic_review_with_an_ask_on_a_txt(&shop);
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_with_steps(
        &third,
        json!([{"id": "c", "title": "C", "why": "w", "pieces": [{"file": "c.txt"}]}]),
    );
    let mut switched = shop.switcher().run().unwrap().review;

    switched.carry_on_from(old);

    assert_eq!(where_the_ask_is_shown(&switched), ShownOn::SafetyNet);
}

#[test]
fn the_switched_review_does_not_say_the_guided_tour_is_ready() {
    let shop = Shop::new();
    let mut old = shop.basic_review();
    old.guided_tour_is(GuidedTourProgress::Ready);
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);
    let mut switched = shop.switcher().run().unwrap().review;

    switched.carry_on_from(old);

    assert_eq!(
        switched.guided_tour_progress(),
        &GuidedTourProgress::NotAsked
    );
    assert_eq!(switched.guided_tour_banner(), None);
}

#[test]
fn files_read_while_the_guided_review_is_being_built_are_carried_into_its_steps() {
    let shop = Shop::new();
    shop.read_in_basic_mode("a.txt", &shop.first);
    let mut basic = shop.basic_review();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);
    let switched = shop.switcher().run().unwrap();
    basic.tick_file(&file_path("b.txt")).unwrap();
    let mut guided = switched.review;

    guided.carry_on_from(basic);

    assert!(guided.is_ticked(&"ab".into()));
    assert_eq!(guided.steps_that_started_read(), Some(1));
    let state = shop.state_file().load().unwrap();
    assert_eq!(
        state.read_tick(&"ab".into()).unwrap().head,
        Sha::new(&shop.first),
        "saved at the oldest head its files were read at"
    );
    assert!(state.file_read_tick(&file_path("a.txt")).is_none());
    assert!(state.file_read_tick(&file_path("b.txt")).is_none());
}

/// The switch runs while the basic review stays on screen; `meanwhile` is what the reviewer does
/// on it before the guided review takes over.
fn switched_while(shop: &Shop, meanwhile: impl FnOnce(&mut Review)) -> Review {
    let mut basic = shop.basic_review();
    let third = shop.commit_third(&["c.txt"]);
    shop.write_guided_tour_at(&third);
    let switched = shop.switcher().run().unwrap();
    meanwhile(&mut basic);
    let mut guided = switched.review;
    guided.carry_on_from(basic);
    guided
}

/// The guided review as a restart opens it from what is saved.
fn reopened_guided(shop: &Shop) -> Review {
    let tour = GuidedTour::load(&Tour::path_for(
        &shop.repo.folders(),
        &ReviewSubject::from(shop.pr()),
    ))
    .unwrap();
    Review::open(
        tour,
        &shop.source(),
        shop.state_file(),
        crate::support::fake_ports::every_capability(),
    )
    .unwrap()
}

fn comment_box_on_first(review: &mut Review) -> CommentPlace {
    select_first(review);
    review
        .open_comment_box(CommentGesture::DragEnded)
        .unwrap()
        .unwrap()
}

fn select_first(review: &mut Review) {
    on_file_screen(review, "a.txt");
    let row = review
        .file_page()
        .unwrap()
        .pieces
        .iter()
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .find(|line| line.text == "first")
        .unwrap()
        .location
        .clone();
    review.begin_selection(row.clone());
    review.extend_selection(row);
}

fn mine(body: &str) -> ThreadComment {
    ThreadComment {
        avatar_url: None,
        id: CommentId::new("C1"),
        reactions: Vec::new(),
        author: "me".into(),
        is_bot: false,
        by_agent: None,
        by_viewer: true,
        body: body.into(),
        url: "https://github.com/acme/shop/pull/7#discussion_C1".into(),
    }
}

fn thread_on_first(comments: Vec<ThreadComment>) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new("T1"),
        is_resolved: false,
        is_outdated: false,
        path: file_path("a.txt"),
        line: Some(line(1)),
        start_line: None,
        side: Side::Head,
        is_file_comment: false,
        original_line: Some(line(1)),
        original_start_line: None,
        original_commit: None,
        comments,
    }
}

#[test]
fn a_comment_draft_typed_while_the_guided_review_is_being_built_is_kept() {
    let shop = Shop::new();

    let guided = switched_while(&shop, |basic| {
        let place = comment_box_on_first(basic);
        basic.type_in(&TextBox::Draft(place), "why first?").unwrap();
        save_typing(basic);
    });

    assert_eq!(drafts(&guided).len(), 1);
    assert_eq!(drafts(&guided)[0].body, "why first?");
    assert_eq!(drafts(&reopened_guided(&shop))[0].body, "why first?");
}

#[test]
fn a_review_body_typed_while_the_guided_review_is_being_built_is_kept() {
    let shop = Shop::new();

    let guided = switched_while(&shop, |basic| {
        basic.type_in(&TextBox::FinishBody, "looks good").unwrap();
        save_typing(basic);
    });

    assert_eq!(guided.text_of(&TextBox::FinishBody), Some("looks good"));
    assert_eq!(
        reopened_guided(&shop).text_of(&TextBox::FinishBody),
        Some("looks good")
    );
}

#[test]
fn a_reply_typed_while_the_guided_review_is_being_built_is_kept() {
    let shop = Shop::new();
    let reply = TextBox::Reply(ThreadId::new("T1"));

    let guided = switched_while(&shop, |basic| {
        basic.set_threads(&[thread_on_first(vec![mine("why?")])]);
        basic.open_box(&reply).unwrap();
        basic.type_in(&reply, "because").unwrap();
        save_typing(basic);
    });

    assert_eq!(guided.text_of(&reply), Some("because"));
    assert_eq!(reopened_guided(&shop).text_of(&reply), Some("because"));
}

#[test]
fn a_comment_queued_while_the_guided_review_is_being_built_is_kept() {
    let shop = Shop::new();
    let host = FakeHost::with_pull("main-tip", &shop.second, &SentLog::default());
    host.inject(HostFault::ConnectionLost);

    let guided = switched_while(&shop, |basic| {
        let place = comment_box_on_first(basic);
        basic
            .type_in(&TextBox::Draft(place.clone()), "later")
            .unwrap();
        let result = basic.send_comment(&place, SendMode::AddToReview, &host);
        assert!(matches!(result.unwrap(), Some(CommentResult::Queued(_))));
    });

    assert_eq!(guided.queued_comments().len(), 1);
    assert_eq!(reopened_guided(&shop).queued_comments().len(), 1);
}

#[test]
fn a_hand_off_made_while_the_guided_review_is_being_built_is_kept() {
    let shop = Shop::new();
    let host = FakeHost::with_pull("main-tip", &shop.second, &SentLog::default());

    let guided = switched_while(&shop, |basic| {
        let place = comment_box_on_first(basic);
        basic
            .type_in(&TextBox::Draft(place.clone()), "why first?")
            .unwrap();
        let request = basic
            .start_sending(&place, SendMode::AddToReview, Audience::Agent)
            .unwrap();
        assert_eq!(
            basic.apply_comment(request.run(&host)).unwrap(),
            CommentResult::Sent
        );
    });

    assert_eq!(guided.hand_offs().all().len(), 1);
    assert_eq!(reopened_guided(&shop).hand_offs().all().len(), 1);
}

#[test]
fn an_ask_made_while_the_guided_review_is_being_built_is_kept() {
    let shop = Shop::new();

    let guided = switched_while(&shop, |basic| {
        select_first(basic);
        let place = basic.place_of_lines(&basic.selected_lines().unwrap());
        basic.cancel_selection();
        let mut processes = FakeAskProcesses::default();
        basic.open_ask(
            place,
            "a.txt line 1".to_owned(),
            &AskSettings::default(),
            Instant::now(),
            &mut processes,
        );
        basic.change_asks(|desk| desk.send("Why?", Instant::now(), &mut processes));
    });

    assert_eq!(guided.asks().all().len(), 1);
    assert_eq!(
        Asks::load(Arc::new(shop.state_file())).unwrap().all().len(),
        1
    );
}
