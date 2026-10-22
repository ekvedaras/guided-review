#[allow(dead_code)]
mod support;

use std::path::PathBuf;
use std::sync::Arc;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::repin::RepinStatus;
use guided_review::tour::Tour;
use guided_review::viewer::{
    Review, ReviewStateFile, Side, StepId, TourRepinner, ViewedState, repin_tour,
};
use serde_json::json;
use support::FixtureRepo;
use support::app_path::AppPath;
use support::fake_ports::{FakeGitHub, SentLog};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn order_php(total: u32, tax: u32, keep: u32) -> String {
    format!(
        "<?php\n\nclass Order\n{{\n    public function total(): int\n    {{\n        return {total};\n    }}\n\n    public function tax(): int\n    {{\n        return {tax};\n    }}\n\n    public function keep(): int\n    {{\n        return {keep};\n    }}\n}}\n"
    )
}

fn yaml(lines: &[(usize, &str)]) -> String {
    (1..=30)
        .map(|number| {
            let text = lines
                .iter()
                .find(|(at, _)| *at == number)
                .map_or("same", |(_, text)| text);
            format!("key{number}: {text}\n")
        })
        .collect()
}

const PRICING_PHP: &str = "<?php\n\nclass Pricing\n{\n    public function discount(): int\n    {\n        return 0;\n    }\n}\n";

/// A tour pinned to the first head. The second head changes `total`, takes `tax` back to what
/// base had (so its piece is gone), keeps `keep`, adds a hunk to config.yaml and a new class.
struct Shop {
    repo: FixtureRepo,
    base: String,
    first_head: String,
}

impl Shop {
    fn with_tour_at_first_head() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("app/Order.php", &order_php(1, 1, 1));
        repo.write("config.yaml", &yaml(&[]));
        repo.write("readme.txt", "old\n");
        repo.write("old.txt", "old\n");
        let base = repo.commit("base");
        repo.write("app/Order.php", &order_php(2, 5, 9));
        repo.write("config.yaml", &yaml(&[(15, "first")]));
        repo.write("readme.txt", "new\n");
        repo.write("old.txt", "new\n");
        let first_head = repo.commit("first head");
        let shop = Self {
            repo,
            base,
            first_head,
        };
        let tour = json!({
            "pr": PR,
            "base": shop.base,
            "head": shop.first_head,
            "title": "Orders",
            "steps": [
                {"id": "total", "title": "Total", "pieces": [{"file": "app/Order.php", "symbol": "Order::total"}]},
                {"id": "tax", "title": "Tax", "pieces": [{"file": "app/Order.php", "symbol": "Order::tax"}]},
                {"id": "keep", "title": "Keep", "pieces": [{"file": "app/Order.php", "symbol": "Order::keep"}]},
                {"id": "config", "title": "Config", "pieces": [{"file": "config.yaml"}]},
                {"id": "readme", "title": "Readme", "pieces": [{"file": "readme.txt"}]},
                {"id": "old", "title": "Old", "pieces": [{"file": "old.txt"}]},
            ],
        });
        let path = shop.tour_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
        shop
    }

    fn commit_second_head(&self) -> String {
        self.repo.write("app/Order.php", &order_php(3, 1, 9));
        self.repo
            .write("config.yaml", &yaml(&[(3, "added above"), (15, "first")]));
        self.repo.write("app/Pricing.php", PRICING_PHP);
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

    fn tick(&self, steps: &[&str]) {
        let mut review = self.open();
        for step in steps {
            review.tick(&(*step).into()).unwrap();
        }
    }

    fn repin_onto(&self, head: &str) {
        repin_tour(
            &self.tour_path(),
            &self.state_file(),
            &self.source(),
            &Sha::new(&self.base),
            &Sha::new(head),
        )
        .unwrap();
    }
}

fn ticked(review: &Review) -> Vec<String> {
    review
        .sidebar()
        .iter()
        .filter(|entry| entry.is_ticked)
        .map(|entry| entry.id.to_string())
        .collect()
}

#[test]
fn a_step_with_a_changed_piece_loses_its_tick_and_a_step_with_same_pieces_keeps_it() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["total", "keep"]);
    let second_head = shop.commit_second_head();

    shop.repin_onto(&second_head);

    assert_eq!(ticked(&shop.open()), ["keep"]);
}

