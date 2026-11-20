//! What the snapshot run shows: every screen in reading order, top and end, then the overlays and
//! panels over the first step. Each scene is staged through the layout's own methods and undone after
//! its picture, so the next one starts from the plain screen.

use std::sync::Arc;

use gpui::{
    App, AsyncWindowContext, Context, Keystroke, ListOffset, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Task, WeakEntity, Window, point,
    px,
};

use super::OneStepLayout;
use super::issue_card::IssueHover;
use crate::app::components::diff_view::RowGesture;
use crate::app::components::pr_switcher::PrSwitching;
use crate::app::components::toast_stack::{ToastShelf, Toaster};
use crate::app::review_session::{ReviewSession, StagedReReview};
use crate::app::snapshots::{Camera, StagedFinishReview, StagedGallery, sample_pr};
use crate::app::updater::Updates;
use crate::file_path::FilePath;
use crate::findings::FindingId;
use crate::issue::{IssueLink, IssueUrl};
use crate::pull_request::RepoName;
use crate::review_subject::{BranchName, ReviewSubject};
use crate::update::{Build, InTheWay, ReleaseAsset, Target, Update, UpdateOffer, Version};
use crate::viewer::{
    AgentStatus, AiReview, AiReviewRun, AutoMerge, CheckRun, CheckState, Checks, FileView,
    LineKind, LineLocation, MergeMethod, MergeMethods, Mergeability, Merging, OpenPrStatus,
    PageRow, PieceLabels, PrStatus, Problem, ProblemKind, Readiness, TerminalId, TextBox,
};
use crate::viewer::{HeadBranch, MergeRoute, PrAuthor, PullRequestOrigin};
use crate::viewer::{IssueDetails, IssueState, RelatedIssue};

const MAX_SCREENS: usize = 16;
/// Not the PR on screen, so the veil and the loading screen have another PR to name.
const OTHER_PR: u32 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scene {
    Settings,
    JumpPalette,
    PrSwitcher,
    FinishReview,
    CommentComposer,
    AskPanel,
    SwitchingVeil,
    LoadingScreen,
    Gallery,
    SettingsEdited,
    SettingsEditedEnd,
    Shortcuts,
    JumpActions,
    StickyStepBar,
    Toasts,
    AllRead,
    DescriptionFolds,
    PictureZoom,
    UpdateOffered,
    ReleaseInstallInTheWay,
    RenderedMarkdown,
    AgentLinkOff,
    GitHubOutage,
    RefreshRunning,
    PinnedFileHeader,
    SettingsKeys,
    FocusedChange,
    SettingsMoreLines,
    SettingsComments,
    CommentComposerNowPrimary,
    PieceLabelsShown,
    PieceLabelsHidden,
    BasicOverview,
    BasicOverviewFiles,
    BasicFile,
    BasicRead,
    BasicGuidedReady,
    SwitchedSummary,
    LocalOverview,
    LocalFile,
    LocalChangedOnDisk,
    LocalMoveOffer,
    LocalTaskSendAnyway,
    TitleBarDraftChecksRunning,
    TitleBarReadyChecksFailing,
    TitleBarAutoMergeOff,
    TitleBarAutoMergeOn,
    TitleBarMergeNow,
    TitleBarChecksList,
    TitleBarIssue,
    TitleBarIssueCard,
    TitleBarIssueList,
    TitleBarMerged,
    TitleBarClosed,
    TitleBarPrOrigin,
    PrSwitcherEnd,
    ReReview,
    ReReviewDone,
    TourEndUnread,
    AiReviewFound,
    AiReviewRunning,
    AiReviewFailed,
    AiReviewOnLines,
    AiReviewFindingsList,
    AiReviewFixRequests,
    AiReviewAskWhy,
    AiReviewOutdated,
    AiReviewThreadFromFinding,
    BasicGuidedWriting,
}

impl Scene {
    const ALL: [Self; 69] = [
        Self::Settings,
        Self::JumpPalette,
        Self::PrSwitcher,
        Self::FinishReview,
        Self::CommentComposer,
        Self::AskPanel,
        Self::SwitchingVeil,
        Self::LoadingScreen,
        Self::Gallery,
        Self::SettingsEdited,
        Self::SettingsEditedEnd,
        Self::Shortcuts,
        Self::JumpActions,
        Self::StickyStepBar,
        Self::Toasts,
        Self::AllRead,
        Self::DescriptionFolds,
        Self::PictureZoom,
        Self::UpdateOffered,
        Self::ReleaseInstallInTheWay,
        Self::RenderedMarkdown,
        Self::AgentLinkOff,
        Self::GitHubOutage,
        Self::RefreshRunning,
        Self::PinnedFileHeader,
        Self::SettingsKeys,
        Self::FocusedChange,
        Self::SettingsMoreLines,
        Self::SettingsComments,
        Self::CommentComposerNowPrimary,
        Self::PieceLabelsShown,
        Self::PieceLabelsHidden,
        Self::BasicOverview,
        Self::BasicOverviewFiles,
        Self::BasicFile,
        Self::BasicRead,
        Self::BasicGuidedReady,
        Self::SwitchedSummary,
        Self::LocalOverview,
        Self::LocalFile,
        Self::LocalChangedOnDisk,
        Self::LocalMoveOffer,
        Self::LocalTaskSendAnyway,
        Self::TitleBarDraftChecksRunning,
        Self::TitleBarReadyChecksFailing,
        Self::TitleBarAutoMergeOff,
        Self::TitleBarAutoMergeOn,
        Self::TitleBarMergeNow,
        Self::TitleBarChecksList,
        Self::TitleBarIssue,
        Self::TitleBarIssueCard,
        Self::TitleBarIssueList,
        Self::TitleBarMerged,
        Self::TitleBarClosed,
        Self::TitleBarPrOrigin,
        Self::PrSwitcherEnd,
        Self::ReReview,
        Self::ReReviewDone,
        Self::TourEndUnread,
        Self::AiReviewFound,
        Self::AiReviewRunning,
        Self::AiReviewFailed,
        Self::AiReviewOnLines,
        Self::AiReviewFindingsList,
        Self::AiReviewFixRequests,
        Self::AiReviewAskWhy,
        Self::AiReviewOutdated,
        Self::AiReviewThreadFromFinding,
        Self::BasicGuidedWriting,
    ];

