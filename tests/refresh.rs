#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]

#[allow(dead_code)]
mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guided_review::claude_cli::ClaudeRefreshWriter;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    PickCodeSource, RefreshWriter, Review, ReviewStateFile, TourRefresher, repin_tour,
};
use serde_json::json;
use support::{FakeGitHub, FixtureRepo, GuidedReview};
use tempfile::TempDir;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn order_php(total: u32, tax: u32, keep: u32) -> String {
    format!(
        "<?php\n\nclass Order\n{{\n    public function total(): int\n    {{\n        return {total};\n    }}\n\n    public function tax(): int\n    {{\n        return {tax};\n    }}\n\n    public function keep(): int\n    {{\n        return {keep};\n    }}\n}}\n"
    )
}

const PRICING_PHP: &str = "<?php\n\nclass Pricing\n{\n    public function discount(): int\n    {\n        return 0;\n    }\n}\n";

struct Shop {
    repo: FixtureRepo,
    base: String,
}

impl Shop {
    fn with_tour_at_first_head() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("app/Order.php", &order_php(1, 1, 1));
        repo.write("readme.txt", "old\n");
        let base = repo.commit("base");
        repo.write("app/Order.php", &order_php(2, 5, 9));
        repo.write("readme.txt", "new\n");
        let first_head = repo.commit("first head");
        let tour = json!({
            "pr": PR, "base": base, "head": first_head, "title": "Orders",
            "steps": [
                {"id": "total", "title": "Total", "pieces": [{"file": "app/Order.php", "symbol": "Order::total"}]},
                {"id": "tax", "title": "Tax", "pieces": [{"file": "app/Order.php", "symbol": "Order::tax"}]},
                {"id": "keep", "title": "Keep", "pieces": [{"file": "app/Order.php", "symbol": "Order::keep"}]},
                {"id": "readme", "title": "Readme", "pieces": [{"file": "readme.txt"}]},
            ],
        });
        let shop = Self { repo, base };
        let path = shop.tour_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, tour.to_string()).unwrap();
        shop
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

    /// Changes `total`, takes `tax` back to base, keeps `keep` and adds a class no step names.
    fn repin_onto_head_with_a_new_class(&self) -> String {
        self.repo.write("app/Order.php", &order_php(3, 1, 9));
        self.repo.write("app/Pricing.php", PRICING_PHP);
        let head = self.repo.commit("second head");
        self.repin_onto(&head);
        head
    }

    fn repin_onto_head_changing_only_total(&self) -> String {
        self.repo.write("app/Order.php", &order_php(3, 5, 9));
        let head = self.repo.commit("second head");
        self.repin_onto(&head);
        head
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

    fn refresh_cli(&self, head: &str, args: &[&str]) -> support::Run {
        let github = FakeGitHub::serving_pull("acme/shop", 7, &self.base, head);
        let mut all = vec!["refresh", PR];
        all.extend(args);
        GuidedReview::new(&self.repo, &github).run(&all)
    }

    fn text_may_be_old(&self, step: &str) -> bool {
        let tour = Tour::load(&self.tour_path()).unwrap();
        let mut review = Review::open(tour, &self.source(), self.state_file()).unwrap();
        review.go_to(&step.into());
        review.current_step().unwrap().text_may_be_old
    }
}