#[test]
fn a_step_whose_whole_file_pointer_gained_a_new_piece_loses_its_tick() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["config", "readme"]);
    let second_head = shop.commit_second_head();

    shop.repin_onto(&second_head);

    assert_eq!(ticked(&shop.open()), ["readme"]);
}

fn state_on_disk(shop: &Shop) -> serde_json::Value {
    let path = shop
        .repo
        .home()
        .join(".guided-review/state/acme/shop/7.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn re_pin_stores_what_became_of_each_piece_and_where_it_started() {
    let shop = Shop::with_tour_at_first_head();
    let second_head = shop.commit_second_head();

    shop.repin_onto(&second_head);

    let record = &state_on_disk(&shop)["last_repin"];
    assert_eq!(record["from_head"], shop.first_head.as_str());
    assert_eq!(record["from_base"], shop.base.as_str());
    assert_eq!(record["to_head"], second_head.as_str());
    assert_eq!(record["pieces"]["app/Order.php#Order::total"], "changed");
    assert_eq!(record["pieces"]["app/Order.php#Order::keep"], "same");
    assert_eq!(record["pieces"]["app/Pricing.php#Pricing"], "new");
    assert_eq!(
        record["gone_by_step"],
        json!({"tax": ["app/Order.php#Order::tax"]})
    );
}

fn page_of(review: &mut Review, step: &str) -> (usize, bool, bool, bool) {
    review.go_to(&step.into());
    let page = review.current_step().unwrap();
    (
        page.gone_pieces,
        page.is_gone,
        page.is_folded,
        page.text_may_be_old,
    )
}

#[test]
fn a_step_left_with_only_gone_pieces_is_gone_folded_and_greyed_in_the_sidebar() {
    let shop = Shop::with_tour_at_first_head();
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);

    let mut review = shop.open();

    let (gone_pieces, is_gone, is_folded, _) = page_of(&mut review, "tax");
    assert_eq!((gone_pieces, is_gone, is_folded), (1, true, true));
    let (gone_pieces, is_gone, is_folded, _) = page_of(&mut review, "total");
    assert_eq!((gone_pieces, is_gone, is_folded), (0, false, false));
    let gone: Vec<String> = review
        .sidebar()
        .iter()
        .filter(|entry| entry.is_gone)
        .map(|entry| entry.id.to_string())
        .collect();
    assert_eq!(gone, ["tax"]);
}

#[test]
fn a_new_piece_no_step_shows_lands_in_the_safety_net_marked_new() {
    let shop = Shop::with_tour_at_first_head();
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);

    let review = shop.open();

    let safety_net = review.safety_net().unwrap();
    let pricing = safety_net
        .pieces
        .iter()
        .find(|piece| piece.id.to_string() == "app/Pricing.php#Pricing::discount")
        .unwrap();
    assert_eq!(pricing.repin_status, Some(RepinStatus::New));
}

#[test]
fn shown_pieces_say_whether_they_changed_and_steps_with_changed_pieces_say_text_may_be_old() {
    let shop = Shop::with_tour_at_first_head();
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);

    let mut review = shop.open();

    assert!(page_of(&mut review, "total").3);
    assert!(!page_of(&mut review, "keep").3);
    review.go_to(&"total".into());
    let page = review.current_step().unwrap();
    assert_eq!(page.pieces[0].repin_status, Some(RepinStatus::Changed));
}

#[test]
fn without_a_re_pin_pieces_carry_no_status() {
    let shop = Shop::with_tour_at_first_head();

    let mut review = shop.open();

    review.go_to(&"total".into());
    let page = review.current_step().unwrap();
    assert_eq!(page.pieces[0].repin_status, None);
    assert!(!page.text_may_be_old);
}

fn lines_of(review: &Review, step: &str) -> Vec<String> {
    let mut review_lines = Vec::new();
    let page = review.current_step().unwrap();
    assert_eq!(page.step.id, step);
    for piece in &page.pieces {
        for line in piece.lines() {
            let sign = match line.location.side {
                Side::Base => "-",
                Side::Head => "+",
            };
            review_lines.push(format!("{sign}{}", line.text.trim()));
        }
    }
    review_lines
}

