use crate::support::graphql::serve_graphql;
use guided_review::github::GitHub;
use guided_review::viewer::{GitHubError, RelevantPullRequestsPort, WhyListed};

const TEAMS: &str = include_str!("../fixtures/github/viewer_teams.json");
const RELEVANT: &str = include_str!("../fixtures/github/relevant_pull_requests.json");

fn searches_asked(asked: &str) -> Vec<String> {
    let body: serde_json::Value = serde_json::from_str(asked).unwrap();
    let variables = body.pointer("/variables").unwrap().as_object().unwrap();
    let mut names: Vec<_> = variables.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| variables[name].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn same_numbered_pull_requests_of_two_repos_both_show_newest_update_first() {
    let (url, _) = serve_graphql(vec![TEAMS.into(), RELEVANT.into()]);

    let relevant = GitHub::at(&url).relevant_pull_requests().unwrap();

    let listed: Vec<_> = relevant
        .pull_requests
        .iter()
        .map(|pr| pr.url.to_string())
        .collect();
    assert_eq!(
        listed,
        [
            "https://github.com/acme/blog/pull/7",
            "https://github.com/acme/api/pull/31",
            "https://github.com/acme/shop/pull/7",
            "https://github.com/acme/shop/pull/12",
        ]
    );
}

#[test]
fn a_row_says_who_wrote_it_whether_it_is_a_draft_and_why_it_is_listed() {
    let (url, _) = serve_graphql(vec![TEAMS.into(), RELEVANT.into()]);

    let relevant = GitHub::at(&url).relevant_pull_requests().unwrap();

    let [blog, api, shop, old] = relevant.pull_requests.as_slice() else {
        panic!("expected four, got {relevant:?}");
    };
    assert_eq!(
        (blog.title.as_str(), blog.author.as_str(), blog.is_draft),
        ("Fix feed", "ekvedaras", true)
    );
    assert_eq!(blog.why_listed, WhyListed::Mine);
    assert_eq!(blog.updated_at, "2026-11-06T07:00:00Z");
    assert_eq!(
        api.why_listed,
        WhyListed::ReviewRequested,
        "my team is asked"
    );
    assert_eq!(api.author, "ghost", "GitHub shows a deleted author as null");
    assert_eq!(shop.why_listed, WhyListed::ReviewRequested, "I am asked");
    assert!(!shop.is_draft);
    assert_eq!(old.why_listed, WhyListed::Involved);
}

#[test]
fn one_call_has_a_search_for_involvement_and_one_per_team_without_a_date_limit() {
    let (url, asked) = serve_graphql(vec![TEAMS.into(), RELEVANT.into()]);

    GitHub::at(&url).relevant_pull_requests().unwrap();

    let asked = asked.lock().unwrap();
    assert_eq!(
        searches_asked(&asked[1]),
        [
            "involves:@me is:open is:pr archived:false",
            "team-review-requested:acme/platform is:open is:pr archived:false",
            "team-review-requested:acme/reviewers is:open is:pr archived:false",
        ]
    );
    assert!(asked[1].contains("first: 100"), "{}", asked[1]);
}

#[test]
fn teams_are_read_once_per_app_run() {
    let (url, asked) = serve_graphql(vec![TEAMS.into(), RELEVANT.into(), RELEVANT.into()]);
    let github = GitHub::at(&url);

    github.relevant_pull_requests().unwrap();
    github.relevant_pull_requests().unwrap();

    assert_eq!(asked.lock().unwrap().len(), 3);
}

#[test]
fn at_most_ten_teams_are_searched_and_the_rest_are_named_as_left_out() {
    let slugs: Vec<String> = (1..=12).map(|n| format!("acme/team{n}")).collect();
    let nodes: Vec<String> = slugs
        .iter()
        .map(|slug| format!(r#"{{"combinedSlug":"{slug}"}}"#))
        .collect();
    let teams = format!(
        r#"{{"data":{{"viewer":{{"login":"me","organizations":{{"nodes":[{{"teams":{{"nodes":[{}]}}}}]}}}}}}}}"#,
        nodes.join(",")
    );
    let (url, asked) = serve_graphql(vec![teams, r#"{"data":{"viewer":{"login":"me"}}}"#.into()]);

    let relevant = GitHub::at(&url).relevant_pull_requests().unwrap();

    assert_eq!(searches_asked(&asked.lock().unwrap()[1]).len(), 11);
    assert_eq!(relevant.teams_left_out, ["acme/team11", "acme/team12"]);
}

#[test]
fn without_a_token_the_list_is_refused_with_the_reason() {
    let error = GitHub::without_token_at("http://127.0.0.1:1")
        .relevant_pull_requests()
        .unwrap_err();

    assert!(matches!(error, GitHubError::NoAccess(_)), "{error:?}");
}
