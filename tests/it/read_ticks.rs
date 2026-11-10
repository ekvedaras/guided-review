use std::path::PathBuf;

use crate::support::{FixtureRepo, file_path};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName};
use guided_review::tour::Tour;
use guided_review::viewer::{Problem, ReadProgress, Review, ReviewState, ReviewStateFile};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    fn with_pull() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        let base = repo.commit("base");
        repo.write("README.md", "hello\nworld\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn state_path(&self) -> PathBuf {
        self.repo
            .home()
            .join(".guided-review/state/acme/shop/7.json")
    }

    fn files_in_state_folder(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.state_path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_pr(self.repo.home(), &PR.parse::<PullRequestUrl>().unwrap())
    }

    fn open(&self) -> Review {
        self.try_open().unwrap()
    }

    fn try_open(&self) -> anyhow::Result<Review> {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Hello world",
            "steps": [
                {"id": "readme", "title": "Readme", "children": [
                    {"id": "greeting", "title": "Greeting"},
                ]},
                {"id": "emails", "title": "Emails"},
            ],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        Review::open(tour, &source, self.state_file())
    }

    fn state_on_disk(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.state_path()).unwrap()).unwrap()
    }
}

fn ticked_in_sidebar(review: &Review) -> Vec<String> {
    review
        .sidebar()
        .iter()
        .filter(|entry| entry.is_ticked)
        .map(|entry| entry.id.to_string())
        .collect()
}

#[test]
fn a_ticked_step_stays_ticked_after_the_app_is_reopened() {
    let shop = Shop::with_pull();
    let mut review = shop.open();

    review.tick(&"greeting".into()).unwrap();
    drop(review);

    assert_eq!(ticked_in_sidebar(&shop.open()), ["greeting"]);
}

#[test]
fn the_state_file_keeps_each_tick_with_the_head_it_was_made_at() {
    let shop = Shop::with_pull();
    let mut review = shop.open();

    review.tick(&"emails".into()).unwrap();

    assert_eq!(
        shop.state_on_disk()["read_ticks"],
        json!({"emails": {"head": shop.head}})
    );
    assert_eq!(shop.files_in_state_folder(), ["7.json", "7.json.lock"]);
}

#[test]
fn an_unticked_step_stays_unticked_after_the_app_is_reopened() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    review.tick(&"readme".into()).unwrap();
    review.tick(&"emails".into()).unwrap();

    review.untick(&"readme".into()).unwrap();
    drop(review);

    assert_eq!(ticked_in_sidebar(&shop.open()), ["emails"]);
}

#[test]
fn progress_counts_ticked_steps_out_of_all_steps() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    assert_eq!(review.progress(), ReadProgress { read: 0, total: 3 });

    review.tick(&"readme".into()).unwrap();
    review.tick(&"emails".into()).unwrap();

    assert_eq!(review.progress(), ReadProgress { read: 2, total: 3 });
}

#[test]
fn a_ticked_step_folds_and_opens_again_on_request() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    review.go_to(&"readme".into());
    assert!(!review.current_step().unwrap().is_folded);

    review.tick(&"readme".into()).unwrap();
    assert!(review.current_step().unwrap().is_folded);

    review.toggle_fold(&"readme".into());
    assert!(!review.current_step().unwrap().is_folded);
    assert!(review.is_ticked(&"readme".into()));
}

#[test]
fn a_folded_step_folds_each_file_box_on_its_own_and_opening_all_drops_single_choices() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    let readme = "readme".into();
    review.tick(&readme).unwrap();
    assert!(review.is_box_folded(&readme, &file_path("README.md")));

    review.toggle_box_fold(&readme, &file_path("README.md"));
    assert!(!review.is_box_folded(&readme, &file_path("README.md")));
    assert!(review.is_folded(&readme), "the step stays folded");

    review.toggle_fold(&readme);
    review.toggle_fold(&readme);
    assert!(review.is_box_folded(&readme, &file_path("README.md")));
}

