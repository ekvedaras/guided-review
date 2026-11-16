use crate::support::{FixtureRepo, file_path};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::tour::{BasicTour, TourPin};
use guided_review::viewer::{
    FolderPath, JumpPlace, NextScreen, OverviewStats, PageRow, ReadingPlace, Review, ReviewChange,
    ReviewStateFile, Screen, TreeRow, TreeRowKind,
};
use std::sync::Arc;

fn open_basic(repo: &FixtureRepo, files: &[&str]) -> Review {
    repo.write("seed.txt", "seed\n");
    let base = repo.commit("base");
    for file in files {
        repo.write(file, "changed\n");
    }
    let head = repo.commit("head");
    let tour = BasicTour::new(
        TourPin {
            version: 1,
            pr: "https://github.com/acme/shop/pull/7".to_owned(),
            local: None,
            base: Sha::new(&base),
            head: Sha::new(&head),
            previous_head: None,
        },
        "Orders cost two".to_owned(),
    );
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_subject(
        &repo.folders(),
        &"https://github.com/acme/shop/pull/7".parse().unwrap(),
    );
    Review::open_basic(tour, &source, state_file).unwrap()
}

const TREE_FILES: [&str; 5] = [
    "README.md",
    "src/main.rs",
    "src/viewer/b.rs",
    "src/viewer/a.rs",
    "docs/guide/intro.md",
];

fn reading_order(review: &mut Review) -> Vec<Screen> {
    let mut screens = vec![review.screen().clone()];
    while review.go_forward() {
        screens.push(review.screen().clone());
    }
    screens
}

fn file(path: &str) -> Screen {
    Screen::File(file_path(path))
}

fn drawn(rows: &[TreeRow]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let indent = "  ".repeat(row.depth);
            match &row.kind {
                TreeRowKind::Folder {
                    label, is_folded, ..
                } => format!(
                    "{indent}{label}/{}",
                    if *is_folded { " (folded)" } else { "" }
                ),
                TreeRowKind::File {
                    label, is_current, ..
                } => format!("{indent}{label}{}", if *is_current { " <-" } else { "" }),
            }
        })
        .collect()
}

#[test]
fn a_basic_review_opens_on_the_overview_and_reads_files_in_tree_order() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &TREE_FILES);

    assert_eq!(review.screen(), &Screen::Overview);
    assert_eq!(
        reading_order(&mut review),
        vec![
            Screen::Overview,
            file("docs/guide/intro.md"),
            file("src/viewer/a.rs"),
            file("src/viewer/b.rs"),
            file("src/main.rs"),
            file("README.md"),
        ]
    );
}

#[test]
fn going_back_walks_the_reading_order_down_to_the_overview() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["a.txt", "b.txt"]);
    review.go_forward();
    review.go_forward();

    assert_eq!(review.screen(), &file("b.txt"));
    assert!(review.go_back());
    assert_eq!(review.screen(), &file("a.txt"));
    assert!(review.go_back());
    assert_eq!(review.screen(), &Screen::Overview);
    assert!(!review.go_back());
}

#[test]
fn a_basic_review_has_no_safety_net() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["a.txt"]);

    assert!(review.safety_net().is_none());
    assert!(review.safety_net_size().is_none());
    review.go_to_safety_net();
    assert_eq!(review.screen(), &Screen::Overview);
}

#[test]
fn the_sidebar_is_a_tree_with_folders_first_and_single_child_folders_joined() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = open_basic(&repo, &TREE_FILES);

    assert_eq!(
        drawn(&review.basic_sidebar()),
        vec![
            "docs/guide/",
            "  intro.md",
            "src/",
            "  viewer/",
            "    a.rs",
            "    b.rs",
            "  main.rs",
            "README.md",
        ]
    );
}

#[test]
fn the_sidebar_marks_the_file_on_screen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["a.txt", "b.txt"]);
    review.go_forward();
    review.go_forward();

    assert_eq!(drawn(&review.basic_sidebar()), vec!["a.txt", "b.txt <-"]);
}

#[test]
fn a_folded_folder_hides_what_is_under_it_until_unfolded() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &TREE_FILES);
    let src = FolderPath::new("src");

    review
        .apply(ReviewChange::ToggleFolder(src.clone()))
        .unwrap();
    assert_eq!(
        drawn(&review.basic_sidebar()),
        vec!["docs/guide/", "  intro.md", "src/ (folded)", "README.md"]
    );

    review.apply(ReviewChange::ToggleFolder(src)).unwrap();
    assert_eq!(drawn(&review.basic_sidebar()).len(), 8);
}

#[test]
fn a_joined_folder_folds_as_one_row() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &TREE_FILES);

    review.toggle_folder(&FolderPath::new("docs/guide"));

    assert_eq!(
        drawn(&review.basic_sidebar())[..2],
        ["docs/guide/ (folded)", "src/"]
    );
}

#[test]
fn guided_only_questions_get_empty_answers_on_every_screen_of_a_basic_review() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["a.txt", "b.txt"]);

    loop {
        let _ = review.page();
        assert_eq!(review.sidebar().len(), 0);
        assert!(review.current_step().is_none());
        assert_eq!(
            review
                .approve_warning()
                .map(|warning| (warning.unread_steps, warning.unread_safety_net_pieces)),
            Some((0, 0)),
            "a basic review counts files, never steps or safety net pieces"
        );
        assert_eq!(review.summary().len(), 0);
        assert!(review.reviewer_summary().is_none());
        assert_eq!(review.route().len(), 0);
        assert_eq!(review.title(), "Orders cost two");
        if !review.go_forward() {
            break;
        }
    }
}