#[test]
fn a_step_read_before_shows_only_what_changed_since_its_tick_and_a_toggle_shows_the_full_diff() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["total"]);
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();
    let total: StepId = "total".into();

    review.go_to(&total);
    let since_read = review.current_step().unwrap().since_read.unwrap();
    assert_eq!(since_read.read_at, Sha::new(&shop.first_head));
    assert!(!since_read.showing_full_diff);
    assert_eq!(lines_of(&review, "total"), ["-return 2;", "+return 3;"]);
    let page = review.current_step().unwrap();
    let removed = page.pieces[0].lines().next().unwrap();
    assert!(
        removed
            .github_link
            .as_str()
            .contains(&format!("/blob/{}/app/Order.php#L7", shop.first_head)),
        "{}",
        removed.github_link
    );

    review.toggle_full_diff(&total);
    assert!(
        review
            .current_step()
            .unwrap()
            .since_read
            .unwrap()
            .showing_full_diff
    );
    assert_eq!(lines_of(&review, "total"), ["-return 1;", "+return 3;"]);
}

#[test]
fn no_row_of_the_change_since_the_read_takes_a_comment_but_the_full_diff_rows_do() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["total"]);
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();
    let total: StepId = "total".into();
    let takes_comment = |review: &Review| -> Vec<bool> {
        review.current_step().unwrap().pieces[0]
            .lines()
            .map(|line| line.takes_comment())
            .collect()
    };

    review.go_to(&total);
    assert_eq!(takes_comment(&review), [false, false]);

    review.toggle_full_diff(&total);
    assert_eq!(takes_comment(&review), [true, true]);
}

#[test]
fn a_ticked_step_whose_pieces_stayed_the_same_shows_no_change_since_it_was_read() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["keep"]);
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();

    review.go_to(&"keep".into());

    assert!(review.current_step().unwrap().since_read.is_some());
    assert_eq!(lines_of(&review, "keep"), Vec::<String>::new());
}

#[test]
fn an_unread_step_shows_the_full_diff() {
    let shop = Shop::with_tour_at_first_head();
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();

    review.go_to(&"total".into());

    assert!(review.current_step().unwrap().since_read.is_none());
    assert_eq!(lines_of(&review, "total"), ["-return 1;", "+return 3;"]);
}

#[test]
fn ticking_a_step_again_at_the_new_head_ends_its_since_view_even_when_unticked_after() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["total"]);
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();
    let total: StepId = "total".into();

    review.tick(&total).unwrap();
    review.untick(&total).unwrap();

    review.go_to(&total);
    assert!(review.current_step().unwrap().since_read.is_none());
    drop(review);
    let mut reopened = shop.open();
    reopened.go_to(&total);
    assert!(reopened.current_step().unwrap().since_read.is_none());
}

#[test]
fn a_hunk_read_before_shows_only_the_changes_since_that_land_inside_it() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["config"]);
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();

    review.go_to(&"config".into());

    let page = review.current_step().unwrap();
    let per_piece: Vec<(String, usize)> = page
        .pieces
        .iter()
        .map(|piece| (piece.id.to_string(), piece.lines().count()))
        .collect();
    assert_eq!(
        per_piece,
        [
            ("config.yaml#hunk-1".to_string(), 2),
            ("config.yaml#hunk-2".to_string(), 0)
        ]
    );
    assert_eq!(
        lines_of(&review, "config"),
        ["-key3: same", "+key3: added above"]
    );
}

#[test]
fn after_a_re_pin_github_viewed_state_is_the_new_baseline_and_still_ticked_files_are_viewed_again()
{
    let shop = Shop::with_tour_at_first_head();
    let sent = SentLog::default();
    let github_before = FakeGitHub::with_pull("main-tip", &shop.first_head, &sent)
        .viewing("old.txt", ViewedState::Viewed);
    let mut review = shop.open();
    review.sync(&github_before).unwrap();
    review.tick(&"readme".into()).unwrap();
    review.tick(&"config".into()).unwrap();
    review.untick(&"old".into()).unwrap();
    drop(review);
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let github = FakeGitHub::with_pull("main-tip", &second_head, &sent)
        .viewing("readme.txt", ViewedState::Dismissed)
        .viewing("config.yaml", ViewedState::Dismissed)
        .viewing("old.txt", ViewedState::Viewed)
        .viewing("app/Order.php", ViewedState::Viewed)
        .viewing("app/Pricing.php", ViewedState::Unviewed);
    let mut review = shop.open();

    review.sync(&github).unwrap();

    assert_eq!(ticked(&review), ["readme"]);
    assert_eq!(github.viewed_state("readme.txt"), ViewedState::Viewed);
    assert_eq!(github.viewed_state("old.txt"), ViewedState::Unviewed);
    assert_eq!(github.viewed_state("config.yaml"), ViewedState::Dismissed);
    assert!(!review.not_synced());

    github.set_viewed("readme.txt", ViewedState::Unviewed);
    review.sync(&github).unwrap();
    assert_eq!(ticked(&review), Vec::<String>::new());
}

