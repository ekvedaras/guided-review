use std::time::Instant;

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeGitHub, SentLog};
use crate::support::work::drive_without_agents;
use guided_review::issue::{IssueLink, IssueUrl};
use guided_review::local_git::LocalGit;
use guided_review::local_work_tree::LocalWorkTree;
use guided_review::open_review::open_review;
use guided_review::pull_request::RepoName;
use guided_review::review_subject::{BranchName, ReviewSubject};
use guided_review::tour::GuidedTour;
use guided_review::update::Build;
use guided_review::viewer::{
    Command, Effect, Intent, PaletteAction, PullRequestIssues, RelatedIssue, Review,
    ReviewStateFile, WindowMoment,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn issue(number: u64) -> IssueUrl {
    format!("https://github.com/acme/shop/issues/{number}")
        .parse()
        .unwrap()
}

fn linked(number: u64, title: &str) -> RelatedIssue {
    RelatedIssue {
        link: IssueLink::GitHub(issue(number)),
        title: Some(title.into()),
    }
}

fn numbers(review: &Review) -> Vec<u64> {
    review
        .related_issues()
        .iter()
        .map(|related| match &related.link {
            IssueLink::GitHub(url) => url.number(),
            IssueLink::Ticket(_) => 0,
        })
        .collect()
}

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("a.txt", "one\n");
        let base = repo.commit("base");
        repo.write("a.txt", "two\n");
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open_with(&self, extra: serde_json::Value) -> Review {
        let mut tour = json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Letters",
            "summary": [],
            "steps": [{"id": "a", "title": "A", "why": "w", "pieces": [{"file": "a.txt"}]}],
        });
        tour.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let tour: GuidedTour = serde_json::from_value(tour).unwrap();
        let state_file = ReviewStateFile::for_subject(&self.repo.folders(), &PR.parse().unwrap());
        Review::open(tour, &self.source(), state_file).unwrap()
    }

    fn github(&self, issues: PullRequestIssues) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent).with_issues(issues)
    }

    fn read(&self, review: &mut Review, github: &FakeGitHub) {
        review.pull_from_github();
        drive_without_agents(review, github, &self.source(), Instant::now());
    }
}

fn branch_issue(branch: &str) -> Option<u64> {
    IssueUrl::of_branch(
        &RepoName::new("acme", "shop"),
        &BranchName::new(branch).unwrap(),
    )
    .map(|url| url.number())
}

#[test]
fn a_branch_names_an_issue_only_by_a_number_that_starts_its_last_part_or_follows_issue_or_gh() {
    for (branch, expected) in [
        ("123-fix-totals", Some(123)),
        ("feat/123-fix-totals", Some(123)),
        ("ekvedaras/45", Some(45)),
        ("issue-45", Some(45)),
        ("feature/issues-45-totals", Some(45)),
        ("gh-7", Some(7)),
        ("GH_7_totals", Some(7)),
        ("main", None),
        ("release-2026", None),
        ("2026-11-18-notes", None),
        ("v2", None),
        ("feature/add-oauth2", None),
        ("0-nothing", None),
    ] {
        assert_eq!(branch_issue(branch), expected, "{branch}");
    }
}