#[test]
fn the_overview_page_gives_title_counts_files_and_an_offered_write_guided_tour() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["src/a.rs", "b.txt"]);
    review
        .set_pr_description("Orders cost **two**.\n\nSee the ticket.")
        .unwrap();

    let page = Arc::clone(review.page());
    let overview = page.overview().unwrap();

    assert_eq!(overview.title, "Orders cost two");
    assert_eq!(
        overview.stats,
        OverviewStats {
            files: 2,
            added_lines: 2,
            removed_lines: 0,
            open_threads: 0,
        }
    );
    assert_eq!(
        overview
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.added_lines, file.is_read))
            .collect::<Vec<_>>(),
        vec![("src/a.rs", 1, false), ("b.txt", 1, false)]
    );
    assert_eq!(overview.description, review.pr_description());
    assert!(overview.description.is_some());
    assert_eq!(overview.write_guided_tour_refusal, None);
}

#[test]
fn a_file_screen_shows_the_whole_file_in_a_box_that_folds_by_its_screen() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["a.txt", "b.txt"]);
    review.go_to_file(&file_path("a.txt"));

    let page = Arc::clone(review.page());
    let kinds: Vec<&str> = page
        .rows()
        .iter()
        .map(|row| match row {
            PageRow::Toolbar(_) => "toolbar",
            PageRow::PieceHeader(_) => "header",
            PageRow::Line(_) => "line",
            PageRow::PieceEnd => "end",
            PageRow::ScreenFooter(_) => "footer",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["toolbar", "header", "line", "end", "footer"]);
    let Some(PageRow::PieceHeader(header)) = page.row(1) else {
        panic!("no header");
    };
    let fold = header.fold.clone().unwrap();
    assert_eq!(fold.screen, file("a.txt"));
    assert!(!fold.is_folded);
    assert_eq!(page.line(2).unwrap().code.as_ref(), "changed");
    let Some(PageRow::ScreenFooter(footer)) = page.rows().last() else {
        panic!("no footer");
    };
    assert_eq!(
        footer.next,
        Some(NextScreen::File {
            path: file_path("b.txt")
        })
    );
    assert!(footer.has_previous);
    assert_eq!(footer.file_place, Some((1, 2)));

    review.toggle_box_fold(&fold.screen, &fold.path);

    assert!(
        !review
            .page()
            .rows()
            .iter()
            .any(|row| matches!(row, PageRow::Line(_)))
    );
    assert!(review.is_box_folded(&file("a.txt"), &file_path("a.txt")));
    assert!(!review.is_box_folded(&file("b.txt"), &file_path("a.txt")));
}

#[test]
fn the_palette_lists_the_overview_then_the_files_in_tree_order() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["src/b.rs", "src/a.rs", "z.txt"]);

    let targets = review.jump_targets("");
    assert_eq!(
        targets
            .iter()
            .map(|target| (target.screen.clone(), target.label().into_owned()))
            .collect::<Vec<_>>(),
        vec![
            (Screen::Overview, "Overview".to_owned()),
            (file("src/a.rs"), "src/a.rs".to_owned()),
            (file("src/b.rs"), "src/b.rs".to_owned()),
            (file("z.txt"), "z.txt".to_owned()),
        ]
    );
    assert_eq!(targets[0].place, JumpPlace::Overview);
    assert_eq!(targets[1].place, JumpPlace::File(file_path("src/a.rs")));
    assert_eq!(targets[1].detail(), None);

    let found = review.jump_targets("b.rs");
    assert_eq!(found.len(), 1);
    review.jump_to(&found[0]);
    assert_eq!(review.screen(), &file("src/b.rs"));
}

#[test]
fn the_reading_place_counts_files_and_names_the_next_one() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["a.txt", "b.txt", "c.txt"]);
    let next = |name: &str| {
        Some(NextScreen::File {
            path: file_path(name),
        })
    };

    assert_eq!(
        review.reading_place(),
        ReadingPlace {
            step_number: None,
            step_count: 0,
            file_number: None,
            file_count: 3,
            has_previous: false,
            next: next("a.txt"),
        }
    );
    review.go_to_file(&file_path("b.txt"));
    assert_eq!(
        review.reading_place(),
        ReadingPlace {
            step_number: None,
            step_count: 0,
            file_number: Some(2),
            file_count: 3,
            has_previous: true,
            next: next("c.txt"),
        }
    );
    review.go_to_file(&file_path("c.txt"));
    assert_eq!(review.reading_place().next, None);
}

#[test]
fn a_basic_review_says_it_is_in_basic_mode() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let review = open_basic(&repo, &["a.txt"]);

    assert_eq!(review.basic_mode_label(), Some("Basic mode"));
}

#[test]
fn folding_does_not_change_the_reading_order() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &TREE_FILES);
    review.toggle_folder(&FolderPath::new("src"));

    assert_eq!(reading_order(&mut review).len(), 6);
}

#[test]
fn the_tree_sorts_names_without_regard_to_case_as_an_ide_does() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &["B.txt", "a.txt", "Zeta/x.txt", "alpha/y.txt"]);

    assert_eq!(
        reading_order(&mut review)[1..],
        [
            file("alpha/y.txt"),
            file("Zeta/x.txt"),
            file("a.txt"),
            file("B.txt"),
        ]
    );
}
