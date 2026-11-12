use crate::support::line;
use std::path::PathBuf;

use crate::support::app_path::AppPath;
use crate::support::fake_ports::{FakeGitHub, GitHubCall, GitHubFault, SentLog};
use crate::support::{FixtureRepo, drafts, file_path, save_typing};
use guided_review::local_git::LocalGit;
use guided_review::pull_request::{PullRequestUrl, RepoName, Sha};
use guided_review::tour::Tour;
use guided_review::viewer::{
    Audience, CommentGesture, CommentId, CommentPlace, CommentRefusal, LineLocation, Markdown,
    Problem, Reaction, ReactionChange, ReactionCount, Read, Resolution, Review, ReviewStateFile,
    ReviewThread, SendMode, Side, TextBox, ThreadAction, ThreadActionResult, ThreadComment,
    ThreadId,
};
use serde_json::json;

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
        id: CommentId::new(id),
        author: "alice".into(),
        is_bot: false,
        by_viewer: false,
        body: body.into(),
        url: format!("{PR}#discussion_{id}"),
        reactions: Vec::new(),
    }
}

fn thread_on_line_ten(comments: Vec<ThreadComment>) -> ReviewThread {
    ReviewThread {
        id: ThreadId::new("T1"),
        is_resolved: false,
        is_outdated: false,
        path: file_path("notes.txt"),
        line: Some(line(10)),
        start_line: None,
        side: Side::Head,
        is_file_comment: false,
        original_line: Some(line(10)),
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
        .flat_map(guided_review::viewer::PieceDiff::lines)
        .find(|line| line.text == text)
        .unwrap_or_else(|| panic!("no row shows {text:?}"))
        .location
        .clone()
}

fn drag(review: &mut Review, from: &str, to: &str) -> CommentPlace {
    let (from, to) = (row(review, from), row(review, to));
    review.begin_selection(from);
    review.extend_selection(to);
    review
        .open_comment_box(CommentGesture::DragEnded)
        .unwrap()
        .unwrap()
}

#[test]
fn resolving_a_thread_tells_github_and_shows_it_resolved() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();

    let result = review.act_on_thread(
        ThreadAction::Resolve {
            thread_id: ThreadId::new("T1"),
        },
        &github,
    );

    assert_eq!(result, ThreadActionResult::Done);
    assert!(pull.sent.github_calls().contains(&GitHubCall::SetResolved {
        thread_id: ThreadId::new("T1"),
        resolution: Resolution::Resolved
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
            thread_id: ThreadId::new("T1"),
        },
        &github,
    );

    assert!(pull.sent.github_calls().contains(&GitHubCall::SetResolved {
        thread_id: ThreadId::new("T1"),
        resolution: Resolution::Unresolved
    }));
    assert!(!shown_thread(&review).is_resolved);
}

fn shown_thread_is_folded(review: &Review) -> bool {
    review
        .current_step()
        .unwrap()
        .threads
        .first()
        .unwrap()
        .is_folded
}

#[test]
fn a_thread_folds_once_it_is_resolved_even_when_the_reviewer_opened_it() {
    let pull = Pull::new();
    let by_bot = ThreadComment {
        is_bot: true,
        ..comment("C1", "nit")
    };
    let github = github_with_thread(&pull, thread_on_line_ten(vec![by_bot]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();
    review.toggle_thread_fold(&ThreadId::new("T1"));
    assert!(!shown_thread_is_folded(&review));

    review.act_on_thread(
        ThreadAction::Resolve {
            thread_id: ThreadId::new("T1"),
        },
        &github,
    );

    assert!(shown_thread_is_folded(&review));
}

#[test]
fn an_opened_resolved_thread_stays_open_once_it_is_unresolved() {
    let pull = Pull::new();
    let mut resolved = thread_on_line_ten(vec![comment("C1", "why?")]);
    resolved.is_resolved = true;
    let github = github_with_thread(&pull, resolved);
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();
    review.toggle_thread_fold(&ThreadId::new("T1"));
    assert!(!shown_thread_is_folded(&review));

    review.act_on_thread(
        ThreadAction::Unresolve {
            thread_id: ThreadId::new("T1"),
        },
        &github,
    );

    assert!(!shown_thread_is_folded(&review));
}

#[test]
fn a_reaction_is_added_and_removed_on_a_click() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();

    review.act_on_thread(
        ThreadAction::SetReaction {
            comment_id: CommentId::new("C1"),
            reaction: Reaction::Rocket,
            change: ReactionChange::Add,
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
            comment_id: CommentId::new("C1"),
            reaction: Reaction::Rocket,
            change: ReactionChange::Remove,
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
        [ReactionChange::Add, ReactionChange::Remove].map(|change| GitHubCall::SetReaction {
            comment_id: CommentId::new("C1"),
            reaction: Reaction::Rocket,
            change
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
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();

    let now = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: ThreadId::new("T1"),
            mode: SendMode::CommentNow,
            audience: Audience::Author,
        },
        &github,
    );
    assert_eq!(now, ThreadActionResult::Done);
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "and also")
        .unwrap();

    let added = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: ThreadId::new("T1"),
            mode: SendMode::AddToReview,
            audience: Audience::Author,
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
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();

    let result = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: ThreadId::new("T1"),
            mode: SendMode::CommentNow,
            audience: Audience::Author,
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
            thread_id: ThreadId::new("T1"),
        },
        &github,
    );

    assert_eq!(result, ThreadActionResult::Failed("network is down".into()));
    assert!(!shown_thread(&review).is_resolved);
}

#[test]
fn threads_that_could_not_be_read_again_after_an_action_are_named_until_a_read_goes_through() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();
    github.inject(GitHubFault::ThreadReadsFail);

    let result = review.act_on_thread(
        ThreadAction::Resolve {
            thread_id: ThreadId::new("T1"),
        },
        &github,
    );

    assert_eq!(result, ThreadActionResult::Done);
    assert_eq!(
        review.problems(),
        [Problem::ThreadsNotRead("network is down".into())]
    );
    github.heal(&GitHubFault::ThreadReadsFail);
    review.pull_threads(&github).unwrap();
    assert_eq!(review.problems(), []);
}

