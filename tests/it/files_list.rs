//! The files list: every changed file as a folder tree, with the screens that show it, and going to one.

use crate::support::FixtureRepo;
use crate::support::file_path;
use guided_review::build::Build;
use guided_review::local_git::LocalGit;
use guided_review::pull_request::Sha;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{BasicTour, GuidedTour, TourPin};
use guided_review::viewer::{
    Command, Effect, FilesList, FilesListRowKind, Intent, Overlay, PaletteAction, Review,
    ReviewStateFile, Screen, StepId, WindowMoment,
};
use serde_json::{Value, json};

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

impl Shop {
    /// `Order::total` and `Order::tax` change in one file, and a line of `config.yml`.
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        let order = |total: u32, tax: u32| {
            format!(
                "<?php\n\nclass Order\n{{\n    public function total(): int\n    {{\n        return {total};\n    }}\n\n    public function tax(): int\n    {{\n        return {tax};\n    }}\n}}\n"
            )
        };
        repo.write("app/Order.php", &order(1, 10));
        repo.write("config.yml", "a: 1\nb: 2\n");
        let base = repo.commit("base");
        repo.write("app/Order.php", &order(2, 20));
        repo.write("config.yml", "a: 1\nb: two\n");
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn state_file(&self) -> ReviewStateFile {
        ReviewStateFile::for_subject(&self.repo.folders(), &PR.parse::<ReviewSubject>().unwrap())
    }

    fn guided(&self) -> Review {
        let tour: GuidedTour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Orders cost more",
            "summary": ["Order total and tax go up"],
            "steps": [
                step("total", json!([{"file": "app/Order.php", "symbol": "Order::total"}])),
                step("config", json!([{"file": "config.yml"}])),
                step("tax", json!([{"file": "app/Order.php", "symbol": "Order::tax"}])),
            ],
        }))
        .unwrap();
        Review::open(
            tour,
            &self.source(),
            self.state_file(),
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }

    fn basic(&self) -> Review {
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
            "Orders cost more".to_owned(),
        );
        Review::open_basic(
            tour,
            &self.source(),
            self.state_file(),
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }
}

fn step(id: &str, pieces: Value) -> Value {
    json!({"id": id, "title": format!("Title of {id}"), "why": "because", "pieces": pieces})
}

/// Each row as `depth label +added -removed`, a folded folder marked `>`.
fn shape(list: &FilesList) -> Vec<String> {
    list.rows()
        .iter()
        .map(|row| match &row.kind {
            FilesListRowKind::Folder {
                label,
                is_folded,
                added,
                removed,
                ..
            } => format!(
                "{}{label}/{} +{added} -{removed}",
                row.depth,
                if *is_folded { ">" } else { "" }
            ),
            FilesListRowKind::File {
                label,
                added,
                removed,
                ..
            } => format!("{}{label} +{added} -{removed}", row.depth),
        })
        .collect()
}

fn cursor_label(list: &FilesList) -> Option<String> {
    list.rows()
        .iter()
        .find(|row| row.is_under_cursor)
        .map(|row| match &row.kind {
            FilesListRowKind::Folder { label, .. } | FilesListRowKind::File { label, .. } => {
                label.clone()
            }
        })
}

fn headings(list: &FilesList) -> Vec<String> {
    list.picked().map_or_default(|file| {
        file.places
            .iter()
            .map(|place| place.heading.clone())
            .collect()
    })
}

#[test]
fn the_files_list_shows_the_changed_files_as_a_tree_with_their_line_counts() {
    let shop = Shop::new();

    let list = shop.guided().files_list();

    assert_eq!(
        shape(&list),
        vec!["0app/ +2 -2", "1Order.php +2 -2", "0config.yml +1 -1"]
    );
    assert_eq!(list.file_count(), 2);
    assert_eq!(list.line_counts(), (3, 3));
}

#[test]
fn a_file_lists_every_step_that_shows_it_in_reading_order() {
    let shop = Shop::new();

    let list = shop.guided().files_list();

    assert_eq!(cursor_label(&list).as_deref(), Some("Order.php"));
    assert_eq!(
        headings(&list),
        vec!["Step 1 · Title of total", "Step 3 · Title of tax"]
    );
}

