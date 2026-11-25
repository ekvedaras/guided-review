use std::path::PathBuf;
use std::sync::Arc;

use std::time::Instant;

use crate::support::fake_ports::{FakeAgentLauncher, FakeAskProcesses, FakeAskStore, SentLog};
use crate::support::{FixtureRepo, drafts, file_path, line};
use guided_review::config::AskSettings;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, TourPin};
use guided_review::viewer::{
    AgentLauncher, Ask, AskTarget, Asks, CommentGesture, CommentPlace, CommentRefusal,
    LineLocation, PageRow, Place, Review, ReviewStateFile, Screen, ShownOn, Side, StepId,
    Unavailable,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";
const PONG: &str = include_str!("../fixtures/ask/pong.jsonl");

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "one\ntwo\nthree\n");
        repo.write("b.txt", "uno\ndos\ntres\n");
        let base = repo.commit("base");
        repo.write("a.txt", "one\nTWO\nthree\n");
        repo.write("b.txt", "uno\nDOS\ntres\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn open(&self) -> Review {
        let tour = BasicTour::new(
            TourPin {
                version: 1,
                pr: PR.to_owned(),
                local: None,
                repo: None,
                host: None,
                base: Sha::new(&self.base),
                head: Sha::new(&self.head),
                previous_head: None,
            },
            "Orders cost two".to_owned(),
        );
        let source = LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_subject(
            &self.repo.folders(),
            &PR.parse::<ReviewSubject>().unwrap(),
        );
        Review::open_basic(
            tour,
            &source,
            state_file,
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }

    fn asks(&self) -> Asks {
        Asks::load(Arc::new(ReviewStateFile::for_subject(
            &self.repo.folders(),
            &PR.parse::<ReviewSubject>().unwrap(),
        )))
        .unwrap()
    }
}

fn on_file_screen(review: &mut Review, path: &str) {
    while review.go_back() {}
    let target = Screen::File(file_path(path));
    while review.screen() != &target {
        assert!(review.go_forward(), "{path} is not in the reading order");
    }
}

fn at(path: &str, side: Side, number: usize) -> LineLocation {
    LineLocation {
        path: file_path(path),
        side,
        line: line(number),
    }
}

/// What a drag over rows of the file on screen offers to ask about.
fn lines_on_screen(review: &mut Review, from: &str, to: &str) -> Place {
    let row = |text: &str| {
        review
            .file_page()
            .unwrap()
            .pieces
            .iter()
            .flat_map(guided_review::viewer::PieceDiff::lines)
            .find(|line| line.text == text)
            .unwrap_or_else(|| panic!("no row shows {text:?}"))
            .location
            .clone()
    };
    let (from, to) = (row(from), row(to));
    review.begin_selection(from);
    review.extend_selection(to);
    let place = review.place_of_lines(&review.selected_lines().unwrap());
    review.cancel_selection();
    place
}

fn toolbar_ask(review: &Review) -> Option<Place> {
    review.page().rows().iter().find_map(|row| match row {
        PageRow::Toolbar(toolbar) => toolbar.ask.as_ref().map(|ask| ask.place.clone()),
        _ => None,
    })
}

#[test]
fn the_overview_offers_an_ask_on_the_whole_pr_and_a_file_screen_one_on_its_file() {
    let shop = Shop::new();
    let mut review = shop.open();

    assert_eq!(toolbar_ask(&review), Some(Place::Tour));

    on_file_screen(&mut review, "b.txt");
    assert_eq!(
        toolbar_ask(&review),
        Some(Place::File {
            path: file_path("b.txt")
        })
    );
}

#[test]
fn an_ask_on_lines_of_a_file_screen_is_shown_on_that_file() {
    let shop = Shop::new();
    let mut review = shop.open();
    on_file_screen(&mut review, "a.txt");

    let place = lines_on_screen(&mut review, "two", "TWO");

    let Place::Lines {
        shown_on, pieces, ..
    } = &place
    else {
        panic!("lines were selected: {place:?}");
    };
    assert_eq!(
        shown_on,
        &ShownOn::File {
            path: file_path("a.txt")
        }
    );
    assert_eq!(pieces.len(), 1);
}

#[test]
fn an_ask_on_lines_is_marked_on_the_file_screen_it_was_asked_on_only() {
    let shop = Shop::new();
    let mut review = shop.open();
    on_file_screen(&mut review, "a.txt");
    let mut processes = FakeAskProcesses::default();
    let place = lines_on_screen(&mut review, "TWO", "TWO");

    review.open_ask(
        place.clone(),
        "a.txt line 2".to_owned(),
        &AskSettings::default(),
        Instant::now(),
        &mut processes,
    );
    review.change_asks(|desk| desk.send("Why?", Instant::now(), &mut processes));

    assert_eq!(ask_markers(&review), [place]);
    on_file_screen(&mut review, "b.txt");
    assert_eq!(ask_markers(&review), []);
    assert_eq!(shop.asks().all().len(), 1, "the Ask is in the state file");
}