#[test]
fn a_pr_head_that_could_not_be_read_is_named_until_a_read_goes_through() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    github.inject(GitHubFault::ReadsFail);

    review.check_pr_head(&github).unwrap_err();

    assert_eq!(
        review.problems(),
        [Problem::NotRead(Read::PrHead, "network is down".into())]
    );
    github.heal(&GitHubFault::ReadsFail);
    review.check_pr_head(&github).unwrap();
    assert_eq!(review.problems(), []);
}

#[test]
fn a_pr_description_that_could_not_be_read_is_named_until_a_read_goes_through() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    github.inject(GitHubFault::ReadsFail);

    review.pull_pr_description(&github).unwrap_err();

    assert_eq!(
        review.problems(),
        [Problem::NotRead(
            Read::PrDescription,
            "network is down".into()
        )]
    );
    github.heal(&GitHubFault::ReadsFail);
    review.pull_pr_description(&github).unwrap();
    assert_eq!(review.problems(), []);
}

#[test]
fn a_pending_review_lookup_that_failed_is_named_until_a_read_goes_through() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    github.inject(GitHubFault::ReadsFail);

    review.pull_pending_review(&github).unwrap_err();

    assert_eq!(
        review.problems(),
        [Problem::NotRead(
            Read::PendingReview,
            "network is down".into()
        )]
    );
    github.heal(&GitHubFault::ReadsFail);
    review.pull_pending_review(&github).unwrap();
    assert_eq!(review.problems(), []);
}

