#[allow(dead_code)]
mod support;

use std::path::Path;

use guided_review::config::Config;
use guided_review::ide::{Ide, IdeLaunch};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{CodeOpener, Destination, Review, ReviewStateFile, Side};
use serde_json::json;
use support::FixtureRepo;

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    fn with_pull() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("app/Order.php", "<?php\nreturn 1;\n");
        repo.write("data.bin", "");
        repo.write("old.txt", "gone\n");
        let base = repo.commit("base");
        repo.write("app/Order.php", "<?php\nreturn 2;\n");
        std::fs::write(repo.path().join("data.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
        std::fs::remove_file(repo.path().join("old.txt")).unwrap();
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": "https://github.com/acme/shop/pull/7",
            "base": self.base,
            "head": self.head,
            "title": "Orders cost two",
            "summary": ["Order total goes up"],
            "steps": [{"id": "all", "title": "All", "why": "because", "pieces": [
                {"file": "app/Order.php"}, {"file": "data.bin"}, {"file": "old.txt"}
            ]}],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_pr(
            self.repo.home(),
            &"https://github.com/acme/shop/pull/7"
                .parse::<PullRequestUrl>()
                .unwrap(),
        );
        let mut review = Review::open(tour, &source, state_file).unwrap();
        review.go_forward();
        review
    }

    fn ide(&self) -> Ide {
        let config = Config::from_text("ide = \"idea://open?file={path}&line={line}\"").unwrap();
        Ide::for_clone(Some(self.repo.path().to_path_buf()), config.ide_template()).unwrap()
    }

    fn in_ide(&self, path: &str, line: usize) -> Destination {
        Destination::Ide(self.ide().launch(Path::new(path), line))
    }

    fn blob(&self, sha: &str, rest: &str) -> Destination {
        Destination::Browser(format!("https://github.com/acme/shop/blob/{sha}/{rest}"))
    }
}

fn line_destination(review: &Review, opener: &CodeOpener, side: Side) -> Destination {
    let page = review.current_step().unwrap();
    let line = page
        .pieces
        .iter()
        .flat_map(|piece| piece.lines())
        .find(|line| line.location.path == "app/Order.php" && line.location.side == side)
        .unwrap();
    opener.destination(&line.code_link)
}

#[test]
fn a_head_line_opens_in_the_ide_when_the_code_is_in_a_local_clone() {
    let shop = Shop::with_pull();
    let review = shop.open();

    let opener = review.code_opener(Some(shop.ide()));

    assert_eq!(
        line_destination(&review, &opener, Side::Head),
        shop.in_ide("app/Order.php", 2)
    );
}

#[test]
fn a_removed_line_opens_on_github_at_base_because_the_working_copy_does_not_have_it() {
    let shop = Shop::with_pull();
    let review = shop.open();

    let opener = review.code_opener(Some(shop.ide()));

    assert_eq!(
        line_destination(&review, &opener, Side::Base),
        shop.blob(&shop.base, "app/Order.php#L2")
    );
}

#[test]
fn without_an_ide_every_line_opens_on_github_at_the_tours_commits() {
    let shop = Shop::with_pull();
    let review = shop.open();

    let opener = review.code_opener(None);

    assert_eq!(
        line_destination(&review, &opener, Side::Head),
        shop.blob(&shop.head, "app/Order.php#L2")
    );
}

#[test]
fn a_file_the_working_copy_does_not_hold_opens_on_github() {
    let shop = Shop::with_pull();
    let review = shop.open();
    std::fs::remove_file(shop.repo.path().join("app/Order.php")).unwrap();

    let opener = review.code_opener(Some(shop.ide()));

    assert_eq!(
        line_destination(&review, &opener, Side::Head),
        shop.blob(&shop.head, "app/Order.php#L2")
    );
}

#[test]
fn a_piece_opens_at_its_first_head_line_and_a_deleted_file_on_github_at_base() {
    let shop = Shop::with_pull();
    let review = shop.open();
    let opener = review.code_opener(Some(shop.ide()));
    let page = review.current_step().unwrap();

    let open = |path: &str| {
        let piece = page
            .pieces
            .iter()
            .find(|piece| piece.id.path == path)
            .unwrap();
        opener.destination(piece.code_link().unwrap())
    };

    assert_eq!(open("app/Order.php"), shop.in_ide("app/Order.php", 2));
    assert_eq!(open("old.txt"), shop.blob(&shop.base, "old.txt#L1"));
}

#[test]
fn a_binary_file_opens_in_the_ide_too() {
    let shop = Shop::with_pull();
    let review = shop.open();
    let opener = review.code_opener(Some(shop.ide()));
    let page = review.current_step().unwrap();

    let binary = page
        .pieces
        .iter()
        .find_map(|piece| piece.binary_file.as_ref())
        .unwrap();

    assert_eq!(opener.destination(binary), shop.in_ide("data.bin", 1));
}

#[test]
fn a_github_link_to_code_of_this_repo_in_markdown_opens_in_the_ide() {
    let shop = Shop::with_pull();
    let opener = shop.open().code_opener(Some(shop.ide()));

    assert_eq!(
        opener.destination_of_url("https://github.com/acme/shop/blob/main/app/Order.php#L2-L5"),
        shop.in_ide("app/Order.php", 2)
    );
    assert_eq!(
        opener.destination_of_url("https://github.com/Acme/Shop/blob/abc123/app/Order.php"),
        shop.in_ide("app/Order.php", 1)
    );
}

#[test]
fn a_repo_relative_path_in_markdown_is_a_code_link() {
    let shop = Shop::with_pull();
    let review = shop.open();

    assert_eq!(
        review
            .code_opener(Some(shop.ide()))
            .destination_of_url("app/Order.php#L2"),
        shop.in_ide("app/Order.php", 2)
    );
    assert_eq!(
        review
            .code_opener(None)
            .destination_of_url("./app/Order.php#L2"),
        shop.blob(&shop.head, "app/Order.php#L2")
    );
}

#[test]
fn other_links_in_markdown_open_in_the_browser_as_written() {
    let shop = Shop::with_pull();
    let opener = shop.open().code_opener(Some(shop.ide()));

    for url in [
        "https://github.com/other/repo/blob/main/app/Order.php#L2",
        "https://github.com/acme/shop/pull/7",
        "https://github.com/acme/shop/blob/main/missing.php#L1",
        "https://example.com/docs",
        "mailto:team@example.com",
    ] {
        assert_eq!(
            opener.destination_of_url(url),
            Destination::Browser(url.into()),
            "{url}"
        );
    }
}

#[test]
fn a_shell_ide_launch_and_a_url_launch_are_both_ide_destinations() {
    let shop = Shop::with_pull();
    let config = Config::from_text("ide = \"phpstorm --line {line} {path}\"").unwrap();
    let ide = Ide::for_clone(Some(shop.repo.path().to_path_buf()), config.ide_template()).unwrap();
    let opener = shop.open().code_opener(Some(ide));

    assert!(matches!(
        opener.destination_of_url("app/Order.php#L2"),
        Destination::Ide(IdeLaunch::Shell(_))
    ));
}