    fn file_name(self) -> &'static str {
        match self {
            Self::Settings => "20-settings",
            Self::JumpPalette => "21-jump-palette",
            Self::PrSwitcher => "22-pr-switcher",
            Self::FinishReview => "23-finish-review",
            Self::CommentComposer => "24-comment-composer",
            Self::AskPanel => "25-ask-panel",
            Self::SwitchingVeil => "26-switching-veil",
            Self::LoadingScreen => "27-loading-screen",
            Self::Gallery => "28-gallery",
            Self::SettingsEdited => "29-settings-edited",
            Self::SettingsEditedEnd => "30-settings-edited-end",
            Self::Shortcuts => "31-shortcuts",
            Self::JumpActions => "32-jump-actions",
            Self::StickyStepBar => "33-sticky-step-bar",
            Self::Toasts => "34-toasts",
            Self::AllRead => "35-all-read",
            Self::DescriptionFolds => "36-description-folds",
            Self::PictureZoom => "40-picture-zoom",
            Self::UpdateOffered => "39-update-offered",
            Self::ReleaseInstallInTheWay => "43-release-install-in-the-way",
            Self::RenderedMarkdown => "41-rendered-markdown",
            Self::AgentLinkOff => "42-agent-link-off",
            Self::GitHubOutage => "44-github-outage",
            Self::RefreshRunning => "45-refresh-running",
            Self::PinnedFileHeader => "46-pinned-file-header",
            Self::SettingsKeys => "47-settings-keys",
            Self::FocusedChange => "48-focused-change",
            Self::SettingsMoreLines => "49-settings-more-lines",
            Self::PieceLabelsShown => "50-piece-labels",
            Self::PieceLabelsHidden => "51-piece-labels-hidden",
            Self::BasicOverview => "52-basic-overview",
            Self::BasicOverviewFiles => "54-basic-overview-files",
            Self::BasicFile => "53-basic-file",
            Self::BasicRead => "55-basic-read",
            Self::BasicGuidedReady => "56-basic-guided-ready",
            Self::SwitchedSummary => "57-switched-summary",
            Self::LocalOverview => "58-local-overview",
            Self::LocalFile => "59-local-file",
            Self::LocalChangedOnDisk => "60-local-changed-on-disk",
            Self::LocalMoveOffer => "61-local-move-to-pr",
            Self::LocalTaskSendAnyway => "62-local-task-send-anyway",
            Self::TitleBarDraftChecksRunning => "63-title-bar-draft-checks-running",
            Self::TitleBarReadyChecksFailing => "64-title-bar-ready-checks-failing",
            Self::TitleBarAutoMergeOff => "65-title-bar-auto-merge-off",
            Self::TitleBarAutoMergeOn => "66-title-bar-auto-merge-on",
            Self::TitleBarMergeNow => "67-title-bar-merge-now",
            Self::TitleBarChecksList => "68-title-bar-checks-list",
            Self::TitleBarIssue => "69-title-bar-issue",
            Self::TitleBarIssueCard => "70-title-bar-issue-card",
            Self::TitleBarIssueList => "71-title-bar-issue-list",
            Self::TitleBarMerged => "72-title-bar-merged",
            Self::TitleBarClosed => "73-title-bar-closed",
            Self::TitleBarPrOrigin => "74-title-bar-pr-origin",
            Self::PrSwitcherEnd => "75-pr-switcher-end",
            Self::ReReview => "77-re-review",
            Self::ReReviewDone => "78-re-review-done",
            Self::TourEndUnread => "79-tour-end-unread",
            Self::AiReviewFound => "80-ai-review-found",
            Self::AiReviewRunning => "81-ai-review-running",
            Self::AiReviewFailed => "82-ai-review-failed",
            Self::AiReviewOnLines => "83-ai-review-on-lines",
            Self::AiReviewFindingsList => "84-ai-review-findings-list",
            Self::AiReviewFixRequests => "85-ai-review-fix-requests",
            Self::AiReviewAskWhy => "86-ai-review-ask-why",
            Self::AiReviewOutdated => "87-ai-review-outdated",
            Self::AiReviewThreadFromFinding => "88-ai-review-thread-from-finding",
            Self::BasicGuidedWriting => "89-basic-guided-writing",
            Self::SettingsComments => "90-settings-comments",
            Self::CommentComposerNowPrimary => "91-comment-composer-now-primary",
        }
    }

    /// The issues a title bar scene shows, made up on the sample PR's repo; empty for every other scene.
    fn staged_issues(self) -> Vec<(RelatedIssue, Option<IssueDetails>)> {
        let Ok(repo) = "ekvedaras/guided-review".parse::<RepoName>() else {
            return Vec::new();
        };
        let link = |number| IssueLink::GitHub(IssueUrl::new(repo.clone(), number));
        let titled = |number, title: &str| {
            (
                RelatedIssue {
                    link: link(number),
                    title: Some(title.into()),
                },
                None,
            )
        };
        match self {
            Self::TitleBarIssue => vec![titled(9001, "Show the issue a review is about")],
            Self::TitleBarIssueCard => {
                let title = "Show the issue a review is about in the title bar";
                let details = IssueDetails {
                    title: title.into(),
                    state: IssueState::Open,
                    author: Some("octocat".into()),
                    labels: ["enhancement", "title bar"].map(Into::into).into(),
                    assignees: ["mona", "hubot"].map(Into::into).into(),
                    body: IssueDetails::markdown_of(ISSUE_BODY),
                };
                vec![(
                    RelatedIssue {
                        link: link(9001),
                        title: Some(title.into()),
                    },
                    Some(details),
                )]
            }
            Self::TitleBarIssueList => vec![
                titled(9001, "Show the issue a review is about"),
                titled(9002, "Name the issue in the PR description"),
                titled(9003, "List several issues when a PR closes more than one"),
            ],
            _ => Vec::new(),
        }
    }

    /// The author and branches a title bar scene shows; a letter stands in for the picture, which would be a download.
    fn staged_origin() -> Option<PullRequestOrigin> {
        Some(PullRequestOrigin {
            author: Some(PrAuthor {
                login: "octocat".into(),
                avatar_url: None,
            }),
            route: MergeRoute {
                base: BranchName::new("release/2.x").ok()?,
                head: HeadBranch::of_repo(BranchName::new("feature/show-the-pr-author").ok()?),
            },
        })
    }

    /// The PR status a title bar scene shows; `None` for every other scene.
    fn staged_status(self) -> Option<PrStatus> {
        let blocked = Merging {
            mergeability: Mergeability::Blocked,
            auto_merge: AutoMerge::Off,
            methods: MergeMethods::only(&MergeMethod::ALL),
            repo_allows_auto_merge: true,
            viewer_may_enable_auto_merge: true,
            viewer_may_disable_auto_merge: true,
            viewer_may_merge: true,
        };
        let cannot_act = Merging {
            viewer_may_enable_auto_merge: false,
            ..blocked
        };
        let open = |readiness, checks, merging| OpenPrStatus {
            readiness,
            checks,
            check_runs: Arc::default(),
            viewer_may_change_readiness: true,
            merging,
        };
        let status = |readiness, checks, merging| PrStatus::Open(open(readiness, checks, merging));
        Some(match self {
            Self::TitleBarDraftChecksRunning => {
                status(Readiness::Draft, Checks::Running, cannot_act)
            }
            Self::TitleBarReadyChecksFailing => {
                status(Readiness::Ready, Checks::Failing, cannot_act)
            }
            Self::TitleBarAutoMergeOff => status(Readiness::Ready, Checks::Running, blocked),
            Self::TitleBarAutoMergeOn => status(
                Readiness::Ready,
                Checks::Running,
                Merging {
                    auto_merge: AutoMerge::On(MergeMethod::Squash),
                    ..blocked
                },
            ),
            Self::TitleBarMergeNow => status(
                Readiness::Ready,
                Checks::Passing,
                Merging {
                    mergeability: Mergeability::Now,
                    ..blocked
                },
            ),
            Self::TitleBarChecksList => PrStatus::Open(OpenPrStatus {
                check_runs: [
                    ("lint", CheckState::Passing),
                    ("test (ubuntu-latest)", CheckState::Failing),
                    ("deploy preview", CheckState::Skipped),
                    ("test (macos-latest)", CheckState::Running),
                ]
                .map(|(name, state)| CheckRun {
                    name: name.into(),
                    state,
                    page: None,
                })
                .into(),
                ..open(Readiness::Ready, Checks::Failing, cannot_act)
            }),
            Self::TitleBarMerged => PrStatus::Merged,
            Self::TitleBarClosed => PrStatus::Closed,
            _ => return None,
        })
    }
}