#[test]
fn an_authorship_lookup_that_failed_is_named_until_a_read_goes_through() {
    let pull = Pull::new();
    let github = pull.github();
    let mut review = pull.open();
    github.inject(GitHubFault::ReadsFail);

    review.pull_viewer_authorship(&github).unwrap_err();

    assert_eq!(
        review.problems(),
        [Problem::NotRead(Read::Authorship, "network is down".into())]
    );
    github.heal(&GitHubFault::ReadsFail);
    review.pull_viewer_authorship(&github).unwrap();
    assert_eq!(review.problems(), []);
}

#[test]
fn an_ide_that_could_not_start_is_named_until_one_starts() {
    let pull = Pull::new();
    let mut review = pull.open();

    review.ide_not_started(&"No such file or directory");

    assert_eq!(
        review.problems(),
        [Problem::IdeNotStarted("No such file or directory".into())]
    );
    review.ide_started();
    assert_eq!(review.problems(), []);
}

#[test]
fn suggest_change_fills_the_box_with_the_selected_lines_in_a_suggestion_block() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line 9", "line ten");

    let body = review.suggest_change(&place).unwrap().unwrap();

    assert_eq!(body, "```suggestion\nline 9\nline ten\n```\n");
    assert_eq!(drafts(&review)[0].body, body);
}

#[test]
fn suggest_change_keeps_the_text_already_typed() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line ten", "line ten");
    review
        .type_in(&TextBox::Draft(place.clone()), "use a name")
        .unwrap();

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
    assert_eq!(drafts(&review)[0].body, "");
}

#[test]
fn a_thread_knows_the_lines_a_suggestion_is_against() {
    let pull = Pull::new();
    let mut review = pull.open();
    let mut thread = thread_on_line_ten(vec![comment("C1", "try:\n\n```suggestion\nline 10\n```")]);
    thread.start_line = Some(line(9));
    review.set_threads(&[thread]);

    let view = review.current_step().unwrap().threads.remove(0);

    let lines: Vec<&str> = view.commented_lines.iter().map(|line| &**line).collect();
    assert_eq!(lines, ["line 9", "line ten"]);
    let Markdown { blocks } = Markdown::parse(&view.thread.comments[0].body);
    assert!(blocks.iter().any(|block| matches!(
        block,
        guided_review::viewer::Block::Code { language: Some(language), .. } if language == "suggestion"
    )));
}

#[test]
fn a_reply_draft_goes_when_the_reply_is_sent() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();
    save_typing(&mut review);

    review.act_on_thread(
        ThreadAction::Reply {
            thread_id: ThreadId::new("T1"),
            mode: SendMode::AddToReview,
            audience: Audience::Author,
        },
        &github,
    );
    assert_eq!(
        pull.open().text_of(&TextBox::Reply(ThreadId::new("T1"))),
        None
    );
}

#[test]
fn a_failed_reply_keeps_its_draft() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();
    save_typing(&mut review);
    github.inject(GitHubFault::ConnectionLost);

    review.act_on_thread(
        ThreadAction::Reply {
            thread_id: ThreadId::new("T1"),
            mode: SendMode::AddToReview,
            audience: Audience::Author,
        },
        &github,
    );

    assert_eq!(
        pull.open().text_of(&TextBox::Reply(ThreadId::new("T1"))),
        Some("because")
    );
}

#[test]
fn a_reply_is_refused_while_the_tour_is_behind_the_pr_head() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pr_head_is(Sha::new("newer-head"));
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();
    save_typing(&mut review);

    let result = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: ThreadId::new("T1"),
            mode: SendMode::AddToReview,
            audience: Audience::Author,
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
    assert_eq!(
        review.text_of(&TextBox::Reply(ThreadId::new("T1"))),
        Some("because"),
        "the text stays"
    );
}