#[test]
fn the_brief_lists_steps_with_old_text_and_new_pieces_no_step_holds() {
    let shop = Shop::with_tour_at_first_head();
    let head = shop.repin_onto_head_with_a_new_class();

    let run = shop.refresh_cli(&head, &[]);

    assert!(run.success, "{}", run.stderr);
    assert!(
        run.stdout.contains("stale steps:\n  total  Total\n"),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("  keep"), "{}", run.stdout);
    assert!(
        run.stdout
            .contains("new pieces no step holds:\n  app/Pricing.php#(top)\n  app/Pricing.php#Pricing\n  app/Pricing.php#Pricing::discount\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn the_brief_suggests_a_redo_above_thirty_percent_new_or_changed_and_otherwise_not() {
    let many = Shop::with_tour_at_first_head();
    let head = many.repin_onto_head_with_a_new_class();
    let few = Shop::with_tour_at_first_head();
    let few_head = few.repin_onto_head_changing_only_total();

    let many_run = many.refresh_cli(&head, &[]);
    let few_run = few.refresh_cli(&few_head, &[]);

    assert!(
        many_run
            .stdout
            .contains("redo: 4 of 6 pieces are new or changed, more than 30%"),
        "{}",
        many_run.stdout
    );
    assert!(!few_run.stdout.contains("redo:"), "{}", few_run.stdout);
}

#[test]
fn done_clears_the_marks_and_the_brief() {
    let shop = Shop::with_tour_at_first_head();
    let head = shop.repin_onto_head_with_a_new_class();
    assert!(shop.text_may_be_old("total"));

    let done = shop.refresh_cli(&head, &["--done"]);

    assert!(done.success, "{}", done.stderr);
    assert!(!shop.text_may_be_old("total"));
    let brief = shop.refresh_cli(&head, &[]);
    assert!(
        brief.stdout.contains("nothing to refresh"),
        "{}",
        brief.stdout
    );
}

#[test]
fn the_app_offers_the_same_brief_until_done() {
    let shop = Shop::with_tour_at_first_head();
    shop.repin_onto_head_with_a_new_class();
    let tour = Tour::load(&shop.tour_path()).unwrap();

    let brief = Review::open(tour, &shop.source(), shop.state_file())
        .unwrap()
        .refresh_brief();

    assert_eq!(brief.stale_steps.len(), 1);
    assert_eq!(
        brief.unplaced_new_pieces,
        [
            "app/Pricing.php#(top)",
            "app/Pricing.php#Pricing",
            "app/Pricing.php#Pricing::discount"
        ]
    );
    assert!(brief.redo_suggested());
}

struct SkillStandIn {
    home: PathBuf,
    pr: PullRequestUrl,
    fails: bool,
}

impl RefreshWriter for SkillStandIn {
    fn refresh(&self, pr: &PullRequestUrl) -> anyhow::Result<()> {
        assert_eq!(pr.to_string(), self.pr.to_string());
        if self.fails {
            anyhow::bail!("claude stopped");
        }
        ReviewStateFile::for_pr(&self.home, pr).update(|state| state.finish_refresh())
    }
}

fn refresher(shop: &Shop, fails: bool) -> TourRefresher {
    let source = shop.repo.path().to_path_buf();
    let pick: PickCodeSource = Arc::new(move |_| {
        Ok(Box::new(
            LocalGit::find(&RepoName::new("acme", "shop"), &source)?.unwrap(),
        ))
    });
    TourRefresher {
        home: shop.repo.home().to_path_buf(),
        pr: shop.pr(),
        pick_code_source: pick,
        writer: Arc::new(SkillStandIn {
            home: shop.repo.home().to_path_buf(),
            pr: shop.pr(),
            fails,
        }),
    }
}

#[test]
fn the_refresh_button_runs_the_writer_then_reopens_the_review_without_the_marks() {
    let shop = Shop::with_tour_at_first_head();
    shop.repin_onto_head_with_a_new_class();

    let refreshed = refresher(&shop, false).run().unwrap();

    assert!(refreshed.review.refresh_brief().is_empty());
}

#[test]
fn a_failed_writer_is_reported_and_leaves_the_marks() {
    let shop = Shop::with_tour_at_first_head();
    shop.repin_onto_head_with_a_new_class();

    let error = refresher(&shop, true).run().err().unwrap();

    assert!(format!("{error:#}").contains("claude stopped"));
    assert!(shop.text_may_be_old("total"));
}

fn fake_claude(dir: &Path, exit_code: u8) -> PathBuf {
    let script = dir.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" > \"{dir}/args\"\npwd > \"{dir}/cwd\"\necho 'no login' >&2\nexit {exit_code}\n",
            dir = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[test]
fn claude_runs_the_tour_skill_in_the_background_with_a_narrow_tool_list_and_no_skipped_permissions()
{
    let dir = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let writer =
        ClaudeRefreshWriter::with_binary(fake_claude(dir.path(), 0), cwd.path().to_path_buf());

    writer.refresh(&PR.parse().unwrap()).unwrap();

    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    assert!(args.contains("-p "), "{args}");
    assert!(args.contains(PR), "{args}");
    assert!(args.contains("--permission-mode dontAsk"), "{args}");
    assert!(args.contains("--allowedTools"), "{args}");
    assert!(args.contains("Bash(guided-review refresh:*)"), "{args}");
    assert!(args.contains("Write(~/.guided-review/tours/**)"), "{args}");
    assert!(!args.contains("--dangerously-skip-permissions"), "{args}");
    assert!(
        !args.contains("--bare") && !args.contains("--redo"),
        "{args}"
    );
    let ran_in = std::fs::read_to_string(dir.path().join("cwd")).unwrap();
    assert_eq!(
        std::fs::canonicalize(ran_in.trim()).unwrap(),
        std::fs::canonicalize(cwd.path()).unwrap()
    );
}

#[test]
fn a_claude_that_exits_with_an_error_fails_the_refresh_with_what_it_said() {
    let dir = TempDir::new().unwrap();
    let writer =
        ClaudeRefreshWriter::with_binary(fake_claude(dir.path(), 1), dir.path().to_path_buf());

    let error = writer.refresh(&PR.parse().unwrap()).unwrap_err();

    assert!(format!("{error:#}").contains("no login"), "{error:#}");
}
