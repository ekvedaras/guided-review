use crate::support::line;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeAgentLauncher, SentLog};
use guided_review::config::AskSettings;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    AgentLaunch, AgentLauncher, AgentPool, AskId, Asks, CommentGesture, CommentRefusal,
    LineLocation, Opening, Place, Review, ReviewStateFile, StepId, TextBox,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";
const PONG: &str = include_str!("../fixtures/ask/pong.jsonl");
const STOPPED: &str = include_str!("../fixtures/ask/stopped.jsonl");
const SESSION: &str = "7f2cd173-4eda-4770-aeb2-c82977b1b673";

fn order_php(total: u32, keep: u32) -> String {
    format!(
        "<?php\n\nclass Order\n{{\n    public function total(): int\n    {{\n        return {total};\n    }}\n\n    public function keep(): int\n    {{\n        return {keep};\n    }}\n}}\n"
    )
}

/// A tour pinned to the first head; the second head changes `total` again and leaves `keep`.
struct Shop {
    repo: FixtureRepo,
    base: String,
    first_head: String,
}

impl Shop {
    fn with_tour_at_first_head() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("app/Order.php", &order_php(1, 1));
        let base = repo.commit("base");
        repo.write("app/Order.php", &order_php(2, 9));
        let first_head = repo.commit("first head");
        let tour = json!({
            "pr": PR,
            "base": base,
            "head": first_head,
            "title": "Orders",
            "steps": [
                {"id": "total", "title": "Total", "why": "pricing moved", "pieces": [{"file": "app/Order.php", "symbol": "Order::total"}]},
                {"id": "keep", "title": "Keep", "why": "kept", "pieces": [{"file": "app/Order.php", "symbol": "Order::keep"}]},
            ],
        });
        let shop = Self {
            repo,
            base,
            first_head,
        };
        let path = shop.tour_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
        shop
    }

    fn commit_second_head(&self) -> String {
        self.repo.write("app/Order.php", &order_php(3, 9));
        self.repo.commit("second head")
    }

    fn pr(&self) -> PullRequestUrl {
        PR.parse().unwrap()
    }

    fn tour_path(&self) -> PathBuf {
        Tour::path_for(self.repo.home(), &self.pr())
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_pr(self.repo.home(), &self.pr())
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour = Tour::load(&self.tour_path()).unwrap();
        Review::open(tour, &self.source(), self.state_file()).unwrap()
    }

    fn asks(&self) -> Asks {
        Asks::load(Arc::new(self.state_file())).unwrap()
    }

    fn repin_onto(&self, head: &str) {
        guided_review::viewer::repin_tour(
            &self.tour_path(),
            &self.state_file(),
            &self.source(),
            &Sha::new(&self.base),
            &Sha::new(head),
        )
        .unwrap();
    }
}

fn total_step() -> Place {
    Place::Step {
        step: StepId::from("total"),
    }
}

fn row(review: &Review, text: &str) -> LineLocation {
    review
        .current_step()
        .unwrap()
        .pieces
        .iter()
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .find(|line| line.text.trim() == text)
        .unwrap_or_else(|| panic!("no row shows {text:?}"))
        .location
        .clone()
}

/// What a drag over rows of the total step offers to ask about.
fn lines_of_total(review: &mut Review, from: &str, to: &str) -> Place {
    review.go_to(&StepId::from("total"));
    let (from, to) = (row(review, from), row(review, to));
    review.begin_selection(from);
    review.extend_selection(to);
    let place = review.place_of_lines(&review.selected_lines().unwrap());
    review.cancel_selection();
    place
}

fn launch(review: &Review) -> AgentLaunch {
    review.agent_launch(PathBuf::from("/clones/shop"), &AskSettings::default(), None)
}

fn launcher(turns: &[&str], sent: &SentLog) -> FakeAgentLauncher {
    FakeAgentLauncher {
        sent: sent.clone(),
        turns: turns.iter().map(ToString::to_string).collect(),
    }
}

fn ask(n: u64) -> AskId {
    serde_json::from_value(n.into()).unwrap()
}

fn minutes(n: u64) -> Duration {
    Duration::from_secs(n * 60)
}

#[test]
fn a_fourth_ask_stops_the_process_used_longest_ago() {
    let at = Instant::now();
    let mut pool = AgentPool::default();
    pool.open(ask(1), at);
    pool.open(ask(2), at + minutes(1));
    pool.open(ask(3), at + minutes(2));
    pool.turn_started(ask(1), at + minutes(3));
    pool.turn_ended(ask(1), at + minutes(3));

    let opening = pool.open(ask(4), at + minutes(4));

    assert_eq!(opening, Opening::Start { stop: Some(ask(2)) });
    assert!(!pool.is_live(ask(2)));
    assert!(pool.is_live(ask(4)));
}

