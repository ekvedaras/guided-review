use crate::support::FixtureRepo;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, GuidedTour, TourPin};
use guided_review::update::Build;
use guided_review::viewer::{
    AgentLinkOff, Effect, GuidedTourProgress, GuidedTourRefusal, Intent, PaletteAction, Review,
    ReviewStateFile, Screen, TourAgentState, WindowMoment,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "one\n");
        let base = repo.commit("base");
        repo.write("a.txt", "one\ntwo\n");
        repo.write("b.txt", "b\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(&self.repo.folders(), &PR.parse::<ReviewSubject>().unwrap())
    }

    fn pin(&self) -> TourPin {
        TourPin {
            version: 1,
            pr: PR.to_owned(),
            local: None,
            base: Sha::new(&self.base),
            head: Sha::new(&self.head),
            previous_head: None,
        }
    }

    fn basic(&self) -> Review {
        let tour = BasicTour::new(self.pin(), "Orders cost two".to_owned());
        Review::open_basic(tour, &self.source(), self.state_file()).unwrap()
    }

    fn guided(&self) -> Review {
        let tour: GuidedTour = serde_json::from_value(json!({
            "pr": PR, "base": self.base, "head": self.head, "title": "T", "summary": [],
            "steps": [{"id": "a", "title": "A", "why": "w", "pieces": [{"file": "a.txt"}]}],
        }))
        .unwrap();
        Review::open(tour, &self.source(), self.state_file()).unwrap()
    }
}

#[test]
fn a_basic_review_that_asked_for_nothing_offers_write_guided_tour_and_no_banner() {
    let shop = Shop::new();
    let review = shop.basic();

    assert_eq!(review.guided_tour_progress(), &GuidedTourProgress::NotAsked);
    assert_eq!(review.write_guided_tour_refusal(), None);
    assert_eq!(review.guided_tour_banner(), None);
    assert_eq!(review.guided_tour_stage(), None);
    assert!(
        review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::WriteGuidedTour)
    );
}

#[test]
fn asking_to_write_starts_a_writing_and_a_second_ask_is_refused_with_the_reason() {
    let shop = Shop::new();
    let mut review = shop.basic();

    review.ask_to_write_guided_tour().unwrap();

    assert!(matches!(
        review.guided_tour_progress(),
        GuidedTourProgress::Writing(_)
    ));
    assert_eq!(
        review.ask_to_write_guided_tour(),
        Err(GuidedTourRefusal::AlreadyWriting)
    );
    assert_eq!(
        review.overview_page().write_guided_tour_refusal,
        Some(GuidedTourRefusal::AlreadyWriting)
    );
    assert!(review.guided_tour_stage().is_some());
    assert!(
        !review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::WriteGuidedTour)
    );
}

#[test]
fn with_the_agent_link_off_writing_is_refused_and_nothing_starts() {
    let shop = Shop::new();
    let mut review = shop.basic();
    review.tour_agent_is(&TourAgentState::LinkOff(AgentLinkOff::ServerNotRunning));

    assert_eq!(
        review.ask_to_write_guided_tour(),
        Err(GuidedTourRefusal::AgentLinkOff)
    );

    assert_eq!(review.guided_tour_progress(), &GuidedTourProgress::NotAsked);
    assert_eq!(
        review.overview_page().write_guided_tour_refusal,
        Some(GuidedTourRefusal::AgentLinkOff)
    );
}

#[test]
fn a_failed_writing_can_be_tried_again() {
    let shop = Shop::new();
    let mut review = shop.basic();
    review.guided_tour_is(GuidedTourProgress::Failed("it stopped".to_owned()));

    let stage = review.guided_tour_stage().unwrap();
    assert!(stage.can_try_again);
    assert!(stage.line.contains("it stopped"));
    review.ask_to_write_guided_tour().unwrap();
    assert!(matches!(
        review.guided_tour_progress(),
        GuidedTourProgress::Writing(_)
    ));
}

#[test]
fn a_ready_guided_tour_shows_the_banner_with_a_switch_that_is_on() {
    let shop = Shop::new();
    let mut review = shop.basic();

    review.guided_tour_is(GuidedTourProgress::Ready);

    let banner = review.guided_tour_banner().unwrap();
    assert_eq!(banner.switch_label, "Switch to guided tour");
    assert_eq!(banner.switch_refusal, None);
    assert_eq!(review.switch_to_guided_tour(), Ok(()));
    assert_eq!(
        review.ask_to_write_guided_tour(),
        Err(GuidedTourRefusal::AlreadyReady)
    );
    let actions = review.palette_actions(Build::Local);
    assert!(actions.contains(&PaletteAction::SwitchToGuidedTour));
    assert!(!actions.contains(&PaletteAction::WriteGuidedTour));
}

#[test]
fn switching_with_nothing_ready_says_so() {
    let shop = Shop::new();
    let review = shop.basic();

    assert_eq!(
        review.switch_to_guided_tour(),
        Err(GuidedTourRefusal::NothingReady)
    );
}

#[test]
fn the_guided_tour_turning_ready_moves_nothing_on_the_screen() {
    let shop = Shop::new();
    let mut review = shop.basic();
    review.go_to_file(&crate::support::file_path("a.txt"));
    let before = review.screen().clone();

    review.guided_tour_is(GuidedTourProgress::Ready);

    assert_eq!(review.screen(), &before);
    assert!(matches!(review.screen(), Screen::File(_)));
}

#[test]
fn a_guided_review_offers_nothing_and_never_shows_the_banner() {
    let shop = Shop::new();
    let mut review = shop.guided();
    review.guided_tour_is(GuidedTourProgress::Ready);

    assert_eq!(review.guided_tour_banner(), None);
    assert_eq!(
        review.ask_to_write_guided_tour(),
        Err(GuidedTourRefusal::NotBasic)
    );
    let actions = review.palette_actions(Build::Local);
    assert!(!actions.contains(&PaletteAction::WriteGuidedTour));
    assert!(!actions.contains(&PaletteAction::SwitchToGuidedTour));
}

#[test]
fn the_palette_and_the_overview_start_the_writing_through_the_same_intent() {
    let shop = Shop::new();
    let review = shop.basic();

    let effect = review.decide(
        PaletteAction::WriteGuidedTour.into(),
        WindowMoment::default(),
    );

    assert_eq!(effect, Effect::WriteGuidedTour);
    assert_eq!(
        Intent::from(PaletteAction::SwitchToGuidedTour),
        Intent::SwitchToGuidedTour
    );
    assert_eq!(PaletteAction::WriteGuidedTour.label(), "Write guided tour");
    assert_eq!(
        PaletteAction::SwitchToGuidedTour.label(),
        "Switch to guided tour"
    );
}