#[test]
fn typed_text_reaches_the_file_only_when_its_save_runs_off_the_ui_thread() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line 9", "line ten");
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();

    review
        .type_in(&TextBox::Draft(place), "use a name")
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();
    review
        .type_in(&TextBox::FinishBody, "overall fine")
        .unwrap();

    let before = pull.open();
    assert_eq!(
        before.text_of(&TextBox::Reply(ThreadId::new("T1"))),
        Some("")
    );
    assert_eq!(before.text_of(&TextBox::FinishBody), None);
    assert_eq!(
        review.text_of(&TextBox::Reply(ThreadId::new("T1"))),
        Some("because")
    );

    review.typing_save().unwrap().run().unwrap();

    let after = pull.open();
    assert_eq!(
        after.text_of(&TextBox::Reply(ThreadId::new("T1"))),
        Some("because")
    );
    assert_eq!(after.text_of(&TextBox::FinishBody), Some("overall fine"));
    assert_eq!(drafts(&after)[0].body, "use a name");
    assert!(review.typing_save().is_none());
}

#[test]
fn a_typing_save_that_a_later_save_overtook_is_dropped() {
    let pull = Pull::new();
    let mut review = pull.open();
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();
    let stale = review.typing_save().unwrap();

    review
        .discard(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    stale.run().unwrap();

    assert_eq!(
        pull.open().text_of(&TextBox::Reply(ThreadId::new("T1"))),
        None
    );
}

#[test]
fn a_save_made_at_once_takes_the_typed_text_with_it() {
    let pull = Pull::new();
    let mut review = pull.open();
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();

    review
        .open_box(&TextBox::Reply(ThreadId::new("T2")))
        .unwrap();

    assert_eq!(
        pull.open().text_of(&TextBox::Reply(ThreadId::new("T1"))),
        Some("because")
    );
    assert!(review.typing_save().is_none());
}

/// Saves fail while a folder stands where the state file's lock goes.
fn break_saves(pull: &Pull) -> PathBuf {
    let lock = pull
        .repo
        .home()
        .join(".guided-review/state/acme/shop/7.json.lock");
    std::fs::remove_file(&lock).ok();
    std::fs::create_dir_all(&lock).unwrap();
    lock
}

#[test]
fn typed_text_whose_save_failed_is_saved_again_by_the_next_save() {
    let pull = Pull::new();
    let mut review = pull.open();
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();
    let lock = break_saves(&pull);

    assert!(review.typing_save().unwrap().run().is_err());
    assert!(matches!(
        review.problems().as_slice(),
        [Problem::NotSaved(reason)] if reason.contains("could not create")
    ));

    std::fs::remove_dir(&lock).unwrap();
    review.typing_save().unwrap().run().unwrap();
    assert_eq!(
        pull.open().text_of(&TextBox::Reply(ThreadId::new("T1"))),
        Some("because")
    );
    assert_eq!(review.problems(), []);
}

#[test]
fn a_read_tick_that_could_not_be_saved_says_why_until_a_save_goes_through() {
    let pull = Pull::new();
    let mut review = pull.open();
    let lock = break_saves(&pull);

    assert!(review.tick(&"notes".into()).is_err());
    assert!(matches!(
        review.problems().as_slice(),
        [Problem::NotSaved(_)]
    ));

    std::fs::remove_dir(&lock).unwrap();
    review.untick(&"notes".into()).unwrap();
    assert_eq!(review.problems(), []);
}

#[test]
fn a_reply_sends_its_draft_as_last_typed() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "becau")
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "because")
        .unwrap();

    let result = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: ThreadId::new("T1"),
            mode: SendMode::AddToReview,
            audience: Audience::Author,
        },
        &github,
    );

    assert_eq!(result, ThreadActionResult::Done);
    assert!(pull.sent.github_calls().iter().any(|call| matches!(
        call,
        GitHubCall::Reply { body, .. } if body == "because"
    )));
}

#[test]
fn a_reply_with_no_text_is_refused_before_it_reaches_github() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review
        .open_box(&TextBox::Reply(ThreadId::new("T1")))
        .unwrap();
    review
        .type_in(&TextBox::Reply(ThreadId::new("T1")), "  ")
        .unwrap();
    let calls_before = pull.sent.github_calls();

    let result = review.act_on_thread(
        ThreadAction::Reply {
            thread_id: ThreadId::new("T1"),
            mode: SendMode::CommentNow,
            audience: Audience::Author,
        },
        &github,
    );

    assert_eq!(
        result,
        ThreadActionResult::Refused(CommentRefusal::EmptyReply)
    );
    assert_eq!(
        pull.sent.github_calls(),
        calls_before,
        "GitHub was not asked"
    );
}

