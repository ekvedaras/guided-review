use guided_review::local_work_tree::{LocalWorkTree, snapshot_ref};
use guided_review::opened_subject::OpenedSubject;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::Tour;

use crate::support::{FixtureRepo, file_path};

/// `feature` one commit ahead of `main`, with a file changed on disk.
fn feature_with_work_in_progress() -> FixtureRepo {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "hello\n");
    repo.commit("Start the shop");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("src/cart.rs", "pub fn total() {}\n");
    repo.commit("Add the cart");
    repo.write("README.md", "hello\nworld\n");
    repo
}

fn key_of(repo: &FixtureRepo) -> ReviewSubject {
    format!(
        "local:{}@feature",
        repo.path().canonicalize().unwrap().display()
    )
    .parse()
    .unwrap()
}

fn opened(repo: &FixtureRepo) -> OpenedSubject {
    OpenedSubject::in_folders(&key_of(repo), &repo.folders(), repo.path())
}

fn shown_pin_head(repo: &FixtureRepo) -> String {
    let subject = key_of(repo);
    Tour::load_of(&Tour::shown_path_for(&repo.folders(), &subject), &subject)
        .unwrap()
        .pin()
        .head
        .as_str()
        .to_owned()
}

#[test]
fn a_local_key_names_no_repo_until_it_is_opened_and_its_clone_names_one() {
    let repo = feature_with_work_in_progress();
    assert_eq!(key_of(&repo).local().unwrap().repo(), None);

    let subject = opened(&repo).subject();

    assert_eq!(
        subject.local().unwrap().repo().unwrap().to_string(),
        "acme/shop"
    );
    assert_eq!(subject, key_of(&repo));
}

#[test]
fn a_local_review_is_read_at_the_snapshot_its_tour_is_pinned_to_however_the_folder_moved_on() {
    let repo = feature_with_work_in_progress();
    assert!(opened(&repo).make_basic_tour_if_no_tour().unwrap());
    let pinned = shown_pin_head(&repo);
    repo.write("README.md", "hello\nagain\n");

    let code = opened(&repo).code_to_read().unwrap();

    assert_eq!(code.tips.head.as_str(), pinned);
    assert_ne!(
        LocalWorkTree::containing(repo.path())
            .unwrap()
            .make_snapshot()
            .unwrap()
            .head
            .as_str(),
        pinned
    );
}

#[test]
fn a_second_open_of_a_local_review_takes_no_new_snapshot() {
    let repo = feature_with_work_in_progress();
    opened(&repo).make_basic_tour_if_no_tour().unwrap();
    let pinned = shown_pin_head(&repo);
    repo.write("README.md", "hello\nagain\n");

    assert!(!opened(&repo).make_basic_tour_if_no_tour().unwrap());
    assert_eq!(shown_pin_head(&repo), pinned);
}

#[test]
fn repin_moves_a_local_tour_and_then_its_snapshot_ref_to_a_new_snapshot_of_the_folder() {
    let repo = feature_with_work_in_progress();
    opened(&repo).make_basic_tour_if_no_tour().unwrap();
    let before = shown_pin_head(&repo);
    repo.write("README.md", "hello\nagain\n");

    let repinned = opened(&repo).repin().unwrap();

    let after = shown_pin_head(&repo);
    assert_ne!(after, before);
    assert!(repinned.to_string().contains("README.md"), "{repinned}");
    let change_set = key_of(&repo);
    assert_eq!(
        repo.git(&["rev-parse", &snapshot_ref(change_set.local().unwrap())]),
        after
    );
    assert_eq!(
        opened(&repo).code_to_read().unwrap().tips.head.as_str(),
        after
    );
}

#[test]
fn a_local_review_opened_as_the_app_opens_it_offers_no_host_viewed_marks() {
    let repo = feature_with_work_in_progress();
    let (mut review, _source) = opened(&repo).open_review().unwrap();

    review.tick_file(&file_path("src/cart.rs")).unwrap();

    assert!(!review.not_synced());
}