#[test]
fn an_ask_with_a_live_process_keeps_it() {
    let at = Instant::now();
    let mut pool = AgentPool::default();
    pool.open(ask(1), at);

    assert_eq!(pool.open(ask(1), at + minutes(1)), Opening::Live);
}

#[test]
fn a_process_answering_is_never_stopped_for_another_and_when_all_three_answer_the_next_waits() {
    let at = Instant::now();
    let mut pool = AgentPool::default();
    for n in 1..=3 {
        pool.open(ask(n), at);
        pool.turn_started(ask(n), at);
    }

    assert_eq!(pool.open(ask(4), at + minutes(1)), Opening::AllAnswering);
    assert!(!pool.is_live(ask(4)));

    pool.turn_ended(ask(2), at + minutes(2));
    assert_eq!(
        pool.open(ask(4), at + minutes(3)),
        Opening::Start { stop: Some(ask(2)) }
    );
}

#[test]
fn processes_idle_ten_minutes_are_due_to_stop_but_one_answering_is_not() {
    let at = Instant::now();
    let mut pool = AgentPool::default();
    pool.open(ask(1), at);
    pool.open(ask(2), at);
    pool.turn_started(ask(2), at);
    pool.open(ask(3), at + minutes(5));

    assert_eq!(pool.stop_idle(at + minutes(9)), []);
    assert_eq!(pool.stop_idle(at + minutes(10)), [ask(1)]);
    assert!(!pool.is_live(ask(1)));
    assert!(pool.is_live(ask(2)));
    assert_eq!(pool.stop_idle(at + minutes(15)), [ask(3)]);
}

#[test]
fn a_process_that_ended_frees_its_place() {
    let at = Instant::now();
    let mut pool = AgentPool::default();
    for n in 1..=3 {
        pool.open(ask(n), at);
    }

    pool.ended(ask(1));

    assert_eq!(
        pool.open(ask(4), at + minutes(1)),
        Opening::Start { stop: None }
    );
}

#[test]
fn after_a_restart_a_follow_up_resumes_the_session_and_sends_only_the_question() {
    let shop = Shop::with_tour_at_first_head();
    let review = shop.open();
    let context = review.ask_context(&total_step()).unwrap();
    let sent = SentLog::default();
    let launcher = launcher(&[PONG], &sent);
    let mut asks = shop.asks();
    let id = asks.start(total_step(), review.head());
    let mut agent = launcher
        .start(&asks.launch_for(id, launch(&review)))
        .unwrap();
    asks.ask(id, "Why?", &context, agent.as_mut()).unwrap();
    drop((agent, asks));

    let mut asks = shop.asks();
    let mut agent = launcher
        .start(&asks.launch_for(id, launch(&review)))
        .unwrap();
    asks.ask(id, "And in tests?", &context, agent.as_mut())
        .unwrap();

    let entries = sent.agent_lines();
    assert_eq!(
        entries[0],
        "agent started in /clones/shop with opus / medium"
    );
    assert_eq!(
        entries[2],
        format!("agent started in /clones/shop with opus / medium, resuming {SESSION}")
    );
    assert_eq!(entries[3], "agent: And in tests?");
    assert_eq!(asks.get(id).unwrap().messages.len(), 4);
}

#[test]
fn an_ask_with_no_answer_yet_starts_a_new_session() {
    let shop = Shop::with_tour_at_first_head();
    let review = shop.open();
    let mut asks = shop.asks();
    let id = asks.start(total_step(), review.head());

    assert_eq!(asks.launch_for(id, launch(&review)).resume, None);
}

#[test]
fn stopping_a_turn_keeps_the_answer_so_far_marked_stopped_and_the_session() {
    let shop = Shop::with_tour_at_first_head();
    let review = shop.open();
    let context = review.ask_context(&total_step()).unwrap();
    let sent = SentLog::default();
    let mut agent = launcher(&[STOPPED], &sent).start(&launch(&review)).unwrap();
    let mut asks = shop.asks();
    let id = asks.start(total_step(), review.head());

    let message = asks.begin_turn(id, "Why?", &context);
    asks.stop_turn(id, agent.turn_stopper().as_ref());
    agent
        .ask(&message, &mut |line| asks.apply_line(id, line))
        .unwrap();

    assert!(
        sent.agent_lines()
            .contains(&"agent: stop the turn".to_owned())
    );
    let ask = asks.get(id).unwrap();
    assert_eq!(ask.messages[1].text, "It returns");
    assert!(ask.messages[1].stopped);
    assert_eq!(ask.failure, None);
    assert_eq!(ask.session_id.as_deref(), Some(SESSION));
    let reloaded = shop.asks();
    assert!(reloaded.get(id).unwrap().messages[1].stopped);
}

