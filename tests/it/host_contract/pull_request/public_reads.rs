use guided_review::pull_request::Sha;
use guided_review::review_subject::BranchName;

use crate::host_contract::{HostBuilder, PullOnHost, Reach, SeedCode, pr};
use crate::support::file_path;

/// A public repo's PR and code read with no token, as anyone may read them on the host's web page.
pub fn a_public_repos_pr_and_code_read_without_a_token(host: HostBuilder) {
    let pull = PullOnHost {
        reach: Reach::NoToken,
        code: SeedCode {
            files: vec![("bbb", "cart.rs", "pub struct Cart;\n")],
            ..SeedCode::default()
        },
        ..PullOnHost::default()
    };
    let host = host(pull.clone());
    let cart = BranchName::new(pull.head_branch).unwrap();

    let tips = host.pull_request_tips(&pr()).unwrap();
    let title = host.pull_request_title(&pr()).unwrap();
    let description = host.pull_request_description(&pr()).unwrap();
    let head_branch = host.pull_request_head_branch(&pr()).unwrap();
    let pull_of_branch = host
        .open_pull_of_branch(&pr().hosted_repo(), &cart)
        .unwrap();
    let code = host
        .code_of(&pr().repo)
        .unwrap()
        .file_at(&Sha::new("bbb"), &file_path("cart.rs"))
        .unwrap();

    assert_eq!(
        (tips.base_branch_tip, tips.head),
        (Sha::new(pull.base_branch_tip), Sha::new(pull.head))
    );
    assert_eq!(
        (title.as_str(), description.as_str()),
        (pull.title, pull.description)
    );
    assert_eq!(head_branch, Some(cart));
    assert_eq!(pull_of_branch.map(|found| found.pr), Some(pr()));
    assert_eq!(code, "pub struct Cart;\n");
}