fn ask_markers(review: &Review) -> Vec<Place> {
    review
        .page()
        .rows()
        .iter()
        .filter_map(|row| match row {
            PageRow::AskMarker(at) => Some(at.place.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_context_of_an_ask_on_the_whole_pr_says_the_tour_is_basic_and_lists_the_files() {
    let shop = Shop::new();
    let review = shop.open();

    let context = review.ask_context(&Place::Tour).unwrap();

    for expected in [
        PR,
        &shop.head,
        "basic",
        "Changed files:",
        "- a.txt",
        "- b.txt",
    ] {
        assert!(
            context.contains(expected),
            "{expected:?} missing in: {context}"
        );
    }
    assert!(!context.contains("Steps:"), "{context}");
    assert!(!context.contains("step"), "{context}");
}

#[test]
fn the_context_of_an_ask_on_a_file_names_the_file_and_has_its_changed_lines() {
    let shop = Shop::new();
    let review = shop.open();
    let place = Place::File {
        path: file_path("a.txt"),
    };

    let context = review.ask_context(&place).unwrap();

    for expected in [
        "basic",
        "a.txt",
        "- a.txt:base:2: two",
        "+ a.txt:head:2: TWO",
    ] {
        assert!(
            context.contains(expected),
            "{expected:?} missing in: {context}"
        );
    }
    assert!(!context.contains("b.txt"), "{context}");
    assert!(!context.contains("step"), "{context}");
}

#[test]
fn the_context_of_an_ask_on_lines_of_a_file_has_the_selected_lines_only() {
    let shop = Shop::new();
    let mut review = shop.open();
    on_file_screen(&mut review, "b.txt");
    let place = lines_on_screen(&mut review, "DOS", "DOS");

    let context = review.ask_context(&place).unwrap();

    assert!(context.contains("basic"), "{context}");
    assert!(context.contains("+ b.txt:head:2: DOS"), "{context}");
    assert!(!context.contains("uno"), "{context}");
    assert!(!context.contains("step"), "{context}");
}

fn ask_at(review: &Review, place: Place) -> Ask {
    let mut asks = Asks::load(Arc::new(FakeAskStore::default())).unwrap();
    let id = asks.start(place, review.head());
    asks.get(id).unwrap().clone()
}

#[test]
fn an_ask_handed_to_the_agent_from_a_file_says_the_tour_is_basic_and_names_the_file() {
    let shop = Shop::new();
    let review = shop.open();
    let ask = ask_at(
        &review,
        Place::File {
            path: file_path("a.txt"),
        },
    );

    let text = review.ask_hand_off_text(&ask, "a.txt");

    for part in [
        PR,
        "File: a.txt",
        "basic tour",
        "guided-review outline",
        "guided-review show",
    ] {
        assert!(text.contains(part), "{part} missing from {text}");
    }
}

#[test]
fn an_ask_handed_to_the_agent_on_the_whole_pr_says_the_tour_is_basic() {
    let shop = Shop::new();
    let review = shop.open();
    let ask = ask_at(&review, Place::Tour);

    let text = review.ask_hand_off_text(&ask, "the whole PR");

    assert!(text.contains("basic tour"), "{text}");
    assert!(!text.contains("File:"), "{text}");
}

#[test]
fn a_place_of_a_step_is_gone_in_a_basic_review() {
    let shop = Shop::new();
    let review = shop.open();

    let step = Place::Step {
        step: StepId::from("order"),
    };

    assert_eq!(review.ask_context(&step), None);
    let target = review.ask_target(
        step,
        "x".to_owned(),
        Some(PathBuf::from("/clones/shop")),
        &AskSettings::default(),
    );
    assert!(matches!(
        target,
        AskTarget::Unavailable {
            why: Unavailable::PlaceGone,
            ..
        }
    ));
}

#[test]
fn an_ask_on_a_file_is_ready_with_a_clone() {
    let shop = Shop::new();
    let review = shop.open();

    let target = review.ask_target(
        Place::File {
            path: file_path("a.txt"),
        },
        "a.txt".to_owned(),
        Some(PathBuf::from("/clones/shop")),
        &AskSettings::default(),
    );

    assert!(matches!(target, AskTarget::Ready { .. }));
}

#[test]
fn asks_saved_before_the_shown_on_value_still_load() {
    let on_safety_net: Ask = serde_json::from_value(json!({
        "id": 1, "head": "abc",
        "place": {"kind": "lines", "step": null,
            "start": {"path": "a.rs", "side": "Head", "line": 3},
            "end": {"path": "a.rs", "side": "Head", "line": 4},
            "pieces": []},
        "messages": [],
    }))
    .unwrap();
    let in_step: Ask = serde_json::from_value(json!({
        "id": 2, "head": "abc",
        "place": {"kind": "lines", "step": "order",
            "start": {"path": "a.rs", "side": "Head", "line": 3},
            "end": {"path": "a.rs", "side": "Head", "line": 4},
            "pieces": []},
        "messages": [],
    }))
    .unwrap();

    let shown_on = |ask: &Ask| match &ask.place {
        Place::Lines {
            shown_on, start, ..
        } => {
            assert_eq!(start, &at("a.rs", Side::Head, 3));
            shown_on.clone()
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(shown_on(&on_safety_net), ShownOn::SafetyNet);
    assert_eq!(
        shown_on(&in_step),
        ShownOn::Step {
            step: StepId::from("order")
        }
    );
}

#[test]
fn a_step_named_safety_net_saved_by_an_old_build_is_still_a_step() {
    let ask: Ask = serde_json::from_value(json!({
        "id": 1, "head": "abc",
        "place": {"kind": "lines", "step": "safety_net",
            "start": {"path": "a.rs", "side": "Head", "line": 3},
            "end": {"path": "a.rs", "side": "Head", "line": 3},
            "pieces": []},
        "messages": [],
    }))
    .unwrap();

    let Place::Lines { shown_on, .. } = ask.place else {
        panic!("lines");
    };
    assert_eq!(
        shown_on,
        ShownOn::Step {
            step: StepId::from("safety_net")
        }
    );
}

#[test]
fn every_shown_on_value_survives_a_save_and_a_load() {
    for shown_on in [
        ShownOn::SafetyNet,
        ShownOn::Step {
            step: StepId::from("order"),
        },
        ShownOn::File {
            path: file_path("a.rs"),
        },
    ] {
        let place = Place::Lines {
            shown_on,
            start: at("a.rs", Side::Head, 3),
            end: at("a.rs", Side::Head, 3),
            pieces: Vec::new(),
        };

        let back: Place = serde_json::from_value(serde_json::to_value(&place).unwrap()).unwrap();

        assert_eq!(back, place);
    }
}

#[test]
fn use_as_comment_fills_a_comment_box_on_the_asked_lines_of_a_file_screen() {
    let shop = Shop::new();
    let mut review = shop.open();
    on_file_screen(&mut review, "a.txt");
    let place = lines_on_screen(&mut review, "TWO", "TWO");
    let context = review.ask_context(&place).unwrap();
    let mut asks = Asks::load(Arc::new(FakeAskStore::default())).unwrap();
    let id = asks.start(place, review.head());
    let mut agent = FakeAgentLauncher {
        sent: SentLog::default(),
        turns: vec![PONG.to_owned()],
    }
    .start(&review.agent_launch(PathBuf::from("/clones/shop"), &AskSettings::default(), None))
    .unwrap();
    asks.ask(id, "Why?", &context, agent.as_mut()).unwrap();

    let comment = review
        .open_comment_box(CommentGesture::AskAnswer(asks.get(id).unwrap()))
        .unwrap()
        .unwrap();

    let CommentPlace::Lines { start, end } = &comment else {
        panic!("a comment on lines");
    };
    assert_eq!((start.line, end.line), (line(2), line(2)));
    let saved = drafts(&shop.open());
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].place, comment);
    assert_eq!(saved[0].body, "pong");
}

#[test]
fn use_as_comment_of_an_ask_on_a_whole_file_has_no_lines_to_comment_on() {
    let shop = Shop::new();
    let mut review = shop.open();
    on_file_screen(&mut review, "a.txt");
    let place = Place::File {
        path: file_path("a.txt"),
    };
    let context = review.ask_context(&place).unwrap();
    let mut asks = Asks::load(Arc::new(FakeAskStore::default())).unwrap();
    let id = asks.start(place, review.head());
    let mut agent = FakeAgentLauncher {
        sent: SentLog::default(),
        turns: vec![PONG.to_owned()],
    }
    .start(&review.agent_launch(PathBuf::from("/clones/shop"), &AskSettings::default(), None))
    .unwrap();
    asks.ask(id, "Why?", &context, agent.as_mut()).unwrap();

    let refused = review.open_comment_box(CommentGesture::AskAnswer(asks.get(id).unwrap()));

    assert_eq!(refused, Err(CommentRefusal::NoLines));
    assert_eq!(drafts(&shop.open()), []);
}