#[test]
fn suggest_change_adds_nothing_when_the_commented_lines_are_not_on_the_screen() {
    let pull = Pull::new();
    let mut review = pull.open();
    let place = drag(&mut review, "line 9", "line ten");
    review.go_to_summary();

    assert_eq!(review.suggest_change(&place).unwrap(), None);
    assert_eq!(drafts(&review)[0].body, "");
}

fn mine(id: &str, body: &str) -> ThreadComment {
    ThreadComment {
        by_viewer: true,
        author: "me".into(),
        ..comment(id, body)
    }
}

fn edit_box_of(review: &Review, id: &str) -> Option<String> {
    let step = review.current_step().unwrap();
    step.threads
        .first()
        .unwrap()
        .edit_drafts
        .get(&CommentId::new(id))
        .cloned()
}

fn edit(id: &str) -> ThreadAction {
    ThreadAction::EditComment {
        comment_id: CommentId::new(id),
    }
}

#[test]
fn editing_my_comment_opens_its_text_and_sends_the_new_text_in_its_place() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![mine("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();

    review
        .open_box(&TextBox::Edit(CommentId::new("C1")))
        .unwrap();
    assert_eq!(edit_box_of(&review, "C1").as_deref(), Some("why?"));
    review
        .type_in(&TextBox::Edit(CommentId::new("C1")), "why not?")
        .unwrap();
    let result = review.act_on_thread(edit("C1"), &github);

    assert_eq!(result, ThreadActionResult::Done);
    assert!(pull.sent.github_calls().contains(&GitHubCall::EditComment {
        comment_id: CommentId::new("C1"),
        body: "why not?".into()
    }));
    assert_eq!(shown_thread(&review).comments[0].body, "why not?");
    assert_eq!(edit_box_of(&review, "C1"), None);
}

#[test]
fn a_comment_of_someone_else_cannot_be_opened_for_editing() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![comment("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();

    review
        .open_box(&TextBox::Edit(CommentId::new("C1")))
        .unwrap();

    assert_eq!(edit_box_of(&review, "C1"), None);
}

#[test]
fn an_edit_that_fails_keeps_the_new_text_and_says_why() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![mine("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();
    review
        .open_box(&TextBox::Edit(CommentId::new("C1")))
        .unwrap();
    review
        .type_in(&TextBox::Edit(CommentId::new("C1")), "why not?")
        .unwrap();
    github.inject(GitHubFault::ConnectionLost);

    let result = review.act_on_thread(edit("C1"), &github);

    assert!(
        matches!(&result, ThreadActionResult::Failed(reason) if reason.contains("network is down")),
        "{result:?}"
    );
    assert_eq!(edit_box_of(&review, "C1").as_deref(), Some("why not?"));
    assert_eq!(shown_thread(&review).comments[0].body, "why?");
}

#[test]
fn an_edit_emptied_of_text_is_refused_and_nothing_is_sent() {
    let pull = Pull::new();
    let github = github_with_thread(&pull, thread_on_line_ten(vec![mine("C1", "why?")]));
    let mut review = pull.open();
    review.pull_threads(&github).unwrap();
    review
        .open_box(&TextBox::Edit(CommentId::new("C1")))
        .unwrap();
    review
        .type_in(&TextBox::Edit(CommentId::new("C1")), "  \n")
        .unwrap();

    let result = review.act_on_thread(edit("C1"), &github);

    assert_eq!(
        result,
        ThreadActionResult::Refused(CommentRefusal::EmptyComment)
    );
    assert!(
        !pull
            .sent
            .github_calls()
            .iter()
            .any(|call| matches!(call, GitHubCall::EditComment { .. }))
    );
    assert_eq!(edit_box_of(&review, "C1").as_deref(), Some("  \n"));
}