#[test]
fn ticks_of_steps_the_tour_no_longer_has_are_kept_but_not_counted() {
    let shop = Shop::with_pull();
    std::fs::create_dir_all(shop.state_path().parent().unwrap()).unwrap();
    std::fs::write(
        shop.state_path(),
        json!({"read_ticks": {"dropped": {"head": "old"}}}).to_string(),
    )
    .unwrap();
    let mut review = shop.open();

    review.tick(&"emails".into()).unwrap();

    assert_eq!(review.progress(), ReadProgress { read: 1, total: 3 });
    assert_eq!(
        shop.state_on_disk()["read_ticks"],
        json!({"dropped": {"head": "old"}, "emails": {"head": shop.head}})
    );
}

#[test]
fn sections_a_newer_app_wrote_survive_a_tick() {
    let shop = Shop::with_pull();
    std::fs::create_dir_all(shop.state_path().parent().unwrap()).unwrap();
    std::fs::write(
        shop.state_path(),
        json!({"read_ticks": {}, "bookmarks": [{"question": "why?"}]}).to_string(),
    )
    .unwrap();
    let mut review = shop.open();

    review.tick(&"emails".into()).unwrap();

    assert_eq!(
        shop.state_on_disk()["bookmarks"],
        json!([{"question": "why?"}])
    );
}

fn write_broken_state(shop: &Shop, text: &str) {
    std::fs::create_dir_all(shop.state_path().parent().unwrap()).unwrap();
    std::fs::write(shop.state_path(), text).unwrap();
}

fn backups_in_state_folder(shop: &Shop) -> Vec<String> {
    shop.files_in_state_folder()
        .into_iter()
        .filter(|name| name.starts_with("7.broken-"))
        .collect()
}

#[test]
fn a_broken_state_file_is_moved_aside_and_the_app_opens_with_fresh_state() {
    let shop = Shop::with_pull();
    write_broken_state(&shop, "{ not json");

    let mut review = shop.open();

    let backups = backups_in_state_folder(&shop);
    assert_eq!(
        backups.len(),
        1,
        "files: {:?}",
        shop.files_in_state_folder()
    );
    assert_eq!(
        std::path::Path::new(&backups[0]).extension().unwrap(),
        "json"
    );
    let backup = shop.state_path().with_file_name(&backups[0]);
    assert_eq!(std::fs::read_to_string(backup).unwrap(), "{ not json");
    assert_eq!(ticked_in_sidebar(&review), Vec::<String>::new());

    review.tick(&"emails".into()).unwrap();
    assert_eq!(
        shop.state_on_disk()["read_ticks"],
        json!({"emails": {"head": shop.head}})
    );
}

#[test]
fn a_moved_aside_state_file_is_a_problem_in_the_sidebar_that_names_the_backup() {
    let shop = Shop::with_pull();
    write_broken_state(&shop, "{ not json");

    let review = shop.open();

    let backups = backups_in_state_folder(&shop);
    let backup = shop.state_path().with_file_name(&backups[0]);
    let problems = review.problems();
    assert_eq!(problems, [Problem::SavedReviewNotRead(backup.clone())]);
    let shown = problems[0].to_string();
    assert!(shown.contains("could not be read"), "shown: {shown}");
    assert!(
        shown.contains(&backup.display().to_string()),
        "shown: {shown}"
    );
}

#[test]
fn a_second_broken_state_file_does_not_overwrite_the_first_backup() {
    let shop = Shop::with_pull();
    write_broken_state(&shop, "first");
    drop(shop.open());
    write_broken_state(&shop, "second");
    drop(shop.open());

    let mut kept: Vec<String> = backups_in_state_folder(&shop)
        .iter()
        .map(|name| std::fs::read_to_string(shop.state_path().with_file_name(name)).unwrap())
        .collect();
    kept.sort();
    assert_eq!(kept, ["first", "second"]);
}

#[test]
fn a_broken_state_file_that_cannot_be_moved_stops_the_app_from_opening() {
    use std::os::unix::fs::PermissionsExt;
    let shop = Shop::with_pull();
    write_broken_state(&shop, "{ not json");
    let folder = shop.state_path().parent().unwrap().to_owned();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = shop.try_open();

    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = result.err().expect("opening should fail");
    assert!(
        format!("{error:#}").contains("is not a valid review state file"),
        "error: {error:#}"
    );
    assert_eq!(
        std::fs::read_to_string(shop.state_path()).unwrap(),
        "{ not json"
    );
}