fn part_of(text: &str) -> Vec<String> {
    IssueLink::named_in(text, &RepoName::new("acme", "shop"))
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_description_names_the_issues_it_is_part_of_by_number_repo_and_number_or_link() {
    assert_eq!(
        part_of("Part of #12"),
        ["https://github.com/acme/shop/issues/12"]
    );
    assert_eq!(
        part_of("Ticket: x\n\n**Part of** acme/stock#4.\nMore text #99"),
        ["https://github.com/acme/stock/issues/4"]
    );
    assert_eq!(
        part_of("part of https://github.com/acme/shop/issues/8, #9 and #10"),
        [
            "https://github.com/acme/shop/issues/8",
            "https://github.com/acme/shop/issues/9",
            "https://github.com/acme/shop/issues/10",
        ]
    );
    assert_eq!(part_of("Part of the plan, see #12"), Vec::<String>::new());
    assert_eq!(part_of("Fixes a bug in #12"), Vec::<String>::new());
    assert_eq!(part_of("Suffix #12, prefix: #13"), Vec::<String>::new());
}

#[test]
fn a_description_names_the_issues_githubs_closing_keywords_name_in_the_order_it_names_them() {
    for keyword in [
        "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
    ] {
        assert_eq!(
            part_of(&format!(
                "{keyword} #3\n{} acme/stock#4",
                keyword.to_uppercase()
            )),
            [
                "https://github.com/acme/shop/issues/3",
                "https://github.com/acme/stock/issues/4",
            ],
            "{keyword}"
        );
    }
    assert_eq!(
        part_of("Closes: #5"),
        ["https://github.com/acme/shop/issues/5"]
    );
    assert_eq!(
        part_of("Fixes #7, part of #6 and fixes #7"),
        [
            "https://github.com/acme/shop/issues/7",
            "https://github.com/acme/shop/issues/6",
        ]
    );
}

#[test]
fn a_ticket_line_names_its_link_as_a_ticket() {
    assert_eq!(
        part_of("Ticket: https://tickets.example/t?selected=5631\n\n## TL;DR"),
        ["https://tickets.example/t?selected=5631"]
    );
    assert_eq!(
        part_of("**Ticket:** [5631](https://tickets.example/5631)"),
        ["https://tickets.example/5631"]
    );
    assert_eq!(part_of("Ticket: none yet"), Vec::<String>::new());
    let ticket: IssueLink = "https://tickets.example/5631".parse().unwrap();
    assert_eq!(ticket.label(), "Ticket");
    assert_eq!(IssueLink::GitHub(issue(12)).label(), "#12");
    assert!("not a link".parse::<IssueLink>().is_err());
}

#[test]
fn a_tours_issue_may_be_a_ticket() {
    let shop = Shop::new();
    let review = shop.open_with(json!({"issue": "https://tickets.example/5631"}));

    assert_eq!(
        review.decide(Intent::OpenIssue, WindowMoment::default()),
        Effect::OpenIssue("https://tickets.example/5631".parse().unwrap())
    );
}

#[test]
fn with_no_linked_issue_the_issues_the_description_is_part_of_come_before_the_tours() {
    let shop = Shop::new();
    let mut review = shop.open_with(json!({"issue": issue(3).to_string()}));

    review.set_pr_description("Part of #16 and #17").unwrap();

    assert_eq!(numbers(&review), [16, 17]);
    let reopened = shop.open_with(json!({}));
    assert_eq!(
        numbers(&reopened),
        [16, 17],
        "the saved description counts at open"
    );
}

#[test]
fn an_issue_url_reads_and_writes_as_its_github_page() {
    let url: IssueUrl = "https://github.com/acme/shop/issues/12".parse().unwrap();

    assert_eq!(url.number(), 12);
    assert_eq!(url.to_string(), "https://github.com/acme/shop/issues/12");
    assert!(
        "https://github.com/acme/shop/pull/12"
            .parse::<IssueUrl>()
            .is_err()
    );
    assert!("#12".parse::<IssueUrl>().is_err());
}

#[test]
fn a_pr_shows_the_issues_github_links_to_it_before_the_tours_and_the_branchs() {
    let shop = Shop::new();
    let mut review = shop.open_with(json!({"issue": issue(3).to_string()}));
    let github = shop.github(PullRequestIssues {
        linked: vec![linked(12, "Totals are wrong"), linked(13, "Totals round")],
        head_branch: Some(BranchName::new("44-totals").unwrap()),
    });

    shop.read(&mut review, &github);

    assert_eq!(numbers(&review), [12, 13]);
    assert_eq!(
        review.related_issues().first().unwrap().title.as_deref(),
        Some("Totals are wrong")
    );
}

#[test]
fn with_no_linked_issue_the_guided_tours_issue_comes_before_the_branchs() {
    let shop = Shop::new();
    let mut review = shop.open_with(json!({"issue": issue(3).to_string()}));
    let github = shop.github(PullRequestIssues {
        linked: vec![],
        head_branch: Some(BranchName::new("44-totals").unwrap()),
    });

    assert_eq!(numbers(&review), [3]);
    shop.read(&mut review, &github);

    assert_eq!(numbers(&review), [3]);
}

#[test]
fn with_no_linked_issue_and_no_tour_issue_the_prs_head_branch_names_it() {
    let shop = Shop::new();
    let mut review = shop.open_with(json!({}));
    let github = shop.github(PullRequestIssues {
        linked: vec![],
        head_branch: Some(BranchName::new("feat/44-totals").unwrap()),
    });

    assert_eq!(numbers(&review), Vec::<u64>::new());
    shop.read(&mut review, &github);

    assert_eq!(numbers(&review), [44]);
}

#[test]
fn a_reopened_review_keeps_what_github_linked() {
    let shop = Shop::new();
    let mut replaced = shop.open_with(json!({}));
    shop.read(
        &mut replaced,
        &shop.github(PullRequestIssues {
            linked: vec![linked(12, "Totals are wrong")],
            head_branch: None,
        }),
    );

    let mut reopened = shop.open_with(json!({}));
    reopened.carry_on_from(replaced);

    assert_eq!(numbers(&reopened), [12]);
}

#[test]
fn a_local_review_takes_its_issue_from_its_branch() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feat/21-greeting"]);
    repo.write("README.md", "hello\nworld\n");
    repo.commit("Greet the world");
    let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
    let subject = ReviewSubject::from(work_tree.change_set().unwrap());

    let review = open_review(&subject, repo.path(), &repo.folders())
        .unwrap()
        .review;

    assert_eq!(numbers(&review), [21]);
}

