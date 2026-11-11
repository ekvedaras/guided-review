use crate::support::FixtureRepo;
use crate::support::{file_path, line};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    CommentId, FollowedLines, LineLocation, Review, ReviewStateFile, ReviewThread, Side,
    ThreadComment, ThreadId, follow_outdated,
};
use serde_json::json;

const PR: &str = "https://github.com/acme/shop/pull/7";

fn numbered(change: impl Fn(usize) -> Option<String>) -> String {
    (1..=20)
        .map(|number| change(number).unwrap_or_else(|| format!("line {number}\n")))
        .collect()
}

/// The thread was written on `commented`; since then two lines came in on top and line 10 changed.
struct Shop {
    repo: FixtureRepo,
    commented: String,
    head: String,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write("notes.txt", &numbered(|_| None));
        let commented = repo.commit("commented");
        repo.write(
            "notes.txt",
            &format!(
                "new a\nnew b\n{}",
                numbered(|number| (number == 10).then(|| "line ten\n".to_owned()))
            ),
        );
        let head = repo.commit("head");
        Self {
            repo,
            commented,
            head,
        }
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.commented,
            "head": self.head,
            "title": "Notes",
            "summary": [],
            "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": [{"file": "notes.txt"}], "children": []}],
        }))
        .unwrap();
        let pr: PullRequestUrl = PR.parse().unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &pr);
        let source = self.source();
        let mut review = Review::open(tour, &source, state_file).unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review
    }

    fn outdated(&self, id: &str, start: Option<usize>, end: usize) -> ReviewThread {
        ReviewThread {
            id: ThreadId::new(id),
            is_resolved: false,
            is_outdated: true,
            path: file_path("notes.txt"),
            line: None,
            start_line: None,
            side: Side::Head,
            is_file_comment: false,
            original_line: Some(line(end)),
            original_start_line: start.map(line),
            original_commit: Some(self.commented.clone()),
            comments: vec![ThreadComment {
                id: CommentId::new(&format!("{id}-c")),
                author: "me".into(),
                is_bot: false,
                by_viewer: true,
                body: "hm".into(),
                url: String::new(),
                reactions: Vec::new(),
            }],
        }
    }

    fn followed(&self, threads: &[ReviewThread]) -> Vec<(ThreadId, FollowedLines)> {
        follow_outdated(threads, &self.source(), &Sha::new(&self.head))
            .into_iter()
            .collect()
    }
}

#[test]
fn an_outdated_thread_on_changed_lines_lands_on_what_replaced_them_and_keeps_what_they_were() {
    let shop = Shop::new();

    let followed = shop.followed(&[shop.outdated("T1", None, 10)]);

    assert_eq!(
        followed,
        [(
            ThreadId::new("T1"),
            FollowedLines {
                start: line(12),
                end: line(12),
                before: vec!["line 10".to_owned()],
                were_changed: true,
            }
        )]
    );
}

#[test]
fn an_outdated_thread_on_unchanged_lines_moves_with_the_lines_added_above() {
    let shop = Shop::new();

    let followed = shop.followed(&[shop.outdated("T1", Some(4), 5)]);

    assert_eq!(followed[0].1.start, line(6));
    assert_eq!(followed[0].1.end, line(7));
    assert_eq!(followed[0].1.before, ["line 4", "line 5"]);
    assert!(!followed[0].1.were_changed);
}

#[test]
fn a_thread_still_on_its_lines_and_a_file_comment_are_not_followed() {
    let shop = Shop::new();
    let mut current = shop.outdated("T1", None, 3);
    current.is_outdated = false;
    current.line = Some(line(5));
    let mut on_file = shop.outdated("T2", None, 3);
    on_file.is_file_comment = true;

    assert_eq!(shop.followed(&[current, on_file]), []);
}

#[test]
fn a_followed_thread_hangs_under_its_lines_and_carries_what_they_were() {
    let shop = Shop::new();
    let mut review = shop.open();
    let threads = [shop.outdated("T1", None, 10)];
    review.set_threads(&threads);
    let unfollowed = review.current_step().unwrap().threads[0].anchor.clone();

    review.set_followed_lines(follow_outdated(
        &threads,
        &shop.source(),
        &Sha::new(&shop.head),
    ));

    assert_eq!(unfollowed, None, "shown after the code until followed");
    let view = &review.current_step().unwrap().threads[0];
    assert_eq!(
        view.anchor,
        Some(LineLocation {
            path: file_path("notes.txt"),
            side: Side::Head,
            line: line(12),
        })
    );
    assert_eq!(
        view.followed
            .as_ref()
            .map(|followed| followed.before.clone()),
        Some(vec!["line 10".to_owned()])
    );
}