pub(super) fn start(window: &Window, cx: &mut Context<OneStepLayout>) -> Option<Task<()>> {
    let camera = Camera::from_env()?;
    Some(cx.spawn_in(window, async move |layout, cx| {
        if let Err(error) = shoot(&camera, &layout, cx).await {
            eprintln!("snapshots stopped: {error:#}");
            camera.note_stop(&error);
        }
        cx.update(|_, cx| cx.quit()).ok();
    }))
}

async fn shoot(
    camera: &Camera,
    layout: &WeakEntity<OneStepLayout>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    for screen in 0..MAX_SCREENS {
        layout.update(cx, |layout, _| layout.scroll_to_row(0))?;
        camera.take(&format!("{screen:02}-top"), cx).await?;
        layout.update(cx, |layout, _| {
            layout.scroll_to_row(layout.screen_list.item_count());
        })?;
        camera.take(&format!("{screen:02}-end"), cx).await?;
        if !layout.update(cx, |layout, cx| {
            layout.change_review(cx, |review| review.go_forward())
        })? {
            break;
        }
    }
    camera.wait_for_loading(cx).await?;
    layout.update(cx, OneStepLayout::go_to_first_open_code)?;
    for scene in Scene::ALL {
        layout.update_in(cx, |layout, window, cx| layout.stage(scene, window, cx))?;
        camera.take(scene.file_name(), cx).await?;
        layout.update_in(cx, |layout, window, cx| layout.unstage(scene, window, cx))?;
    }
    drag_the_issue_cards_scrollbar(camera, layout, cx).await?;
    pick_a_word_and_copy(camera, layout, cx).await
}

/// The issue card's description dragged down by its scrollbar, as the mouse gives it: pressed on the
/// thumb, moved down with the button held, then let go on the card, which stays.
async fn drag_the_issue_cards_scrollbar(
    camera: &Camera,
    layout: &WeakEntity<OneStepLayout>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    layout.update_in(cx, |layout, window, cx| {
        layout.stage(Scene::TitleBarIssueCard, window, cx);
    })?;
    camera.take("76-issue-card-before-drag", cx).await?;
    let body = layout.update(cx, |layout, _| layout.issue_card_scroll.bounds())?;
    let press = point(body.right() - px(6.), body.top() + px(20.));
    let to = point(press.x, press.y + px(120.));
    cx.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: press,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: press,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        // A frame between, as the screen gives: a drag listens for moves from the frame after the press.
        window.refresh();
        window.draw(cx).clear(cx);
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: to,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        window.refresh();
        window.draw(cx).clear(cx);
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: to,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            cx,
        );
    })?;
    camera.take("76-issue-card-dragged", cx).await?;
    layout.update_in(cx, |layout, window, cx| {
        layout.unstage(Scene::TitleBarIssueCard, window, cx);
    })?;
    Ok(())
}

