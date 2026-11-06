use crate::support::FixtureRepo;
use crate::support::fake_tour_agent::FakeTourAgent;
use guided_review::pull_request::PullRequestUrl;
use guided_review::viewer::{
    ActivePr, AgentStatus, AppStateFile, OpenOrigin, TerminalId, TourFact, WriteDecision,
    WritingStage,
};

const SHOWN: &str = "https://github.com/acme/shop/pull/7";
const REPLIED: &str = "https://github.com/acme/shop/pull/8";
const WRITTEN: &str = "https://github.com/acme/shop/pull/9";

fn pr(url: &str) -> PullRequestUrl {
    url.parse().unwrap()
}

fn restarted(repo: &FixtureRepo) -> ActivePr {
    ActivePr::remembering_in(AppStateFile::in_home(repo.home()))
}

#[test]
fn an_agent_reply_not_seen_before_a_restart_is_still_unseen_after_it() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut before = restarted(&repo);
    before.agent_replied(&pr(REPLIED));
    assert!(before.has_unseen_reply(&pr(REPLIED)));

    let after = restarted(&repo);

    assert!(after.has_unseen_reply(&pr(REPLIED)));
    assert!(!after.has_unseen_reply(&pr(SHOWN)));
}

fn writing_before_a_restart(repo: &FixtureRepo) {
    let mut before = restarted(repo);
    assert_eq!(before.write(pr(WRITTEN)), WriteDecision::StartWriter);
    before.writer_started(&pr(WRITTEN), Ok(TerminalId::new("t1")));
}

#[test]
fn a_tour_being_written_before_a_restart_is_followed_again_in_its_terminal() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    writing_before_a_restart(&repo);
    let herdr = FakeTourAgent::new().with_terminal("t1", Some(WRITTEN), AgentStatus::Working);

    let mut after = restarted(&repo);
    let to_follow = after.resume_writings(&herdr);

    assert_eq!(to_follow, vec![(pr(WRITTEN), TerminalId::new("t1"))]);
    let writing = after.writing_of(&pr(WRITTEN)).unwrap();
    assert_eq!(writing.stage, WritingStage::Writing);
    assert_eq!(writing.terminal, Some(TerminalId::new("t1")));
    assert_eq!(after.tour_fact_of(&pr(WRITTEN)), Some(TourFact::Writing));
}

#[test]
fn a_tour_being_written_before_a_restart_fails_with_a_reason_when_its_terminal_is_gone() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    writing_before_a_restart(&repo);
    let herdr = FakeTourAgent::new();

    let mut after = restarted(&repo);
    let to_follow = after.resume_writings(&herdr);

    assert_eq!(to_follow.len(), 0);
    let WritingStage::Failed(reason) = &after.writing_of(&pr(WRITTEN)).unwrap().stage else {
        panic!("a writing with no terminal has failed");
    };
    assert!(reason.contains("terminal"), "{reason}");
    assert_eq!(
        after.tour_fact_of(&pr(WRITTEN)),
        Some(TourFact::WritingFailed)
    );
}

#[test]
fn a_writing_whose_tour_was_opened_before_a_restart_is_not_brought_back() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    writing_before_a_restart(&repo);
    let mut before = restarted(&repo);
    before.resume_writings(&FakeTourAgent::new());
    before.request(pr(WRITTEN), OpenOrigin::AppStartedAgent);

    let mut after = restarted(&repo);

    assert_eq!(after.resume_writings(&FakeTourAgent::new()).len(), 0);
    assert!(after.writing_of(&pr(WRITTEN)).is_none());
}