#[test]
fn the_issue_key_opens_the_one_issue_lists_several_and_does_nothing_with_none() {
    let shop = Shop::new();
    let none = shop.open_with(json!({}));
    let one = shop.open_with(json!({"issue": issue(3).to_string()}));
    let mut several = shop.open_with(json!({}));
    shop.read(
        &mut several,
        &shop.github(PullRequestIssues {
            linked: vec![linked(12, "a"), linked(13, "b")],
            head_branch: None,
        }),
    );
    let moment = WindowMoment::default();

    assert_eq!(none.decide(Intent::OpenIssue, moment), Effect::Nothing);
    assert_eq!(
        one.decide(Intent::OpenIssue, moment),
        Effect::OpenIssue(IssueLink::GitHub(issue(3)))
    );
    assert_eq!(
        several.decide(Intent::OpenIssue, moment),
        Effect::ListIssues
    );
    assert_eq!(
        several.decide(Intent::OpenIssueAt(1), moment),
        Effect::OpenIssue(IssueLink::GitHub(issue(13)))
    );
    assert_eq!(
        several.decide(Intent::OpenIssueAt(2), moment),
        Effect::Nothing
    );
}

#[test]
fn the_palette_offers_the_issue_only_when_there_is_one_and_i_is_its_key() {
    let shop = Shop::new();
    let offers = |review: &Review| {
        review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::OpenIssue)
    };

    assert!(!offers(&shop.open_with(json!({}))));
    assert!(offers(
        &shop.open_with(json!({"issue": issue(3).to_string()}))
    ));
    assert_eq!(PaletteAction::OpenIssue.command(), Some(Command::OpenIssue));
    assert_eq!(
        Command::OpenIssue
            .default_chords()
            .iter()
            .map(|chord| chord.keystroke())
            .collect::<Vec<_>>(),
        ["i"]
    );
}