/// With a comment box open on the page, a double click on the first added line's code on screen, as the
/// mouse gives it, then Primary+C: the word is selected, and the copy says so.
async fn pick_a_word_and_copy(
    camera: &Camera,
    layout: &WeakEntity<OneStepLayout>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<()> {
    layout.update(cx, OneStepLayout::go_to_first_open_code)?;
    layout.update_in(cx, OneStepLayout::compose_on_first_added_lines)?;
    camera.take("37-before-word", cx).await?;
    let spot = layout.update(cx, |layout, cx| {
        let bounds = layout
            .added_lines(usize::MAX, cx)
            .into_iter()
            .find_map(|(row, _)| layout.screen_list.bounds_for_item(row))?;
        Some(point(bounds.left() + px(205.), bounds.center().y))
    })?;
    let spot = spot.ok_or_else(|| anyhow::anyhow!("no added line on screen to double click"))?;
    // Through the window, not the layout: the layout handles these events and must not be held.
    cx.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: spot,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        for click_count in [1, 2] {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: spot,
                    modifiers: Modifiers::default(),
                    click_count,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: spot,
                    modifiers: Modifiers::default(),
                    click_count,
                }),
                cx,
            );
        }
    })?;
    camera.take("37-word", cx).await?;
    cx.update(|window, cx| {
        if let Ok(keys) = Keystroke::parse("secondary-c") {
            window.dispatch_keystroke(keys, cx);
        }
    })?;
    camera.take("38-copied", cx).await
}

