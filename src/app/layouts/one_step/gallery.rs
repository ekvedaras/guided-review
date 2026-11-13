//! Snapshots only: surfaces the demo PR never shows (review threads, banners, a comment GitHub
//! refused, richer markdown), drawn from made-up data so a picture can show how they look.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, IntoElement, ParentElement, Styled, div};

use crate::app::components::card::card;
use crate::app::components::chip::notice_band;
use crate::app::components::comment_composer::QueuedCommentNote;
use crate::app::components::markdown_view::{MarkdownView, parsed_markdown};
use crate::app::components::new_commits_banner::{NewCommitsBanner, RepinProgress};
use crate::app::components::palette::{Colors, Tone};
use crate::app::components::thread_view::{ThreadChanges, ThreadDrafts, ThreadElement};
use crate::file_path::FilePath;
use crate::line_number::LineNumber;
use crate::pull_request::Sha;
use crate::viewer::{
    Audience, CommentDraft, CommentId, CommentPlace, LineLocation, OldHead, QueuedComment,
    Reaction, ReactionCount, ReviewThread, SendMode, Side, ThreadComment, ThreadId, ThreadView,
};

const SAMPLE_MARKDOWN: &str = "A note with `inline code`, a [link](https://example.test) and a list:\n\n\
- the first point\n- a second, longer point that wraps onto another line when the card is narrow\n\n\
> A quote from the PR description.\n\n\
```rust\nfn total(order: &Order) -> Money {\n    order.lines.iter().map(Line::price).sum()\n}\n```\n\n\
<details><summary>More detail</summary>\n\nHidden until opened.\n\n</details>\n\n---\n\n1. numbered\n2. items";

pub fn gallery(colors: &Colors) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(NewCommitsBanner::new(
            OldHead {
                tour_head: Sha::new("3f2a9c1d0e4b"),
                pr_head: Sha::new("8b41e07f22aa"),
            },
            RepinProgress::Failed("GitHub did not answer; try again in a moment.".to_owned()),
            |_, _, _| {},
        ))
        .child(
            notice_band(colors, Tone::Warning)
                .child("Waiting for 1 Ask to answer before the PR reopens."),
        )
        .child(
            div()
                .px_6()
                .pt_6()
                .flex()
                .items_start()
                .gap_4()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(thread(ThreadSample::Open))
                        .child(thread(ThreadSample::Folded))
                        .child(queued_comment()),
                )
                .child(
                    card(colors)
                        .w(gpui::px(460.))
                        .flex_none()
                        .px_5()
                        .py_4()
                        .text_sm()
                        .child(MarkdownView::new(
                            "gallery-markdown",
                            parsed_markdown(SAMPLE_MARKDOWN),
                        )),
                ),
        )
        .into_any_element()
}

fn queued_comment() -> QueuedCommentNote {
    QueuedCommentNote::new(
        QueuedComment {
            draft: CommentDraft {
                place: CommentPlace::File(path()),
                body: "Could this keep the old name for one release?".to_owned(),
                written_at: None,
            },
            mode: SendMode::AddToReview,
            audience: Audience::Author,
            failed_because: "GitHub refused the line: it is outside the diff".to_owned(),
        },
        Rc::new(|_, _| {}),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ThreadSample {
    Open,
    Folded,
}

#[expect(
    clippy::expect_used,
    reason = "snapshots only, and the path is a valid literal"
)]
fn path() -> FilePath {
    FilePath::new("src/viewer/settings.rs").expect("a valid sample path")
}

fn comment(id: &str, author: &str, body: &str, reactions: Vec<ReactionCount>) -> ThreadComment {
    ThreadComment {
        avatar_url: None,
        id: CommentId::new(id),
        author: author.to_owned(),
        is_bot: author.ends_with("[bot]"),
        by_viewer: author == "you",
        body: body.to_owned(),
        url: "https://example.test".to_owned(),
        reactions,
    }
}

fn thread(sample: ThreadSample) -> impl IntoElement {
    let is_open = sample == ThreadSample::Open;
    let comments = if is_open {
        vec![
            comment(
                "c1",
                "reviewer",
                "Should a blank `Custom` be refused here, or kept as *no choice*?",
                vec![ReactionCount {
                    reaction: Reaction::ThumbsUp,
                    count: 2,
                    viewer_has_reacted: true,
                }],
            ),
            comment(
                "c2",
                "you",
                "Refused: Save says why under the setting.\n\n> each problem shown under its setting",
                Vec::new(),
            ),
            comment("c4", "you", "Never mind, I see it now.", Vec::new()),
        ]
    } else {
        vec![comment(
            "c3",
            "lint[bot]",
            "This function is long.",
            Vec::new(),
        )]
    };
    let view = ThreadView {
        followed: None,
        thread: ReviewThread {
            id: ThreadId::new(if is_open { "t1" } else { "t2" }),
            is_resolved: is_open,
            is_outdated: !is_open,
            path: path(),
            line: LineNumber::try_from(30_u32).ok(),
            start_line: LineNumber::try_from(28_u32).ok(),
            side: Side::Head,
            is_file_comment: false,
            original_line: LineNumber::try_from(12_u32).ok(),
            original_start_line: None,
            original_commit: None,
            comments,
        },
        is_folded: !is_open,
        anchor: None::<LineLocation>,
        commented_lines: Arc::from(Vec::new()),
        reply_draft: None,
        edit_drafts: BTreeMap::new(),
        delete_asked: is_open.then(|| CommentId::new("c4")),
        hand_off_line: is_open.then(|| Arc::from("The agent is working on it")),
        agent_is_working: is_open,
        failed_hand_off: None,
    };
    ThreadElement::new(
        Arc::new(view),
        ThreadDrafts {
            can_reply_now: true,
            can_ask_agent: true,
            reply: is_open.then(|| "Agreed, and the test names it.".to_owned()),
            edits: BTreeMap::new(),
        },
        ThreadChanges {
            on_reply_draft: Rc::new(|_, _| {}),
            on_comment_edit: Rc::new(|_, _, _| {}),
            on_delete_question: Rc::new(|_, _, _| {}),
            on_toggle: Box::new(|_, _, _| {}),
            on_action: Rc::new(|_, _, _| {}),
            on_try_hand_off_again: Rc::new(|_, _| {}),
        },
    )
}
