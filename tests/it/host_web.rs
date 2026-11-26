//! A PR's code host on the web: its links read back, its pages, and links of hosts the app does not read.

use guided_review::file_path::FilePath;
use guided_review::line_number::LineNumber;
use guided_review::pull_request::{HostSite, PullRequestUrl, RepoName, Sha};

fn pr() -> PullRequestUrl {
    "https://github.com/acme/shop/pull/7".parse().unwrap()
}

#[test]
fn a_github_pr_link_reads_as_the_pr_on_github_and_writes_back_the_same() {
    let pr = pr();

    assert_eq!(pr.host, HostSite::GitHub);
    assert_eq!(pr.repo, RepoName::new("acme", "shop"));
    assert_eq!(pr.number, 7);
    assert_eq!(pr.to_string(), "https://github.com/acme/shop/pull/7");
}

#[test]
fn a_pr_link_of_a_host_the_app_does_not_read_is_refused_naming_the_hosts_it_reads() {
    let link = "https://gitlab.com/acme/shop/-/merge_requests/7";

    let refused = link.parse::<PullRequestUrl>().unwrap_err();

    assert_eq!(
        refused.to_string(),
        format!("not a PR link of a code host guided-review reads (GitHub): {link}")
    );
}

#[test]
fn the_checks_page_of_a_github_pr_is_its_checks_tab() {
    assert_eq!(
        pr().checks_page(),
        "https://github.com/acme/shop/pull/7/checks"
    );
}

#[test]
fn a_file_page_on_github_reads_back_as_the_same_file_and_line() {
    let path = FilePath::new("app/my shop/Café.php").unwrap();
    let line = LineNumber::try_from(12_usize).unwrap();

    let page = HostSite::GitHub.file_page(&pr().repo, &Sha::new("abc123"), &path, Some(line));

    assert_eq!(
        page,
        "https://github.com/acme/shop/blob/abc123/app/my%20shop/Caf%C3%A9.php#L12"
    );
    assert_eq!(
        HostSite::GitHub.file_of_link(&pr().repo, &page),
        Some((path, line))
    );
}
