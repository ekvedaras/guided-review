#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]

#[allow(dead_code)]
mod support;

use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    CommentPlace, CommentRefusal, LineLocation, Markdown, Reaction, ReactionCount, Review,
    ReviewStateFile, ReviewThread, SendMode, Side, SuggestionDiff, ThreadAction,
    ThreadActionResult, ThreadComment,
};
use serde_json::json;
use support::FixtureRepo;
use support::app_path::AppPath;
use support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};

const PR: &str = "https://github.com/acme/shop/pull/7";

struct Pull {
    repo: FixtureRepo,
    base: String,
    head: String,
    sent: SentLog,
}

impl Pull {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        let numbered = |ten: &str| -> String {
            (1..=20)
                .map(|number| match number {
                    10 => format!("{ten}\n"),
                    _ => format!("line {number}\n"),
                })
                .collect()
        };
        repo.write("notes.txt", &numbered("line 10"));
        let base = repo.commit("base");
        repo.write("notes.txt", &numbered("line ten"));
        let head = repo.commit("head");
        Self {
            repo,
            base,
            head,
            sent: SentLog::default(),
        }
    }

    fn github(&self) -> FakeGitHub {
        FakeGitHub::with_pull("main-tip", &self.head, &self.sent)
    }

    fn open(&self) -> Review {
        let tour: Tour = serde_json::from_value(json!({
            "pr": PR,
            "base": self.base,
            "head": self.head,
            "title": "Notes",
            "summary": [],
            "steps": [{"id": "notes", "title": "Notes", "why": "because", "pieces": [{"file": "notes.txt"}], "children": []}],
        }))
        .unwrap();
        let source = LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
            .unwrap()
            .unwrap();
        let pr: PullRequestUrl = PR.parse().unwrap();
        let state_file = ReviewStateFile::for_pr(self.repo.home(), &pr);
        let mut review = Review::open(tour, &source, state_file).unwrap();
        let files_at_head = review.files_at_head_request().read(&source);
        review.set_files_at_head(files_at_head);
        review.go_forward();
        review
    }
}

fn comment(id: &str, body: &str) -> ThreadComment {
    ThreadComment {
        id: id.into(),
        author: "alice".into(),
        is_bot: false,
        body: body.into(),
        url: format!("{PR}#discussion_{id}"),
        reactions: Vec::new(),
    }
}

fn thread_on_line_ten(comments: Vec<ThreadComment>) -> ReviewThread {
    ReviewThread {
        id: "T1".into(),
        is_resolved: false,
        is_outdated: false,
        path: "notes.txt".into(),
        line: Some(10),
        start_line: None,
        side: Side::Head,
        is_file_comment: false,
        original_line: Some(10),
        original_start_line: None,
        original_commit: None,
        comments,
    }
}

fn github_with_thread(pull: &Pull, thread: ReviewThread) -> FakeGitHub {
    let mut github = pull.github();
    github.threads = vec![thread];
    github
}

fn shown_thread(review: &Review) -> ReviewThread {
    review
        .current_step()
        .unwrap()
        .threads
        .first()
        .unwrap()
        .thread
        .clone()
}

fn row(review: &Review, text: &str) -> LineLocation {
    review
        .current_step()
        .unwrap()
        .pieces
        .iter()
        .flat_map(|piece| piece.lines())
        .find(|line| line.text == text)
        .unwrap_or_else(|| panic!("no row shows {text:?}"))
        .location
        .clone()
}

fn drag(review: &mut Review, from: &str, to: &str) -> CommentPlace {
    let (from, to) = (row(review, from), row(review, to));
    review.begin_selection(from);
    review.extend_selection(to);
    review.finish_selection().unwrap().unwrap()
}

#[test]
fn resolving_a_thread_tells_github_and_shows_it_resolved() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();

    let result = review.act_on_thread(
        ThreadAction::Resolve {
            thread_id: "T1".into(),
        },
        &github,
    );

    assert_eq!(result, ThreadActionResult::Done);
    assert!(pull.sent.github_calls().contains(&GitHubCall::SetResolved {
        thread_id: "T1".into(),
        resolved: true
    }));
    assert!(shown_thread(&review).is_resolved);
}

#[test]
fn unresolving_a_thread_tells_github_and_shows_it_open() {
    let pull = Pull::new();
    let mut resolved = thread_on_line_ten(vec![comment("C1", "why?")]);
    resolved.is_resolved = true;
    let github = github_with_thread(&pull, resolved);
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();

    review.act_on_thread(
        ThreadAction::Unresolve {
            thread_id: "T1".into(),
        },
        &github,
    );

    assert!(pull.sent.github_calls().contains(&GitHubCall::SetResolved {
        thread_id: "T1".into(),
        resolved: false
    }));
    assert!(!shown_thread(&review).is_resolved);
}