#[test]
fn an_ask_on_an_old_head_is_pinned_to_the_tour_head() {
    let shop = Shop::with_tour_at_first_head();
    let second_head = shop.commit_second_head();
    let mut review = shop.open();
    review.pr_head_is(Sha::new(&second_head));
    assert!(review.old_head().is_some());
    let mut asks = shop.asks();

    let id = asks.start(total_step(), review.head());
    let context = review.ask_context(&total_step()).unwrap();

    assert_eq!(asks.get(id).unwrap().head, Sha::new(&shop.first_head));
    assert!(context.contains(&format!("Head commit: {}", shop.first_head)));
    assert!(!context.contains(&second_head));
    assert!(context.contains("return 2;"));
}

#[test]
fn asking_about_selected_lines_sends_only_those_lines_with_their_step() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();

    let place = lines_of_total(&mut review, "return 2;", "return 2;");
    let context = review.ask_context(&place).unwrap();

    assert!(context.contains("pricing moved"), "{context}");
    assert!(context.contains("app/Order.php:head:7: "), "{context}");
    assert!(context.contains("return 2;"), "{context}");
    assert!(!context.contains("return 1;"), "{context}");
    assert!(context.contains("app/Order.php#Order::total"), "{context}");
}

#[test]
fn asks_about_lines_are_kept_per_selection_newest_first() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();
    let place = lines_of_total(&mut review, "return 1;", "return 2;");
    let context = review.ask_context(&place).unwrap();
    let sent = SentLog::default();
    let mut agent = launcher(&[PONG, PONG], &sent)
        .start(&launch(&review))
        .unwrap();
    let mut asks = shop.asks();
    let first = asks.start(place.clone(), review.head());
    asks.ask(first, "first?", &context, agent.as_mut()).unwrap();
    let second = asks.start(place, review.head());
    asks.ask(second, "second?", &context, agent.as_mut())
        .unwrap();

    let again = lines_of_total(&mut review, "return 1;", "return 2;");
    let reloaded = shop.asks();

    let questions: Vec<&str> = reloaded
        .at(&again)
        .iter()
        .map(|ask| ask.messages[0].text.as_str())
        .collect();
    assert_eq!(questions, ["second?", "first?"]);
    assert_eq!(reloaded.count_at(&total_step()), 0);
}

#[test]
fn after_a_re_pin_asks_whose_piece_changed_say_code_changed_since_and_the_others_do_not() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();
    let on_total_lines = lines_of_total(&mut review, "return 2;", "return 2;");
    let mut asks = shop.asks();
    let places = [
        total_step(),
        on_total_lines,
        Place::Piece {
            step: StepId::from("total"),
            piece: "app/Order.php#Order::total".parse().unwrap(),
        },
        Place::Step {
            step: StepId::from("keep"),
        },
        Place::Tour,
    ];
    let ids: Vec<AskId> = places
        .iter()
        .map(|place| asks.start(place.clone(), review.head()))
        .collect();
    let changed_before: Vec<bool> = ids
        .iter()
        .map(|id| review.code_changed_since(asks.get(*id).unwrap()))
        .collect();

    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let review = shop.open();
    let changed_after: Vec<bool> = ids
        .iter()
        .map(|id| review.code_changed_since(asks.get(*id).unwrap()))
        .collect();
    let asked_after_the_re_pin = asks.start(total_step(), review.head());

    assert_eq!(changed_before, [false; 5]);
    assert_eq!(changed_after, [true, true, true, false, false]);
    assert!(!review.code_changed_since(asks.get(asked_after_the_re_pin).unwrap()));
}

fn answered(shop: &Shop, review: &Review, place: Place) -> (Asks, AskId) {
    let context = review.ask_context(&place).unwrap_or_default();
    let mut agent = launcher(&[PONG], &SentLog::default())
        .start(&launch(review))
        .unwrap();
    let mut asks = shop.asks();
    let id = asks.start(place, review.head());
    asks.ask(id, "Why?", &context, agent.as_mut()).unwrap();
    (asks, id)
}