impl OneStepLayout {
    fn scroll_to_row(&self, row: usize) {
        self.screen_list.scroll_to(ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
    }

    /// The overlays sit over a step whose code is open, so the comment box has lines to go on.
    fn go_to_first_open_code(&mut self, cx: &mut Context<Self>) {
        self.change_review(cx, |review| review.go_to_summary());
        for _ in 0..MAX_SCREENS {
            if !self.change_review(cx, |review| review.go_forward())
                || !self.added_lines(1, cx).is_empty()
            {
                break;
            }
        }
        self.scroll_to_row(0);
    }

    /// The first screen with a Markdown file's box, for its rendered view; `None` when no screen has one.
    fn go_to_first_markdown_box(&mut self, cx: &mut Context<Self>) -> Option<FilePath> {
        self.change_review(cx, |review| review.go_to_summary());
        for _ in 0..MAX_SCREENS {
            if let Some(path) = self.markdown_box(cx) {
                return Some(path);
            }
            if !self.change_review(cx, |review| review.go_forward()) {
                break;
            }
        }
        None
    }

    fn markdown_box(&self, cx: &App) -> Option<FilePath> {
        let rows = self.session.read(cx).rows();
        (0..rows.len()).find_map(|index| match rows.row(index)? {
            PageRow::PieceHeader(heading) => heading
                .view
                .as_ref()
                .filter(|switch| {
                    std::path::Path::new(switch.path.as_str())
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
                })
                .map(|switch| switch.path.clone()),
            _ => None,
        })
    }

    /// A skim step folds its boxes; the scene shows the rendered file, so its box is opened.
    fn open_box(&mut self, path: &FilePath, cx: &mut Context<Self>) {
        let fold = self.row_of_box(path, cx).and_then(|row| {
            match self.session.read(cx).rows().row(row)? {
                PageRow::PieceHeader(heading) => heading.fold.clone(),
                _ => None,
            }
        });
        if let Some(fold) = fold.filter(|fold| fold.is_folded) {
            self.change_review(cx, |review| {
                review.toggle_box_fold(&fold.screen, &fold.path);
            });
        }
    }

    /// The first file box with piece labels shows them or not, and the page goes to its header.
    fn stage_piece_labels(&mut self, labels: PieceLabels, cx: &mut Context<Self>) {
        self.open_every_box(cx);
        if self.first_box_with_labels(cx).is_none() {
            self.change_review(cx, |review| review.go_to_summary());
            for _ in 0..MAX_SCREENS {
                if !self.change_review(cx, |review| review.go_forward()) {
                    break;
                }
                self.open_every_box(cx);
                if self.first_box_with_labels(cx).is_some() {
                    break;
                }
            }
        }
        let Some((_, path)) = self.first_box_with_labels(cx) else {
            eprintln!("no file box with piece labels on this screen");
            return;
        };
        self.change_review(cx, |review| {
            review
                .display_options_mut()
                .show_piece_labels(&path, labels);
        });
        if let Some((row, _)) = self.first_box_with_labels(cx) {
            self.scroll_to_row(row.saturating_sub(1));
        }
    }

    /// A read or skim step folds its boxes, and a folded box has no labels to toggle.
    fn open_every_box(&mut self, cx: &mut Context<Self>) {
        let folded: Vec<_> = {
            let rows = self.session.read(cx).rows();
            (0..rows.len())
                .filter_map(|index| match rows.row(index)? {
                    PageRow::PieceHeader(heading) => heading.fold.clone(),
                    _ => None,
                })
                .filter(|fold| fold.is_folded)
                .collect()
        };
        for fold in folded {
            self.change_review(cx, |review| {
                review.toggle_box_fold(&fold.screen, &fold.path);
            });
        }
    }

    fn first_box_with_labels(&self, cx: &App) -> Option<(usize, FilePath)> {
        let rows = self.session.read(cx).rows();
        (0..rows.len()).find_map(|index| match rows.row(index)? {
            PageRow::PieceHeader(heading) => heading
                .labels
                .as_ref()
                .map(|switch| (index, switch.path.clone())),
            _ => None,
        })
    }

    /// A symbol's label with code under it, so the pinned header has a symbol to name.
    fn first_marker_over_code(&self, cx: &App) -> Option<usize> {
        let rows = self.session.read(cx).rows();
        (0..rows.len()).find(|index| {
            matches!(rows.row(*index), Some(PageRow::PieceMarker(_)))
                && matches!(rows.row(index + 1), Some(PageRow::Line(_)))
        })
    }

    fn row_of_box(&self, path: &FilePath, cx: &App) -> Option<usize> {
        let rows = self.session.read(cx).rows();
        (0..rows.len()).find(|index| {
            matches!(rows.row(*index), Some(PageRow::PieceHeader(heading)) if heading.path == *path)
        })
    }

    fn added_lines(&self, count: usize, cx: &App) -> Vec<(usize, LineLocation)> {
        let rows = self.session.read(cx).rows();
        (0..rows.len())
            .filter_map(|index| {
                let line = rows.line(index)?;
                (line.kind == LineKind::Added).then(|| (index, line.location.clone()))
            })
            .take(count)
            .collect()
    }

    fn stage(&mut self, scene: Scene, window: &mut Window, cx: &mut Context<Self>) {
        match scene {
            Scene::Settings => self
                .settings
                .update(cx, |settings, cx| settings.open(window, cx)),
            Scene::JumpPalette => {
                let groups = self.session.read(cx).review().palette("", Build::running());
                self.jump
                    .update(cx, |palette, cx| palette.open(groups, window, cx));
            }
            Scene::PrSwitcher => {
                ReviewSession::stage_local_review_on_disk(cx);
                self.pr_switcher
                    .update(cx, |switcher, cx| switcher.open(window, cx));
            }
            Scene::PrSwitcherEnd => {
                ReviewSession::stage_local_review_on_disk(cx);
                self.pr_switcher
                    .update(cx, |switcher, cx| switcher.open_at_end(window, cx));
            }
            Scene::FinishReview => {
                self.session.update(cx, |session, cx| {
                    session.type_in(
                        &TextBox::FinishBody,
                        "Reads well. Two small questions inline, nothing blocking.",
                        cx,
                    );
                });
                cx.set_global(StagedFinishReview(true));
            }
            Scene::CommentComposer => self.compose_on_first_added_lines(window, cx),
            Scene::CommentComposerNowPrimary => {
                if let Ok(keymap) =
                    crate::config::Config::from_text("primary_comment_action = \"comment_now\"\n")
                        .map(|config| config.resolved_keymap().keymap)
                {
                    crate::app::keymap::set_keymap(keymap, cx);
                }
                self.compose_on_first_added_lines(window, cx);
            }
            Scene::AskPanel => self.ask_about_screen(window, cx),
            Scene::SwitchingVeil => {
                if let Some(other) = sample_pr(OTHER_PR).map(ReviewSubject::from) {
                    let active = cx.global::<PrSwitching>().active.clone();
                    active.update(cx, |active, cx| {
                        active.picked(other);
                        cx.notify();
                    });
                }
            }
            Scene::LoadingScreen => {
                if let Some(other) = sample_pr(OTHER_PR).map(ReviewSubject::from) {
                    let active = cx.global::<PrSwitching>().active.clone();
                    active.update(cx, |active, cx| {
                        active.write(other.clone());
                        active.writer_started(&other, Ok(TerminalId::new("w2:t4")));
                        active.writer_status(&other, AgentStatus::Working);
                        active.writer_screen(&other, sample_agent_screen());
                        cx.notify();
                    });
                }
            }
            Scene::Gallery => cx.set_global(StagedGallery(true)),
            Scene::SettingsEdited => self.settings.update(cx, |settings, cx| {
                settings.open(window, cx);
                settings.stage_sample_edits(cx);
            }),
            Scene::SettingsEditedEnd => self.settings.update(cx, |settings, cx| {
                settings.open(window, cx);
                settings.stage_sample_edits(cx);
                settings.scroll_to_end();
            }),
            Scene::SettingsKeys => self.settings.update(cx, |settings, cx| {
                settings.open(window, cx);
                settings.stage_sample_keys(cx);
                settings.scroll_to_keys();
            }),
            Scene::SettingsMoreLines => self.settings.update(cx, |settings, cx| {
                settings.open(window, cx);
                settings.stage_sample_edits(cx);
                settings.scroll_to_more_lines();
            }),
            Scene::SettingsComments => self.settings.update(cx, |settings, cx| {
                settings.open(window, cx);
                settings.stage_comment_now_primary(cx);
                settings.scroll_to_comments();
            }),
            Scene::Shortcuts => self
                .shortcuts
                .update(cx, |sheet, cx| sheet.toggle(window, cx)),
            Scene::StickyStepBar => {
                let rows = self.screen_list.item_count();
                self.scroll_to_row(rows / 2);
            }
            Scene::PieceLabelsShown => self.stage_piece_labels(PieceLabels::Shown, cx),
            Scene::PieceLabelsHidden => self.stage_piece_labels(PieceLabels::Hidden, cx),
            Scene::PinnedFileHeader => {
                self.stage_piece_labels(PieceLabels::Shown, cx);
                let row = self
                    .first_marker_over_code(cx)
                    .unwrap_or_else(|| self.screen_list.item_count() / 2);
                self.scroll_to_row(row);
            }
            Scene::FocusedChange => {
                let row = self.change_review(cx, |review| {
                    review.focus_next_change();
                    review.focus_next_change()
                });
                self.scroll_to_row(row.map_or(0, |row| row.saturating_sub(6)));
            }
            Scene::Toasts => {
                if let Some(toaster) = cx.try_global::<Toaster>().cloned() {
                    toaster.0.update(cx, ToastShelf::stage_samples);
                }
            }
            Scene::AllRead => {
                self.change_review(cx, |review| while review.go_forward() {});
                self.session.update(cx, ReviewSession::stage_all_read);
                self.scroll_to_row(self.screen_list.item_count());
            }
            Scene::ReReview => self.session.update(cx, |session, cx| {
                session.stage_re_review(StagedReReview::Open, cx);
            }),
            Scene::ReReviewDone => self.session.update(cx, |session, cx| {
                session.stage_re_review(StagedReReview::MarkedDone, cx);
            }),
            Scene::AiReviewFound => self.change_review(cx, |review| {
                review.ai_review_is(AiReview::staged());
            }),
            Scene::AiReviewRunning => self.change_review(cx, |review| {
                review.ai_review_run_is(AiReviewRun::staged_running());
            }),
            Scene::AiReviewFailed => self.change_review(cx, |review| {
                review.ai_review_run_is(AiReviewRun::staged_failed());
            }),
            Scene::AiReviewFindingsList => {
                self.change_review(cx, |review| {
                    review.ai_review_is(AiReview::staged());
                    review.stage_dismissed_finding();
                });
                let list = self.session.read(cx).review().findings_list();
                self.findings.update(cx, |sheet, cx| {
                    sheet.toggle(list, window, cx);
                    sheet.stage_sample(cx);
                    sheet.stage_dismissed_shown(cx);
                });
            }
            Scene::AiReviewOnLines => self.change_review(cx, |review| {
                review.go_forward();
                let first_line = review
                    .page()
                    .lines()
                    .next()
                    .map(|line| line.location.clone());
                if let Some(at) = first_line {
                    review.ai_review_is(AiReview::staged_on(&at.path, at.line));
                }
            }),
            Scene::AiReviewFixRequests => self.change_review(cx, |review| {
                review.go_forward();
                let first_line = review
                    .page()
                    .lines()
                    .next()
                    .map(|line| line.location.clone());
                if let Some(at) = first_line {
                    review.ai_review_is(AiReview::staged_on(&at.path, at.line));
                    review.stage_fix_requests();
                }
            }),
            Scene::AiReviewThreadFromFinding => self.change_review(cx, |review| {
                review.go_forward();
                let first_line = review
                    .page()
                    .lines()
                    .next()
                    .map(|line| line.location.clone());
                if let Some(at) = first_line {
                    review.ai_review_is(AiReview::staged_on(&at.path, at.line));
                    review.stage_thread_from_finding();
                }
            }),
            Scene::AiReviewOutdated => self.change_review(cx, |review| {
                review.go_forward();
                let first_line = review
                    .page()
                    .lines()
                    .next()
                    .map(|line| line.location.clone());
                if let Some(at) = first_line {
                    review.ai_review_is(AiReview::staged_outdated_on(&at.path, at.line));
                }
            }),
            Scene::AiReviewAskWhy => {
                self.change_review(cx, |review| {
                    review.go_forward();
                    let first_line = review
                        .page()
                        .lines()
                        .next()
                        .map(|line| line.location.clone());
                    if let Some(at) = first_line {
                        review.ai_review_is(AiReview::staged_on(&at.path, at.line));
                        review.stage_finding_threads();
                    }
                });
                if let Ok(answered) = FindingId::try_from("a".to_owned()) {
                    self.finding_questions
                        .open(&answered, "Is it on purpose?", "to ask", cx);
                }
            }
            Scene::TourEndUnread => {
                self.change_review(cx, |review| while review.go_forward() {});
                self.session.update(cx, ReviewSession::stage_unread_end);
                self.scroll_to_row(self.screen_list.item_count());
            }
            Scene::DescriptionFolds => {
                self.change_review(cx, |review| review.go_to_summary());
                self.toggle_description_folds(cx);
                if let Some(row) = self.session.read(cx).rows().description_row() {
                    self.scroll_to_row(row);
                }
            }
            Scene::PictureZoom => {
                self.change_review(cx, |review| review.go_to_summary());
                if let Some(picture) = self.session.read(cx).rows().first_summary_picture() {
                    self.picture_zoom
                        .update(cx, |zoom, cx| zoom.open(picture, None, window, cx));
                }
            }
            Scene::UpdateOffered => {
                let folders = cx.global::<PrSwitching>().folders.clone();
                if let Some(update) = sample_update() {
                    Updates::stage(folders.home(), UpdateOffer::Available(update), cx);
                }
            }
            Scene::ReleaseInstallInTheWay => self.change_review(cx, |review| {
                review.raise_problem(Problem::ReleaseInstallInTheWay(InTheWay::FirstOnPath {
                    binary: "/Users/you/.cargo/bin/guided-review".into(),
                    release_install: "/Users/you/.local/bin/guided-review".into(),
                }));
            }),
            Scene::RenderedMarkdown => {
                if let Some(path) = self.go_to_first_markdown_box(cx) {
                    self.change_review(cx, |review| {
                        review
                            .display_options_mut()
                            .show_file_as(&path, FileView::Rendered);
                    });
                    self.open_box(&path, cx);
                    if let Some(row) = self.row_of_box(&path, cx) {
                        self.scroll_to_row(row);
                    }
                }
            }
            Scene::BasicOverview => self.session.update(cx, ReviewSession::stage_basic),
            Scene::BasicOverviewFiles => {
                self.session.update(cx, ReviewSession::stage_basic);
                self.scroll_to_row(self.screen_list.item_count());
            }
            Scene::BasicFile => {
                self.session.update(cx, ReviewSession::stage_basic);
                self.change_review(cx, |review| review.go_forward());
            }
            Scene::LocalOverview => self.session.update(cx, ReviewSession::stage_local),
            Scene::LocalChangedOnDisk => self
                .session
                .update(cx, ReviewSession::stage_local_changed_on_disk),
            Scene::LocalMoveOffer => self
                .session
                .update(cx, ReviewSession::stage_local_move_offer),
            Scene::LocalFile => {
                self.session
                    .update(cx, ReviewSession::stage_local_with_task);
                self.change_review(cx, |review| {
                    if let Ok(path) = FilePath::new("README.md") {
                        review.go_to_file(&path);
                    }
                });
            }
            Scene::LocalTaskSendAnyway => {
                self.session
                    .update(cx, ReviewSession::stage_local_with_held_back_task);
                self.change_review(cx, |review| {
                    if let Ok(path) = FilePath::new("README.md") {
                        review.go_to_file(&path);
                    }
                });
            }
            Scene::BasicRead => self.session.update(cx, ReviewSession::stage_basic_read),
            Scene::BasicGuidedReady => self
                .session
                .update(cx, ReviewSession::stage_basic_guided_ready),
            Scene::BasicGuidedWriting => self
                .session
                .update(cx, ReviewSession::stage_basic_guided_writing),
            Scene::SwitchedSummary => self.change_review(cx, |review| {
                review.go_to_summary();
                review.steps_carried_read_ticks_are(3);
            }),
            Scene::AgentLinkOff => self.session.update(cx, ReviewSession::stage_agent_link_off),
            Scene::GitHubOutage => self.session.update(cx, ReviewSession::stage_outage),
            Scene::RefreshRunning => {
                self.scroll_to_row(0);
                self.session
                    .update(cx, ReviewSession::stage_refresh_running);
            }
            Scene::TitleBarDraftChecksRunning
            | Scene::TitleBarReadyChecksFailing
            | Scene::TitleBarAutoMergeOff
            | Scene::TitleBarAutoMergeOn
            | Scene::TitleBarMergeNow
            | Scene::TitleBarChecksList
            | Scene::TitleBarMerged
            | Scene::TitleBarClosed => {
                if let Some(status) = scene.staged_status() {
                    self.session
                        .update(cx, |session, cx| session.stage_pr_status(status, cx));
                }
                self.checks_hover.chip = scene == Scene::TitleBarChecksList;
            }
            Scene::TitleBarIssue | Scene::TitleBarIssueCard | Scene::TitleBarIssueList => {
                let issues = scene.staged_issues();
                self.session.update(cx, |session, cx| {
                    session.stage_related_issues(issues, cx);
                });
                self.issue_hover.target = (scene == Scene::TitleBarIssueCard).then_some(0);
                self.issue_list_open = scene == Scene::TitleBarIssueList;
            }
            Scene::TitleBarPrOrigin => {
                if let Some(origin) = Scene::staged_origin() {
                    self.session
                        .update(cx, |session, cx| session.stage_pr_origin(origin, cx));
                }
            }
            Scene::JumpActions => {
                let query = ">";
                let groups = self
                    .session
                    .read(cx)
                    .review()
                    .palette(query, Build::running());
                self.jump.update(cx, |palette, cx| {
                    palette.open(Vec::new(), window, cx);
                    palette.stage_query(query, groups);
                });
            }
        }
        cx.notify();
    }

    fn unstage(&mut self, scene: Scene, window: &mut Window, cx: &mut Context<Self>) {
        match scene {
            Scene::Settings
            | Scene::SettingsEdited
            | Scene::SettingsEditedEnd
            | Scene::SettingsKeys
            | Scene::SettingsMoreLines
            | Scene::SettingsComments => self
                .settings
                .update(cx, |settings, cx| settings.close(window, cx)),
            Scene::JumpPalette | Scene::JumpActions => self
                .jump
                .update(cx, |palette, cx| palette.close(window, cx)),
            Scene::PrSwitcher | Scene::PrSwitcherEnd => self
                .pr_switcher
                .update(cx, |switcher, cx| switcher.close(window, cx)),
            Scene::FinishReview => {
                self.session.update(cx, |session, cx| {
                    session.type_in(&TextBox::FinishBody, "", cx);
                });
                cx.set_global(StagedFinishReview(false));
            }
            Scene::CommentComposer | Scene::CommentComposerNowPrimary => {
                if scene == Scene::CommentComposerNowPrimary {
                    crate::app::keymap::set_keymap(
                        crate::app::keymap::configured_keymap().keymap,
                        cx,
                    );
                }
                self.session.update(cx, |session, cx| {
                    let drafts: Vec<_> = session
                        .review()
                        .comment_boxes()
                        .map(|comment_box| TextBox::Draft(comment_box.draft.place.clone()))
                        .collect();
                    for draft in &drafts {
                        session.discard(draft, cx);
                    }
                });
                window.focus(&self.focus, cx);
            }
            Scene::AskPanel => self.ask.update(cx, |panel, cx| panel.close(window, cx)),
            Scene::SwitchingVeil => {}
            Scene::LoadingScreen => {
                let active = cx.global::<PrSwitching>().active.clone();
                active.update(cx, |active, cx| {
                    active.stop_waiting();
                    cx.notify();
                });
            }
            Scene::Gallery => cx.set_global(StagedGallery(false)),
            Scene::Shortcuts => self
                .shortcuts
                .update(cx, |sheet, cx| sheet.close(window, cx)),
            Scene::StickyStepBar | Scene::PinnedFileHeader | Scene::FocusedChange => {
                self.scroll_to_row(0);
            }
            Scene::PieceLabelsShown | Scene::PieceLabelsHidden => self.go_to_first_open_code(cx),
            Scene::Toasts => {
                if let Some(toaster) = cx.try_global::<Toaster>().cloned() {
                    toaster.0.update(cx, ToastShelf::clear);
                }
            }
            Scene::GitHubOutage => {
                self.session.update(cx, ReviewSession::unstage_outage);
                if let Some(toaster) = cx.try_global::<Toaster>().cloned() {
                    toaster.0.update(cx, ToastShelf::clear);
                }
            }
            Scene::AllRead | Scene::TourEndUnread => {
                self.session.update(cx, ReviewSession::unstage);
                self.go_to_first_open_code(cx);
            }
            Scene::ReReview | Scene::ReReviewDone => {
                self.session.update(cx, ReviewSession::unstage);
            }
            Scene::AiReviewFound => self.change_review(cx, |review| {
                review.ai_review_is(AiReview::NotRun);
            }),
            Scene::AiReviewFindingsList => {
                self.findings
                    .update(cx, |sheet, cx| sheet.close(window, cx));
                self.change_review(cx, |review| review.ai_review_is(AiReview::NotRun));
            }
            Scene::AiReviewOnLines | Scene::AiReviewOutdated => self.change_review(cx, |review| {
                review.ai_review_is(AiReview::NotRun);
                review.go_to_summary();
            }),
            Scene::AiReviewAskWhy => {
                if let Ok(answered) = FindingId::try_from("a".to_owned()) {
                    self.finding_questions.close(&answered);
                }
                self.change_review(cx, |review| {
                    review.unstage_finding_threads();
                    review.ai_review_is(AiReview::NotRun);
                    review.go_to_summary();
                });
            }
            Scene::AiReviewThreadFromFinding => self.change_review(cx, |review| {
                review.unstage_thread_from_finding();
                review.ai_review_is(AiReview::NotRun);
                review.go_to_summary();
            }),
            Scene::AiReviewFixRequests => self.change_review(cx, |review| {
                review.unstage_fix_requests();
                review.ai_review_is(AiReview::NotRun);
                review.go_to_summary();
            }),
            Scene::AiReviewRunning | Scene::AiReviewFailed => self.change_review(cx, |review| {
                review.ai_review_run_is(AiReviewRun::NotRun);
            }),
            Scene::DescriptionFolds => {
                self.toggle_description_folds(cx);
                self.go_to_first_open_code(cx);
            }
            Scene::PictureZoom => {
                self.picture_zoom
                    .update(cx, |zoom, cx| zoom.close(window, cx));
                self.go_to_first_open_code(cx);
            }
            Scene::UpdateOffered => {
                if cx.has_global::<Updates>() {
                    cx.remove_global::<Updates>();
                }
            }
            Scene::ReleaseInstallInTheWay => self.change_review(cx, |review| {
                review.clear_problem(ProblemKind::ReleaseInstallInTheWay);
            }),
            Scene::BasicOverview
            | Scene::BasicOverviewFiles
            | Scene::BasicFile
            | Scene::BasicRead
            | Scene::BasicGuidedReady
            | Scene::BasicGuidedWriting
            | Scene::LocalOverview
            | Scene::LocalMoveOffer
            | Scene::LocalFile
            | Scene::LocalTaskSendAnyway => {
                self.session.update(cx, ReviewSession::unstage_basic);
                self.go_to_first_open_code(cx);
            }
            Scene::LocalChangedOnDisk => {
                self.session
                    .update(cx, ReviewSession::unstage_local_changed_on_disk);
                self.go_to_first_open_code(cx);
            }
            Scene::SwitchedSummary => {
                self.change_review(cx, |review| review.steps_carried_read_ticks_are(0));
                self.go_to_first_open_code(cx);
            }
            Scene::AgentLinkOff => self
                .session
                .update(cx, ReviewSession::unstage_agent_link_off),
            Scene::TitleBarDraftChecksRunning
            | Scene::TitleBarReadyChecksFailing
            | Scene::TitleBarAutoMergeOff
            | Scene::TitleBarAutoMergeOn
            | Scene::TitleBarMergeNow
            | Scene::TitleBarChecksList
            | Scene::TitleBarMerged
            | Scene::TitleBarClosed => {
                self.session.update(cx, ReviewSession::unstage_pr_status);
                self.checks_hover.chip = false;
            }
            Scene::TitleBarIssue | Scene::TitleBarIssueCard | Scene::TitleBarIssueList => {
                self.session
                    .update(cx, ReviewSession::unstage_related_issues);
                self.issue_hover = IssueHover::default();
                self.issue_list_open = false;
            }
            Scene::TitleBarPrOrigin => self.session.update(cx, ReviewSession::unstage_pr_origin),
            Scene::RefreshRunning => self
                .session
                .update(cx, ReviewSession::unstage_refresh_running),
            Scene::RenderedMarkdown => {
                if let Some(path) = self.markdown_box(cx) {
                    self.change_review(cx, |review| {
                        review
                            .display_options_mut()
                            .show_file_as(&path, FileView::Code);
                    });
                }
                self.go_to_first_open_code(cx);
            }
        }
        cx.notify();
    }

    fn toggle_description_folds(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            for key in session.rows().description_fold_keys() {
                session.toggle_details(&key, cx);
            }
        });
    }

    /// Drags over three added lines in a row, as the reviewer would, and types a comment.
    fn compose_on_first_added_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let added = self.added_lines(usize::MAX, cx);
        let run = added
            .windows(3)
            .find(|run| matches!(run, [(a, _), _, (c, _)] if a + 2 == *c))
            .or_else(|| added.get(..1));
        let (Some((first_row, first)), Some((_, last))) =
            (run.and_then(<[_]>::first), run.and_then(<[_]>::last))
        else {
            eprintln!("no added lines on this screen to comment on");
            return;
        };
        self.scroll_to_row(first_row.saturating_sub(4));
        self.on_row_gesture(RowGesture::Press, first, cx);
        self.on_row_gesture(RowGesture::Over, last, cx);
        self.end_row_selection(window, cx);
        self.session.update(cx, |session, cx| {
            if let Some(place) = session.review().composing().cloned() {
                let draft = TextBox::Draft(place);
                session.type_in(&draft, "Should a blank value be refused here?", cx);
            }
        });
    }
}