#[test]
fn a_reaction_is_added_and_removed_on_a_click() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();

    review.act_on_thread(
        ThreadAction::SetReaction {
            comment_id: "C1".into(),
            reaction: Reaction::Rocket,
            on: true,
        },
        &github,
    );
    let rocket = |review: &Review| {
        shown_thread(review).comments[0]
            .reaction_summary()
            .into_iter()
            .find(|count| count.reaction == Reaction::Rocket)
            .unwrap()
    };
    assert_eq!(
        rocket(&review),
        ReactionCount {
            reaction: Reaction::Rocket,
            count: 1,
            viewer_has_reacted: true
        }
    );

    review.act_on_thread(
        ThreadAction::SetReaction {
            comment_id: "C1".into(),
            reaction: Reaction::Rocket,
            on: false,
        },
        &github,
    );
    assert_eq!(rocket(&review).count, 0);
    assert_eq!(
        pull.sent
            .github_calls()
            .into_iter()
            .filter(|call| matches!(call, GitHubCall::SetReaction { .. }))
            .collect::<Vec<_>>(),
        [true, false].map(|on| GitHubCall::SetReaction {
            comment_id: "C1".into(),
            reaction: Reaction::Rocket,
            on
        })
    );
}

#[test]
fn a_comment_always_shows_all_eight_reactions_in_githubs_order() {
    let mut commented = comment("C1", "why?");
    commented.reactions = vec![ReactionCount {
        reaction: Reaction::Heart,
        count: 2,
        viewer_has_reacted: true,
    }];

    let summary = commented.reaction_summary();

    assert_eq!(
        summary
            .iter()
            .map(|count| count.reaction)
            .collect::<Vec<_>>(),
        Reaction::ALL
    );
    assert_eq!(summary[5].count, 2);
    assert_eq!(summary.iter().map(|count| count.count).sum::<usize>(), 2);
}

#[test]
fn a_reply_goes_into_the_pending_review_or_posts_now() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_pending_review(&github).unwrap();
    assert!(review.can_comment_now());

    let now = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: "T1".into(),
            body: "because".into(),
            mode: SendMode::CommentNow,
        },
        &github,
    );
    assert_eq!(now, ThreadActionResult::Done);

    let added = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: "T1".into(),
            body: "and also".into(),
            mode: SendMode::AddToReview,
        },
        &github,
    );
    assert_eq!(added, ThreadActionResult::Done);
    assert!(
        !review.can_comment_now(),
        "the reply started a pending review"
    );
    assert!(github.has_pending_review_now());
}

#[test]
fn a_reply_now_is_refused_while_a_pending_review_exists() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]))
        .with_web_pending_review();
    let mut review = pull.open();

    let result = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: "T1".into(),
            body: "because".into(),
            mode: SendMode::CommentNow,
        },
        &github,
    );

    assert_eq!(
        result,
        ThreadActionResult::Refused(CommentRefusal::PendingReviewExists)
    );
    assert!(
        !pull
            .sent
            .github_calls()
            .iter()
            .any(|call| matches!(call, GitHubCall::Reply { .. })),
        "nothing was sent"
    );
    assert!(!review.can_comment_now());
}

#[test]
fn a_failed_action_says_why_and_changes_nothing() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();
    github.inject(GitHubFault::ConnectionLost);

    let result = review.act_on_thread(
        ThreadAction::Resolve {
            thread_id: "T1".into(),
        },
        &github,
    );

    assert_eq!(result, ThreadActionResult::Failed("network is down".into()));
    assert!(!shown_thread(&review).is_resolved);
}

#[test]
fn suggest_change_fills_the_box_with_the_selected_lines_in_a_suggestion_block() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line 9", "line ten");

    let body = review.suggest_change(&place).unwrap().unwrap();

    assert_eq!(body, "```suggestion\nline 9\nline ten\n```\n");
    assert_eq!(review.comment_drafts()[0].body, body);
}

#[test]
fn suggest_change_keeps_the_text_already_typed() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line ten", "line ten");
    review.edit_draft(&place, "use a name").unwrap();

    let body = review.suggest_change(&place).unwrap().unwrap();

    assert_eq!(body, "use a name\n\n```suggestion\nline ten\n```\n");
}

#[test]
fn suggest_change_is_not_offered_when_removed_lines_are_selected() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line 10", "line ten");

    assert!(!review.can_suggest_change(&place));
    assert_eq!(review.suggest_change(&place).unwrap(), None);
    assert!(review.comment_drafts()[0].body.is_empty());
}

#[test]
fn a_suggestion_shows_as_the_commented_lines_out_and_the_new_text_in() {
    let diff = SuggestionDiff::of(
        &["line 9".to_string(), "line ten".to_string()],
        "line nine\nline 10\n",
    );

    assert_eq!(diff.removed, ["line 9", "line ten"]);
    assert_eq!(diff.added, ["line nine", "line 10"]);
}