#[test]
fn writers_saving_the_same_state_file_at_once_all_succeed_and_leave_no_temp_files() {
    let shop = Shop::with_pull();
    let writers: Vec<_> = (0..8)
        .map(|_| {
            let state_file = shop.state_file();
            std::thread::spawn(move || {
                (0..50)
                    .map(|_| state_file.save(&ReviewState::default()))
                    .collect::<anyhow::Result<Vec<()>>>()
            })
        })
        .collect();

    for writer in writers {
        writer.join().unwrap().unwrap();
    }

    assert_eq!(shop.state_on_disk()["read_ticks"], json!({}));
    assert_eq!(shop.files_in_state_folder(), ["7.json", "7.json.lock"]);
}

#[test]
fn changes_made_to_the_same_state_file_at_once_are_all_kept() {
    let shop = Shop::with_pull();
    let head = guided_review::pull_request::Sha::new(&shop.head);
    let writers: Vec<_> = (0..8)
        .map(|writer| {
            let (state_file, head) = (shop.state_file(), head.clone());
            std::thread::spawn(move || {
                (0..10)
                    .map(|change| {
                        let step = format!("step-{writer}-{change}");
                        state_file.update(|state| state.tick(&step.as_str().into(), &head))
                    })
                    .collect::<anyhow::Result<Vec<()>>>()
            })
        })
        .collect();

    for writer in writers {
        writer.join().unwrap().unwrap();
    }

    let ticks = shop.state_on_disk()["read_ticks"]
        .as_object()
        .unwrap()
        .len();
    assert_eq!(ticks, 80);
}

#[test]
fn the_read_share_runs_from_none_to_all_and_is_complete_only_at_all() {
    let none = ReadProgress { read: 0, total: 4 };
    let some = ReadProgress { read: 1, total: 4 };
    let all = ReadProgress { read: 4, total: 4 };

    assert!((none.share() - 0.).abs() < f32::EPSILON);
    assert!((some.share() - 0.25).abs() < f32::EPSILON);
    assert!((all.share() - 1.).abs() < f32::EPSILON);
    assert!(!some.is_complete());
    assert!(all.is_complete());
}

#[test]
fn a_tour_without_steps_has_read_nothing_and_is_not_complete() {
    let empty = ReadProgress { read: 0, total: 0 };

    assert!((empty.share() - 0.).abs() < f32::EPSILON);
    assert!(!empty.is_complete());
}

#[test]
fn next_unread_goes_to_the_first_unread_step_after_the_open_screen() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    review.tick(&"greeting".into()).unwrap();

    assert!(review.go_to_next_unread());
    assert_eq!(
        review.screen(),
        &guided_review::viewer::Screen::Step("readme".into())
    );
    assert!(review.go_to_next_unread());
    assert_eq!(
        review.screen(),
        &guided_review::viewer::Screen::Step("emails".into())
    );
}

#[test]
fn next_unread_starts_over_from_the_top_once_past_the_last_unread_step() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    review.tick(&"greeting".into()).unwrap();
    review.go_to(&"emails".into());

    assert!(review.go_to_next_unread());
    assert_eq!(
        review.screen(),
        &guided_review::viewer::Screen::Step("readme".into())
    );
}

#[test]
fn next_unread_stays_put_when_every_step_is_read() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    for step in ["readme", "greeting", "emails"] {
        review.tick(&step.into()).unwrap();
    }
    review.go_to(&"greeting".into());

    assert!(!review.go_to_next_unread());
    assert_eq!(
        review.screen(),
        &guided_review::viewer::Screen::Step("greeting".into())
    );
    assert_eq!(review.first_unread_step(), None);
}

#[test]
fn the_first_unread_step_is_named_by_its_title() {
    let shop = Shop::with_pull();
    let mut review = shop.open();
    review.tick(&"readme".into()).unwrap();

    let first = review.first_unread_step().unwrap();
    assert_eq!(first.id, "greeting".into());
    assert_eq!(first.title, "Greeting");
}
