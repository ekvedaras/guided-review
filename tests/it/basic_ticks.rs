use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeHost, HostFault, SentLog};
use crate::support::{FixtureRepo, file_path};
use guided_review::build::Build;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, TourPin};
use guided_review::viewer::{
    Effect, Intent, NextScreen, PaletteAction, ReadProgress, Review, ReviewStateFile, Screen,
    TourLead, TreeRowKind, ViewedState, WindowMoment,
};
use serde_json::Value;

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Shop {
    fn changing(files: &[&str]) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("seed.txt", "seed\n");
        let base = repo.commit("base");
        for file in files {
            repo.write(file, "changed\n");
        }
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn open(&self) -> Review {
        let tour = BasicTour::new(
            TourPin {
                version: 1,
                pr: PR.to_owned(),
                local: None,
                repo: None,
                host: None,
                base: Sha::new(&self.base),
                head: Sha::new(&self.head),
                previous_head: None,
            },
            "Orders cost two".to_owned(),
        );
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let state_file = ReviewStateFile::for_subject(
            &self.repo.folders(),
            &PR.parse::<ReviewSubject>().unwrap(),
        );
        Review::open_basic(
            tour,
            &source,
            state_file,
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }

    fn host(&self, files: &[&str]) -> FakeHost {
        files.iter().fold(
            FakeHost::with_pull("main-tip", &self.head, &self.sent),
            |host, path| host.viewing(path, ViewedState::Unviewed),
        )
    }

    fn state_on_disk(&self) -> Value {
        let path = self
            .repo
            .home()
            .join(".local/state/guided-review/state/acme/shop/7.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }
}

fn act(review: &mut Review, intent: Intent) {
    match review.decide(intent, WindowMoment::default()) {
        Effect::Change(change) => review.apply(change).unwrap(),
        other => panic!("{intent:?} changed nothing: {other:?}"),
    }
}

fn file(path: &str) -> Screen {
    Screen::File(file_path(path))
}

#[test]
fn marking_a_file_read_is_saved_and_survives_a_restart() {
    let shop = Shop::changing(&["a.txt", "b.txt"]);
    let mut review = shop.open();

    review.tick_file(&file_path("a.txt")).unwrap();

    assert!(review.is_file_ticked(&file_path("a.txt")));
    assert!(!review.is_file_ticked(&file_path("b.txt")));
    assert_eq!(
        shop.state_on_disk()["file_read_ticks"]["a.txt"]["head"],
        shop.head.as_str()
    );
    assert!(
        shop.state_on_disk()["read_ticks"]
            .as_object()
            .is_none_or(serde_json::Map::is_empty),
        "file ticks are apart from step ticks"
    );
    let reopened = shop.open();
    assert!(reopened.is_file_ticked(&file_path("a.txt")));
    assert!(!reopened.is_file_ticked(&file_path("b.txt")));
}

#[test]
fn unmarking_a_file_removes_its_tick_from_the_saved_state() {
    let shop = Shop::changing(&["a.txt"]);
    let mut review = shop.open();
    review.tick_file(&file_path("a.txt")).unwrap();

    review.untick_file(&file_path("a.txt")).unwrap();

    assert!(!review.is_file_ticked(&file_path("a.txt")));
    assert!(!shop.open().is_file_ticked(&file_path("a.txt")));
}

#[test]
fn a_file_the_tour_does_not_hold_cannot_be_ticked() {
    let shop = Shop::changing(&["a.txt"]);
    let mut review = shop.open();

    review.tick_file(&file_path("nope.txt")).unwrap();

    assert!(!review.is_file_ticked(&file_path("nope.txt")));
}

#[test]
fn the_overview_shows_which_files_are_read() {
    let shop = Shop::changing(&["a.txt", "b.txt"]);
    let mut review = shop.open();
    review.tick_file(&file_path("b.txt")).unwrap();

    let read: Vec<_> = review
        .overview_page()
        .files
        .iter()
        .map(|file| (file.path.to_string(), file.is_read))
        .collect();

    assert_eq!(
        read,
        vec![("a.txt".to_owned(), false), ("b.txt".to_owned(), true)]
    );
}

#[test]
fn the_forward_key_ticks_a_file_only_once_its_end_was_seen() {
    let shop = Shop::changing(&["a.txt", "b.txt"]);
    let mut review = shop.open();
    review.go_to_file(&file_path("a.txt"));

    act(&mut review, Intent::NextScreen);
    assert!(!review.is_file_ticked(&file_path("a.txt")));
    assert_eq!(review.screen(), &file("b.txt"));

    review.go_to_file(&file_path("a.txt"));
    review.saw_end_of_screen();
    act(&mut review, Intent::NextScreen);
    assert!(review.is_file_ticked(&file_path("a.txt")));
}

#[test]
fn the_footers_button_ticks_the_file_without_the_end_seen() {
    let shop = Shop::changing(&["a.txt", "b.txt"]);
    let mut review = shop.open();
    review.go_to_file(&file_path("a.txt"));

    act(&mut review, Intent::MarkReadAndGoOn);

    assert!(review.is_file_ticked(&file_path("a.txt")));
    assert_eq!(review.screen(), &file("b.txt"));
}

#[test]
fn the_palette_marks_a_file_as_read_and_as_unread() {
    let shop = Shop::changing(&["a.txt"]);
    let mut review = shop.open();
    review.go_to_file(&file_path("a.txt"));
    let actions = review.palette_actions(Build::Local);
    assert!(actions.contains(&PaletteAction::MarkFileRead));
    assert_eq!(PaletteAction::MarkFileRead.label(), "Mark file as read");

    act(&mut review, PaletteAction::MarkFileRead.into());
    assert!(review.is_file_ticked(&file_path("a.txt")));
    assert!(
        review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::MarkFileUnread)
    );

    act(&mut review, PaletteAction::MarkFileUnread.into());
    assert!(!review.is_file_ticked(&file_path("a.txt")));
}

#[test]
fn marking_as_read_on_the_overview_does_nothing() {
    let shop = Shop::changing(&["a.txt"]);
    let review = shop.open();

    assert_eq!(
        review.decide(Intent::ToggleRead, WindowMoment::default()),
        Effect::Nothing
    );
}

#[test]
fn progress_counts_files_and_the_lead_goes_start_continue_finish() {
    let shop = Shop::changing(&["a.txt", "b.txt"]);
    let mut review = shop.open();
    assert_eq!(review.progress(), ReadProgress { read: 0, total: 2 });
    let TourLead::StartFile(first) = review.tour_lead() else {
        panic!("{:?}", review.tour_lead());
    };
    assert_eq!(first.path, file_path("a.txt"));

    review.tick_file(&file_path("a.txt")).unwrap();
    assert_eq!(review.progress(), ReadProgress { read: 1, total: 2 });
    let TourLead::ContinueFile(next) = review.tour_lead() else {
        panic!("{:?}", review.tour_lead());
    };
    assert_eq!(next.path, file_path("b.txt"));

    review.tick_file(&file_path("b.txt")).unwrap();
    assert!(review.progress().is_complete());
    assert!(matches!(
        review.tour_lead(),
        TourLead::FinishFiles { files: 2, .. }
    ));
    assert_eq!(review.first_unread_file(), None);
}

#[test]
fn next_unread_walks_to_the_first_unread_file_after_the_open_one_and_wraps() {
    let shop = Shop::changing(&["a.txt", "b.txt", "c.txt"]);
    let mut review = shop.open();
    review.tick_file(&file_path("b.txt")).unwrap();
    review.go_to_file(&file_path("a.txt"));

    assert!(review.go_to_next_unread());
    assert_eq!(review.screen(), &file("c.txt"));
    assert!(review.go_to_next_unread());
    assert_eq!(review.screen(), &file("a.txt"), "wraps to the start");
    review.tick_file(&file_path("a.txt")).unwrap();
    review.tick_file(&file_path("c.txt")).unwrap();
    assert!(!review.go_to_next_unread());
}

#[test]
fn going_on_skips_files_already_read() {
    let shop = Shop::changing(&["a.txt", "b.txt", "c.txt"]);
    let mut review = shop.open();
    review.tick_file(&file_path("b.txt")).unwrap();
    review.go_to_file(&file_path("a.txt"));

    assert!(review.go_on());

    assert_eq!(review.screen(), &file("c.txt"));
    assert_eq!(
        review.reading_place().next,
        None,
        "nothing after the last file"
    );
    review.go_to_file(&file_path("a.txt"));
    assert_eq!(
        review.reading_place().next,
        Some(NextScreen::File {
            path: file_path("c.txt")
        })
    );
}

#[test]
fn the_approve_warning_names_how_many_files_are_unread() {
    let shop = Shop::changing(&["a.txt", "b.txt", "c.txt"]);
    let mut review = shop.open();
    review.tick_file(&file_path("a.txt")).unwrap();

    let warning = review.approve_warning().unwrap();
    assert_eq!(warning.unread_files, 2);
    assert_eq!(warning.unread_steps, 0);
    assert_eq!(warning.unread_safety_net_pieces, 0);

    review.tick_file(&file_path("b.txt")).unwrap();
    review.tick_file(&file_path("c.txt")).unwrap();
    assert_eq!(review.approve_warning(), None);
}

#[test]
fn a_folder_shows_as_read_once_every_file_under_it_is() {
    let shop = Shop::changing(&["src/a.rs", "src/b.rs", "top.txt"]);
    let mut review = shop.open();
    let folder_read = |review: &Review| {
        review
            .basic_sidebar()
            .into_iter()
            .find_map(|row| match row.kind {
                TreeRowKind::Folder { is_read, .. } => Some(is_read),
                TreeRowKind::File { .. } => None,
            })
            .unwrap()
    };
    let files_read = |review: &Review| -> Vec<bool> {
        review
            .basic_sidebar()
            .into_iter()
            .filter_map(|row| match row.kind {
                TreeRowKind::File { is_read, .. } => Some(is_read),
                TreeRowKind::Folder { .. } => None,
            })
            .collect()
    };

    review.tick_file(&file_path("src/a.rs")).unwrap();
    assert!(!folder_read(&review));
    assert_eq!(files_read(&review), [true, false, false]);

    review.tick_file(&file_path("src/b.rs")).unwrap();
    assert!(folder_read(&review));
    assert_eq!(files_read(&review), [true, true, false]);
}

#[test]
fn viewed_marks_follow_file_ticks_one_to_one() {
    let shop = Shop::changing(&["a.txt", "b.txt"]);
    let host = shop.host(&["a.txt", "b.txt"]);
    let mut review = shop.open();
    review.sync(&host).unwrap();

    review.tick_file(&file_path("a.txt")).unwrap();
    review.sync(&host).unwrap();
    assert_eq!(host.viewed_state("a.txt"), ViewedState::Viewed);
    assert_eq!(host.viewed_state("b.txt"), ViewedState::Unviewed);
    assert!(!review.not_synced());

    review.untick_file(&file_path("a.txt")).unwrap();
    review.sync(&host).unwrap();
    assert_eq!(host.viewed_state("a.txt"), ViewedState::Unviewed);
}

#[test]
fn a_change_on_githubs_page_ticks_and_unticks_the_file() {
    let shop = Shop::changing(&["a.txt", "b.txt"]);
    let host = shop.host(&["a.txt", "b.txt"]);
    let mut review = shop.open();
    review.sync(&host).unwrap();

    host.set_viewed("b.txt", ViewedState::Viewed);
    review.sync(&host).unwrap();
    assert!(review.is_file_ticked(&file_path("b.txt")));
    assert!(!review.is_file_ticked(&file_path("a.txt")));

    host.set_viewed("b.txt", ViewedState::Dismissed);
    review.sync(&host).unwrap();
    assert!(!review.is_file_ticked(&file_path("b.txt")));
}

#[test]
fn file_ticks_keep_working_when_viewed_sync_is_off() {
    let shop = Shop::changing(&["a.txt"]);
    let host = shop.host(&["a.txt"]);
    host.inject(HostFault::TokenRefused("GitHub refused the token".into()));
    let mut review = shop.open();
    review.sync(&host).ok();
    assert!(review.sync_banner().is_some());

    review.tick_file(&file_path("a.txt")).unwrap();

    assert!(review.is_file_ticked(&file_path("a.txt")));
    assert!(shop.open().is_file_ticked(&file_path("a.txt")));
}