fn sample_update() -> Option<Update> {
    let version = Version::from_tag("v0.5.0")?;
    let target = Target::running();
    let archive = format!("guided-review-{version}-{}.tar.gz", target.triple());
    Some(Update {
        version,
        target,
        checksum: ReleaseAsset {
            name: format!("{archive}.sha256"),
            url: String::new(),
        },
        archive: ReleaseAsset {
            name: archive,
            url: String::new(),
        },
    })
}

fn sample_agent_screen() -> Vec<String> {
    [
        "⏺ Reading the outline of ekvedaras/guided-review#14",
        "  ⎿  12 pieces in 7 files, small PR",
        "⏺ Opening src/app/mod.rs and src/viewer/app_window.rs",
        "⏺ Writing the tour: 4 steps, one diagram",
        "✻ Checking the tour…",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// Long enough to scroll in the issue card, and to show Markdown the card renders.
const ISSUE_BODY: &str = "The title bar should say which issue the PR is about, so a reviewer can open it without leaving the review.

## What to show

- The issue's number on a button, with the key that opens it.
- On hover, a card with the issue's title, state, author, labels and assignees.
- Its description, as Markdown, cut to a set height.

## Why

Reviewers keep asking where the PR came from. The answer is nearly always in the issue, and the PR description only links it.

## Details

1. Read the issues GitHub links to the PR first.
2. Then the ones the description names with a closing keyword or `Part of`.
3. Then the one the guided tour names, and last the one the branch is named after.

A ticket elsewhere has no card, only its link. A failed read keeps what was shown before, since the button is a shortcut and not something to warn about.

## Out of scope

Editing the issue, commenting on it and showing its timeline. The issue's page is one click away for those.

## Open questions

- Should a closed issue look different on the button?
- Should the card show the issue's milestone?
- How many issues are too many for a list?

> Keep the card quiet: it appears on hover and leaves with the mouse.
";