#[test]
fn an_empty_suggestion_deletes_the_lines() {
    let diff = SuggestionDiff::of(&["line ten".to_string()], "");

    assert_eq!(diff.removed, ["line ten"]);
    assert!(diff.added.is_empty());
}

#[test]
fn a_thread_knows_the_lines_a_suggestion_is_against() {
    let pull = Pull::new();
    let mut review = pull.open();
    let mut thread = thread_on_line_ten(vec![comment("C1", "try:\n\n```suggestion\nline 10\n```")]);
    thread.start_line = Some(9);
    review.set_threads(&[thread]);

    let view = review.current_step().unwrap().threads.remove(0);

    assert_eq!(view.commented_lines, ["line 9", "line ten"]);
    let Markdown { blocks } = Markdown::parse(&view.thread.comments[0].body);
    assert!(blocks.iter().any(|block| matches!(
        block,
        guided_review::viewer::Block::Code { language: Some(language), .. } if language == "suggestion"
    )));
}

#[test]
fn a_reply_being_written_is_saved_and_comes_back_after_reopening() {
    let pull = Pull::new();
    let mut review = pull.open();

    review.open_reply("T1").unwrap();
    review.edit_reply("T1", "because it was slow").unwrap();

    let reopened = pull.open();
    assert_eq!(reopened.reply_draft("T1"), Some("because it was slow"));
    assert_eq!(reopened.reply_draft("T2"), None);
}

#[test]
fn a_reply_draft_goes_when_the_reply_is_sent_or_discarded() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();
    review.open_reply("T1").unwrap();
    review.edit_reply("T1", "because").unwrap();

    review.act_on_thread(
        ThreadAction::Reply {
            thread_id: "T1".into(),
            body: "because".into(),
            mode: SendMode::AddToReview,
        },
        &github,
    );
    assert_eq!(pull.open().reply_draft("T1"), None);

    review.open_reply("T1").unwrap();
    review.edit_reply("T1", "again").unwrap();
    review.discard_reply("T1").unwrap();
    assert_eq!(pull.open().reply_draft("T1"), None);
}

#[test]
fn a_failed_reply_keeps_its_draft() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.open_reply("T1").unwrap();
    review.edit_reply("T1", "because").unwrap();
    github.inject(GitHubFault::ConnectionLost);

    review.act_on_thread(
        ThreadAction::Reply {
            thread_id: "T1".into(),
            body: "because".into(),
            mode: SendMode::AddToReview,
        },
        &github,
    );

    assert_eq!(pull.open().reply_draft("T1"), Some("because"));
}

#[test]
fn a_reply_is_refused_while_the_tour_is_behind_the_pr_head() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pr_head_is(Sha::new("newer-head"));
    review.open_reply("T1").unwrap();
    review.edit_reply("T1", "because").unwrap();

    let result = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: "T1".into(),
            body: "because".into(),
            mode: SendMode::AddToReview,
        },
        &github,
    );

    assert_eq!(
        result,
        ThreadActionResult::Refused(CommentRefusal::TourIsOld)
    );
    assert!(
        !pull
            .sent
            .github_calls()
            .iter()
            .any(|call| matches!(call, GitHubCall::Reply { .. })),
        "nothing was sent"
    );
    assert_eq!(review.reply_draft("T1"), Some("because"), "the text stays");
}

#[test]
fn typed_text_reaches_the_file_only_when_its_save_runs_off_the_ui_thread() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line 9", "line ten");
    review.open_reply("T1").unwrap();

    review.type_in_draft(&place, "use a name").unwrap();
    review.type_in_reply("T1", "because").unwrap();
    review.type_in_finish_body("overall fine").unwrap();

    let before = pull.open();
    assert_eq!(before.reply_draft("T1"), Some(""));
    assert_eq!(before.finish_body(), "");
    assert_eq!(review.reply_draft("T1"), Some("because"));

    review.typing_save().unwrap().run().unwrap();

    let after = pull.open();
    assert_eq!(after.reply_draft("T1"), Some("because"));
    assert_eq!(after.finish_body(), "overall fine");
    assert_eq!(after.comment_drafts()[0].body, "use a name");
    assert!(review.typing_save().is_none());
}

#[test]
fn a_typing_save_that_a_later_save_overtook_is_dropped() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.open_reply("T1").unwrap();
    review.type_in_reply("T1", "because").unwrap();
    let stale = review.typing_save().unwrap();

    review.discard_reply("T1").unwrap();
    stale.run().unwrap();

    assert_eq!(pull.open().reply_draft("T1"), None);
}

#[test]
fn a_save_made_at_once_takes_the_typed_text_with_it() {
    let pull = Pull::new();
    let mut review = pull.open();
    review.open_reply("T1").unwrap();
    review.type_in_reply("T1", "because").unwrap();

    review.open_reply("T2").unwrap();

    assert_eq!(pull.open().reply_draft("T1"), Some("because"));
    assert!(review.typing_save().is_none());
}
