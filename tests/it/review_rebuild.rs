use std::sync::Arc;

use crate::support::FixtureRepo;
use crate::support::fake_ports::{FakeHost, SentLog};
use guided_review::open_review::{OpenedReview, open_review};
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, Tour, TourPin};
use guided_review::viewer::{
    MergeMethod, ProblemKind, RefreshWriter, RepinnedTour, Review, TourRefresher,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

/// What a review takes from the config and the disk when it is built, not from the reviewer.
#[derive(Debug, PartialEq, Eq)]
struct BuiltFacts {
    merge_method: MergeMethod,
    more_lines: usize,
    local_tours_not_read: bool,
}

impl BuiltFacts {
    fn of(review: &Review) -> Self {
        Self {
            merge_method: review.merge_method(),
            more_lines: review.display_options().more_lines().get(),
            local_tours_not_read: review
                .problems()
                .iter()
                .any(|problem| problem.kind() == ProblemKind::LocalToursNotRead),
        }
    }

    fn as_the_config_and_disk_say() -> Self {
        Self {
            merge_method: MergeMethod::Squash,
            more_lines: 5,
            local_tours_not_read: true,
        }
    }
}

/// README.md changed at `first`, then again at `second`; the config names a merge method and more
/// lines, and a local tour beside the PR's cannot be read.
struct Shop {
    repo: FixtureRepo,
    base: String,
    first: String,
    second: String,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        let base = repo.commit("base");
        repo.write("README.md", "hello\nworld\n");
        let first = repo.commit("first");
        repo.write("README.md", "hello\nworld\nagain\n");
        let second = repo.commit("second");
        let folders = repo.folders();
        let config = folders.config_file();
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(config, "more_lines = 5\nmerge_method = \"squash\"\n").unwrap();
        let local: ReviewSubject = format!("local:{}@feature", repo.path().display())
            .parse()
            .unwrap();
        let unreadable = Tour::path_for(&folders, &local);
        std::fs::create_dir_all(unreadable.parent().unwrap()).unwrap();
        std::fs::write(unreadable, "not a tour").unwrap();
        Self {
            repo,
            base,
            first,
            second,
        }
    }

    fn subject() -> ReviewSubject {
        PR.parse().unwrap()
    }

    fn write_guided_tour_at(&self, head: &str) -> &Self {
        let path = Tour::path_for(&self.repo.folders(), &Self::subject());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let tour = json!({
            "pr": PR, "base": self.base, "head": head, "title": "Hello", "summary": [],
            "steps": [{"id": "readme", "title": "Readme", "why": "w", "pieces": [{"file": "README.md"}]}],
        });
        std::fs::write(path, tour.to_string()).unwrap();
        self
    }

    fn write_basic_tour_at(&self, head: &str) -> &Self {
        let path = Tour::basic_path_for(&self.repo.folders(), &Self::subject());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        BasicTour::new(
            TourPin {
                version: 1,
                pr: PR.to_owned(),
                local: None,
                repo: None,
                base: Sha::new(&self.base),
                head: Sha::new(head),
                previous_head: None,
            },
            "Hello".to_owned(),
        )
        .save(&path)
        .unwrap();
        self
    }

    fn open(&self) -> OpenedReview {
        open_review(&Self::subject(), self.repo.path(), &self.repo.folders()).unwrap()
    }
}

/// The window's reopen: the rebuilt review goes on with what the one it replaces was doing.
fn reopened(opened: OpenedReview, rebuilt: RepinnedTour) -> Review {
    let mut review = rebuilt.review;
    review.carry_on_from(opened.review);
    review
}

struct WriterThatChangesNothing;

impl RefreshWriter for WriterThatChangesNothing {
    fn refresh(&self, _: &ReviewSubject, _: &mut dyn FnMut(&str)) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn a_review_opened_takes_the_merge_method_and_more_lines_from_the_config_and_names_unreadable_local_tours()
 {
    let shop = Shop::new();
    shop.write_guided_tour_at(&shop.second);

    let opened = shop.open();

    assert_eq!(
        BuiltFacts::of(&opened.review),
        BuiltFacts::as_the_config_and_disk_say()
    );
}

#[test]
fn a_review_updated_to_new_commits_keeps_what_the_config_and_disk_say() {
    let shop = Shop::new();
    shop.write_guided_tour_at(&shop.first);
    let opened = shop.open();
    let host = FakeHost::with_pull(&shop.base, &shop.second, &SentLog::default());

    let updated = opened.repinner.run(&host).unwrap();

    assert_eq!(updated.review.head(), &Sha::new(&shop.second));
    assert_eq!(
        BuiltFacts::of(&reopened(opened, updated)),
        BuiltFacts::as_the_config_and_disk_say()
    );
}

#[test]
fn a_review_reloaded_keeps_what_the_config_and_disk_say() {
    let shop = Shop::new();
    shop.write_guided_tour_at(&shop.second);
    let opened = shop.open();

    let reloaded = opened.reloader.run().unwrap();

    assert_eq!(
        BuiltFacts::of(&reopened(opened, reloaded)),
        BuiltFacts::as_the_config_and_disk_say()
    );
}

#[test]
fn a_review_switched_to_the_guided_tour_keeps_what_the_config_and_disk_say() {
    let shop = Shop::new();
    shop.write_basic_tour_at(&shop.second)
        .write_guided_tour_at(&shop.second);
    let opened = shop.open();
    assert!(opened.review.basic_mode_label().is_some());

    let switched = opened.switcher.run().unwrap();

    assert_eq!(
        BuiltFacts::of(&reopened(opened, switched)),
        BuiltFacts::as_the_config_and_disk_say()
    );
}

#[test]
fn a_review_refreshed_keeps_what_the_config_and_disk_say() {
    let shop = Shop::new();
    shop.write_guided_tour_at(&shop.second);
    let opened = shop.open();
    let refresher = TourRefresher {
        folders: shop.repo.folders(),
        subject: Shop::subject(),
        pick_code_source: Arc::clone(&opened.refresher.pick_code_source),
        writer: Arc::new(WriterThatChangesNothing),
    };

    let refreshed = refresher.run(&mut |_| {}).unwrap();

    assert_eq!(
        BuiltFacts::of(&reopened(opened, refreshed)),
        BuiltFacts::as_the_config_and_disk_say()
    );
}
