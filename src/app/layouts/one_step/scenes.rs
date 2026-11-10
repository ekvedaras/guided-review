//! What the snapshot run shows: every screen in reading order, top and end, then the overlays and
//! panels over the first step. Each scene is staged through the layout's own methods and undone after
//! its picture, so the next one starts from the plain screen.

use gpui::{App, AsyncWindowContext, Context, ListOffset, Task, WeakEntity, Window, px};

use super::OneStepLayout;
use crate::app::components::diff_view::RowGesture;
use crate::app::components::pr_switcher::PrSwitching;
use crate::app::page_rows::PageRow;
use crate::app::snapshots::{Camera, StagedFinishReview, StagedGallery, sample_pr};
use crate::viewer::{AgentStatus, LineKind, LineLocation, Review, TerminalId};

const MAX_SCREENS: usize = 16;
/// Not the PR on screen, so the veil and the loading screen have another PR to name.
const OTHER_PR: u32 = 14;

#[derive(Debug, Clone, Copy)]
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
}

impl Scene {
    const ALL: [Self; 11] = [
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
        }
    }
}

pub(super) fn start(window: &Window, cx: &mut Context<OneStepLayout>) -> Option<Task<()>> {
    let camera = Camera::from_env()?;
    Some(cx.spawn_in(window, async move |layout, cx| {
        if let Err(error) = shoot(&camera, &layout, cx).await {
            eprintln!("snapshots stopped: {error:#}");
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
            layout.change_review(cx, Review::go_forward)
        })? {
            break;
        }
    }
    layout.update(cx, OneStepLayout::go_to_first_open_code)?;
    for scene in Scene::ALL {
        layout.update_in(cx, |layout, window, cx| layout.stage(scene, window, cx))?;
        camera.take(scene.file_name(), cx).await?;
        layout.update_in(cx, |layout, window, cx| layout.unstage(scene, window, cx))?;
    }
    Ok(())
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
        self.change_review(cx, Review::go_to_summary);
        for _ in 0..MAX_SCREENS {
            if !self.change_review(cx, Review::go_forward) || !self.added_lines(1, cx).is_empty() {
                break;
            }
        }
        self.scroll_to_row(0);
    }

    fn added_lines(&self, count: usize, cx: &App) -> Vec<(usize, LineLocation)> {
        let rows = self.session.read(cx).rows();
        (0..rows.len())
            .filter_map(|index| match rows.row(index) {
                Some(PageRow::Line(line)) if line.kind == LineKind::Added => {
                    Some((index, line.location.clone()))
                }
                _ => None,
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
                let targets = self.session.read(cx).review().jump_targets("");
                self.jump
                    .update(cx, |palette, cx| palette.open(targets, window, cx));
            }
            Scene::PrSwitcher => self
                .pr_switcher
                .update(cx, |switcher, cx| switcher.open(window, cx)),
            Scene::FinishReview => {
                self.session.update(cx, |session, cx| {
                    session.type_in_finish_body(
                        "Reads well. Two small questions inline, nothing blocking.",
                        cx,
                    );
                });
                cx.set_global(StagedFinishReview(true));
            }
            Scene::CommentComposer => self.compose_on_first_added_lines(window, cx),
            Scene::AskPanel => self.ask_about_screen(window, cx),
            Scene::SwitchingVeil => {
                if let Some(other) = sample_pr(OTHER_PR) {
                    let active = cx.global::<PrSwitching>().active.clone();
                    active.update(cx, |active, cx| {
                        active.picked(other);
                        cx.notify();
                    });
                }
            }
            Scene::LoadingScreen => {
                if let Some(other) = sample_pr(OTHER_PR) {
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
        }
        cx.notify();
    }

    fn unstage(&mut self, scene: Scene, window: &mut Window, cx: &mut Context<Self>) {
        match scene {
            Scene::Settings | Scene::SettingsEdited | Scene::SettingsEditedEnd => self
                .settings
                .update(cx, |settings, cx| settings.close(window, cx)),
            Scene::JumpPalette => self
                .jump
                .update(cx, |palette, cx| palette.close(window, cx)),
            Scene::PrSwitcher => self
                .pr_switcher
                .update(cx, |switcher, cx| switcher.close(window, cx)),
            Scene::FinishReview => {
                self.session
                    .update(cx, |session, cx| session.type_in_finish_body("", cx));
                cx.set_global(StagedFinishReview(false));
            }
            Scene::CommentComposer => {
                self.session.update(cx, |session, cx| {
                    let places: Vec<_> = session
                        .review()
                        .comment_drafts()
                        .iter()
                        .map(|draft| draft.place.clone())
                        .collect();
                    for place in &places {
                        session.discard_comment(place, cx);
                    }
                });
                self.lines_to_ask = None;
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
        }
        cx.notify();
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
            if let Some(place) = session.composing().cloned() {
                session.type_in_comment(&place, "Should a blank value be refused here?", cx);
            }
        });
    }

    fn ask_about_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.session.read(cx).rows();
        let ask = (0..rows.len()).find_map(|index| match rows.row(index) {
            Some(PageRow::Toolbar(toolbar)) => toolbar
                .ask
                .as_ref()
                .map(|ask| (ask.place.clone(), ask.label.clone())),
            _ => None,
        });
        let Some((place, label)) = ask else {
            eprintln!("no Ask button on this screen");
            return;
        };
        let ask = self.ask.clone();
        self.session.update(cx, |session, cx| {
            ask.update(cx, |panel, cx| {
                panel.open(session.review(), place, label, window, cx);
            });
        });
    }
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