#[test]
fn use_as_comment_fills_a_comment_box_on_the_asked_lines_with_the_answer() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();
    let place = lines_of_total(&mut review, "return 1;", "return 2;");
    let (asks, id) = answered(&shop, &review, place);

    let comment = review
        .open_comment_box(CommentGesture::AskAnswer(asks.get(id).unwrap()))
        .unwrap()
        .unwrap();

    let guided_review::viewer::CommentPlace::Lines { start, end } = &comment else {
        panic!("a comment on lines is not on a file");
    };
    assert_eq!((start.line, end.line), (line(7), line(7)));
    assert!(!comment.is_single_line());
    let drafts = shop.open().comment_drafts().to_vec();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].place, comment);
    assert_eq!(drafts[0].body, "pong");
}

#[test]
fn use_as_comment_on_a_piece_comments_on_all_its_lines_and_adds_to_text_already_there() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();
    let piece = Place::Piece {
        step: StepId::from("total"),
        piece: "app/Order.php#Order::total".parse().unwrap(),
    };
    let (asks, id) = answered(&shop, &review, piece);
    let first = review
        .open_comment_box(CommentGesture::AskAnswer(asks.get(id).unwrap()))
        .unwrap()
        .unwrap();
    review
        .type_in(&TextBox::Draft(first.clone()), "My note")
        .unwrap();

    let second = review
        .open_comment_box(CommentGesture::AskAnswer(asks.get(id).unwrap()))
        .unwrap()
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(review.comment_drafts()[0].body, "My note\n\npong");
}

#[test]
fn use_as_comment_is_refused_without_lines_on_an_old_tour_and_for_an_ask_on_older_code() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();
    let on_lines = lines_of_total(&mut review, "return 2;", "return 2;");
    let (asks, on_step) = answered(&shop, &review, total_step());
    let (asks_on_lines, on_lines) = answered(&shop, &review, on_lines);

    assert_eq!(
        review.open_comment_box(CommentGesture::AskAnswer(asks.get(on_step).unwrap())),
        Err(CommentRefusal::NoLines)
    );
    let second_head = shop.commit_second_head();
    review.pr_head_is(Sha::new(&second_head));
    assert_eq!(
        review.open_comment_box(CommentGesture::AskAnswer(
            asks_on_lines.get(on_lines).unwrap()
        )),
        Err(CommentRefusal::TourIsOld)
    );
    shop.repin_onto(&second_head);
    let mut review = shop.open();
    assert_eq!(
        review.open_comment_box(CommentGesture::AskAnswer(
            asks_on_lines.get(on_lines).unwrap()
        )),
        Err(CommentRefusal::AskIsOnOlderCode)
    );
    assert_eq!(review.comment_drafts(), []);
}

#[test]
fn use_as_comment_is_refused_for_an_ask_with_no_answer_yet() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();
    let place = lines_of_total(&mut review, "return 1;", "return 2;");
    let mut asks = shop.asks();
    let id = asks.start(place, review.head());

    let refused = review.open_comment_box(CommentGesture::AskAnswer(asks.get(id).unwrap()));

    assert_eq!(refused, Err(CommentRefusal::NoAnswerYet));
    assert_eq!(shop.open().comment_drafts().first(), None);
}

#[test]
fn the_pool_counts_the_turns_answering_now() {
    let at = Instant::now();
    let mut pool = AgentPool::default();
    pool.open(ask(1), at);
    pool.open(ask(2), at);
    assert_eq!(pool.answering(), 0);

    pool.turn_started(ask(1), at);
    pool.turn_started(ask(2), at);
    assert_eq!(pool.answering(), 2);

    pool.turn_ended(ask(1), at + minutes(1));
    assert_eq!(pool.answering(), 1);

    pool.ended(ask(2));
    assert_eq!(pool.answering(), 0);
}

#[test]
fn a_comment_box_on_lines_asks_about_the_same_place_as_the_drag_that_opened_it() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();
    review.go_to(&StepId::from("total"));
    let (from, to) = (row(&review, "return 1;"), row(&review, "return 2;"));
    review.begin_selection(from);
    review.extend_selection(to);
    let dragged = review.place_of_lines(&review.selected_lines().unwrap());

    let comment_place = review
        .open_comment_box(CommentGesture::DragEnded)
        .unwrap()
        .unwrap();

    assert_eq!(review.ask_place_of(&comment_place), Some(dragged));
    review.go_to_summary();
    assert_eq!(
        review.ask_place_of(&comment_place),
        None,
        "the lines are not on this screen"
    );
}