#[test]
fn going_to_a_file_lands_on_its_first_step_or_on_the_step_picked_by_number() {
    let shop = Shop::new();
    let mut review = shop.guided();
    let list = review.files_list();

    let first = list.target_of_picked(0).unwrap();
    let second = list.target_of_picked(1).unwrap();
    review.jump_to(&second);

    assert_eq!(first.screen, Screen::Step(StepId::from("total")));
    assert_eq!(second.screen, Screen::Step(StepId::from("tax")));
    assert_eq!(review.screen(), &Screen::Step(StepId::from("tax")));
    assert_eq!(list.target_of_picked(2), None);
}

#[test]
fn a_file_is_read_once_every_step_that_shows_it_is_read() {
    let shop = Shop::new();
    let mut review = shop.guided();
    review.tick(&StepId::from("total")).unwrap();
    let one_of_two = review.files_list().picked().unwrap().is_read;

    review.tick(&StepId::from("tax")).unwrap();
    let list = review.files_list();

    assert!(!one_of_two);
    assert!(list.picked().unwrap().is_read);
    assert!(matches!(
        list.rows()[0].kind,
        FilesListRowKind::Folder { is_read: true, .. }
    ));
}

#[test]
fn the_cursor_starts_on_the_first_file_the_screen_on_show_shows() {
    let shop = Shop::new();
    let mut review = shop.guided();
    review.go_to(&StepId::from("config"));

    let list = review.files_list();

    assert_eq!(cursor_label(&list).as_deref(), Some("config.yml"));
    assert!(list.picked().unwrap().places[0].is_current);
}

#[test]
fn the_cursor_cycles_and_a_folded_folder_hides_its_files() {
    let shop = Shop::new();
    let mut list = shop.guided().files_list();

    list.cursor_up();
    let on_folder = cursor_label(&list);
    list.fold_under_cursor();
    let folded = shape(&list);
    list.cursor_up();
    let wrapped = cursor_label(&list);
    list.cursor_down();
    list.unfold_under_cursor();

    assert_eq!(on_folder.as_deref(), Some("app"));
    assert_eq!(folded, vec!["0app/> +2 -2", "0config.yml +1 -1"]);
    assert_eq!(wrapped.as_deref(), Some("config.yml"));
    assert_eq!(list.rows().len(), 3);
    assert!(list.is_on_folder());
}

#[test]
fn a_refresh_keeps_the_folds_and_the_cursor() {
    let shop = Shop::new();
    let mut review = shop.guided();
    let mut list = review.files_list();
    list.cursor_up();
    list.fold_under_cursor();

    review.tick(&StepId::from("config")).unwrap();
    let list = list.refreshed_to(&review.files_list());

    assert_eq!(shape(&list), vec!["0app/> +2 -2", "0config.yml +1 -1"]);
    assert_eq!(cursor_label(&list).as_deref(), Some("app"));
}

#[test]
fn a_basic_tours_file_goes_to_its_own_screen() {
    let shop = Shop::new();
    let mut review = shop.basic();
    review.tick_file(&file_path("app/Order.php")).unwrap();

    let list = review.files_list();

    assert_eq!(headings(&list), vec!["app/Order.php"]);
    assert!(list.picked().unwrap().is_read);
    assert_eq!(
        list.target_of_picked(0).unwrap().screen,
        Screen::File(file_path("app/Order.php"))
    );
}

#[test]
fn t_and_the_palette_open_the_files_list() {
    let shop = Shop::new();
    let review = shop.guided();

    assert_eq!(
        review.decide(Intent::ToggleFilesList, WindowMoment::default()),
        Effect::Toggle(Overlay::Files)
    );
    assert!(
        review
            .palette_actions(Build::Local)
            .contains(&PaletteAction::FilesList)
    );
    assert_eq!(
        Intent::from(PaletteAction::FilesList),
        Intent::ToggleFilesList
    );
    assert_eq!(PaletteAction::FilesList.label(), "Files list");
    assert_eq!(PaletteAction::FilesList.command(), Some(Command::FilesList));
    assert_eq!(Command::FilesList.default_chords()[0].keystroke(), "t");
}
