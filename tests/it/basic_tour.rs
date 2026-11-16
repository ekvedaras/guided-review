use crate::support::{FixtureRepo, file_path};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::tour::{BasicTour, TourPin};
use guided_review::viewer::{
    FolderPath, Review, ReviewChange, ReviewStateFile, Screen, TreeRow, TreeRowKind,
};

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
            base: Sha::new(&base),
            head: Sha::new(&head),
            previous_head: None,
        },
        "Orders cost two".to_owned(),
    );
    let source = LocalGit::find(&RepoName::new("acme", "shop"), repo.path())
        .unwrap()
        .unwrap();
    let state_file = ReviewStateFile::for_pr(
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
        assert_eq!(review.jump_targets("").len(), 0);
        assert!(review.current_step().is_none());
        assert!(review.approve_warning().is_none());
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
fn folding_does_not_change_the_reading_order() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let mut review = open_basic(&repo, &TREE_FILES);
    review.toggle_folder(&FolderPath::new("src"));

    assert_eq!(reading_order(&mut review).len(), 6);
}
