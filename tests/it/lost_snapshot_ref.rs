use crate::support::FixtureRepo;
use guided_review::local_work_tree::{LocalWorkTree, snapshot_ref};
use guided_review::open_review::open_review;
use guided_review::opened_subject::OpenedChangeSet;
use guided_review::review_subject::ReviewSubject;
use guided_review::tour::{Tour, ToursOnDisk};

struct Feature {
    repo: FixtureRepo,
    subject: ReviewSubject,
}

impl Feature {
    fn with_a_basic_tour() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("README.md", "hello\n");
        repo.commit("Start the shop");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("src/cart.rs", "pub fn total() {}\n");
        repo.commit("Add the cart");
        let work_tree = LocalWorkTree::containing(repo.path()).unwrap();
        let subject = ReviewSubject::from(work_tree.change_set().unwrap());
        let feature = Self { repo, subject };
        feature
            .opened_change_set()
            .make_basic_tour_if_no_tour()
            .unwrap();
        feature
    }

    fn opened_change_set(&self) -> OpenedChangeSet {
        OpenedChangeSet::new(self.subject.local().unwrap(), &self.repo.folders())
    }

    fn lose_the_ref(&self) {
        let reference = snapshot_ref(self.subject.local().unwrap());
        self.repo.git(&["update-ref", "-d", &reference]);
    }

    fn tours_on_disk(&self) -> ToursOnDisk {
        ToursOnDisk::of(&self.repo.folders(), &self.subject)
    }

    fn open_error(&self) -> String {
        let error = open_review(&self.subject, self.repo.path(), &self.repo.folders())
            .err()
            .expect("the open should have been refused");
        format!("{error:#}")
    }
}

#[test]
fn a_lost_snapshot_ref_is_a_problem_that_keeps_the_tour() {
    let feature = Feature::with_a_basic_tour();
    feature.lose_the_ref();

    let problem = feature.open_error();

    assert!(problem.contains("snapshot"), "{problem}");
    assert!(problem.contains("lost"), "{problem}");
    assert!(problem.contains("Drop review"), "{problem}");
    assert_ne!(feature.tours_on_disk(), ToursOnDisk::None);
}

#[test]
fn a_lost_snapshot_ref_is_the_same_problem_every_time_it_is_opened() {
    let feature = Feature::with_a_basic_tour();
    feature.lose_the_ref();

    let first = feature.open_error();

    assert_eq!(feature.open_error(), first);
}

#[test]
fn a_review_with_a_lost_ref_can_be_dropped_and_started_again() {
    let feature = Feature::with_a_basic_tour();
    feature.lose_the_ref();
    feature.opened_change_set().drop_review().unwrap();

    let started = feature
        .opened_change_set()
        .make_basic_tour_if_no_tour()
        .unwrap();

    assert!(started);
    assert!(
        open_review(
            &feature.subject,
            feature.repo.path(),
            &feature.repo.folders()
        )
        .is_ok()
    );
}

#[test]
fn a_tour_that_outlives_a_drop_is_deleted_as_gone_on_purpose() {
    let feature = Feature::with_a_basic_tour();
    let basic = Tour::basic_path_for(&feature.repo.folders(), &feature.subject);
    let kept_copy = std::fs::read(&basic).unwrap();
    feature.opened_change_set().drop_review().unwrap();
    std::fs::create_dir_all(basic.parent().unwrap()).unwrap();
    std::fs::write(&basic, kept_copy).unwrap();

    let problem = feature.open_error();

    assert!(
        problem.contains("was dropped or moved to a PR"),
        "{problem}"
    );
    assert_eq!(feature.tours_on_disk(), ToursOnDisk::None);
}

#[test]
fn starting_a_dropped_review_again_forgets_that_it_was_dropped() {
    let feature = Feature::with_a_basic_tour();
    feature.opened_change_set().drop_review().unwrap();
    feature
        .opened_change_set()
        .make_basic_tour_if_no_tour()
        .unwrap();
    feature.lose_the_ref();

    let problem = feature.open_error();

    assert!(problem.contains("lost"), "{problem}");
    assert_ne!(feature.tours_on_disk(), ToursOnDisk::None);
}