#[test]
fn two_re_pins_without_a_refresh_read_as_one_from_the_first_head() {
    let shop = Shop::with_tour_at_first_head();
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    shop.repo.write("app/Order.php", &order_php(3, 1, 10));
    let third_head = shop.repo.commit("third head");

    shop.repin_onto(&third_head);

    let record = &state_on_disk(&shop)["last_repin"];
    assert_eq!(record["from_head"], shop.first_head.as_str());
    assert_eq!(record["to_head"], third_head.as_str());
    assert_eq!(record["pieces"]["app/Order.php#Order::total"], "changed");
    assert_eq!(record["pieces"]["app/Order.php#Order::keep"], "changed");
    assert_eq!(record["pieces"]["app/Pricing.php#Pricing::discount"], "new");
    assert_eq!(
        record["gone_by_step"],
        json!({"tax": ["app/Order.php#Order::tax"]})
    );
}

#[test]
fn the_change_since_read_is_built_from_hunks_with_context_around_its_changes() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["total"]);
    let second_head = shop.commit_second_head();
    shop.repin_onto(&second_head);
    let mut review = shop.open();
    review.set_files_at_head(review.files_at_head_request().read(&shop.source()));
    review.go_to(&"total".into());

    let page = review.current_step().unwrap();

    let piece = &page.pieces[0];
    assert_eq!(piece.hunks.len(), 1);
    let changed: Vec<_> = piece.lines().filter(|line| line.is_change()).collect();
    assert_eq!(changed.len(), 2);
    assert!(piece.lines().any(|line| !line.is_change()));
    assert!(
        piece
            .lines()
            .all(|line| line.location.path == "app/Order.php")
    );
    assert!(
        changed
            .iter()
            .all(|line| line.github_link.as_str().contains(&shop.first_head)
                || line.github_link.as_str().contains(&second_head))
    );
}

#[test]
fn repin_re_pins_to_the_pr_head_and_opens_the_review_on_the_new_tour() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["total", "keep"]);
    let second_head = shop.commit_second_head();
    let github = FakeGitHub::with_pull(&shop.base, &second_head, &SentLog::default());
    let clone = shop.repo.path().to_path_buf();
    let repinner = TourRepinner {
        home: shop.repo.home().to_path_buf(),
        pr: shop.pr(),
        pick_code_source: Arc::new(move |_| {
            Ok(Box::new(
                LocalGit::find(&RepoName::new("acme", "shop"), &clone)?.unwrap(),
            ))
        }),
    };

    let repinned = repinner.run(&github).unwrap();

    assert_eq!(repinned.review.head(), &Sha::new(&second_head));
    assert_eq!(
        Tour::load(&shop.tour_path()).unwrap().head,
        Sha::new(&second_head)
    );
    assert_eq!(ticked(&repinned.review), ["keep"]);
}

#[test]
fn ticks_on_the_open_review_are_refused_while_a_re_pin_runs_so_they_cannot_overwrite_its_record() {
    let shop = Shop::with_tour_at_first_head();
    shop.tick(&["total", "keep"]);
    let second_head = shop.commit_second_head();
    let mut review = shop.open();

    review.start_repin();
    shop.repin_onto(&second_head);
    let refused = [review.tick(&"total".into()), review.untick(&"keep".into())];

    assert!(refused.iter().all(Result::is_err));
    assert_eq!(ticked(&review), ["total", "keep"]);
    assert_eq!(ticked(&shop.open()), ["keep"]);
    assert_eq!(
        state_on_disk(&shop)["read_before"],
        json!({"total": {"head": shop.first_head}})
    );
}

#[test]
fn ticks_count_again_once_a_re_pin_failed() {
    let shop = Shop::with_tour_at_first_head();
    let mut review = shop.open();

    review.start_repin();
    review.repin_failed();
    review.tick(&"total".into()).unwrap();

    assert_eq!(ticked(&shop.open()), ["total"]);
}
