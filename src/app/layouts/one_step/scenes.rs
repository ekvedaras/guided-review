//! What the snapshot run shows: every screen in reading order, top and end, then the overlays and
//! panels over the first step. Each scene is staged through the layout's own methods and undone after
//! its picture, so the next one starts from the plain screen.

use gpui::{
    App, AsyncWindowContext, Context, Keystroke, ListOffset, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Task, WeakEntity, Window, point,
    px,
};

use super::OneStepLayout;
use crate::app::components::diff_view::RowGesture;
use crate::app::components::pr_switcher::PrSwitching;
use crate::app::components::toast_stack::{ToastShelf, Toaster};
use crate::app::page_rows::PageRow;
use crate::app::review_session::ReviewSession;
use crate::app::snapshots::{Camera, StagedFinishReview, StagedGallery, sample_pr};
use crate::app::updater::Updates;
use crate::update::{ReleaseAsset, Target, Update, UpdateOffer, Version};
use crate::viewer::{AgentStatus, LineKind, LineLocation, TerminalId, TextBox};

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
    Shortcuts,
    JumpActions,
    StickyStepBar,
    Toasts,
    AllRead,
    DescriptionFolds,
    PictureZoom,
    UpdateOffered,
}

impl Scene {
    const ALL: [Self; 19] = [
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
        }
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
    pick_a_word_and_copy(camera, layout, cx).await
}

/// With a comment box open on the page, a double click on the first added line's code on screen, as the
/// mouse gives it, then cmd-c: the word is selected, and the copy says so.
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
        if let Ok(keys) = Keystroke::parse("cmd-c") {
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
                let groups = self.session.read(cx).review().palette("");
                self.jump
                    .update(cx, |palette, cx| palette.open(groups, window, cx));
            }
            Scene::PrSwitcher => self
                .pr_switcher
                .update(cx, |switcher, cx| switcher.open(window, cx)),
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
            Scene::Shortcuts => self
                .shortcuts
                .update(cx, |sheet, cx| sheet.toggle(window, cx)),
            Scene::StickyStepBar => {
                let rows = self.screen_list.item_count();
                self.scroll_to_row(rows / 2);
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
                let home = cx.global::<PrSwitching>().home.clone();
                if let Some(update) = sample_update() {
                    Updates::stage(&home, UpdateOffer::Available(update), cx);
                }
            }
            Scene::JumpActions => {
                let query = ">";
                let groups = self.session.read(cx).review().palette(query);
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
            Scene::Settings | Scene::SettingsEdited | Scene::SettingsEditedEnd => self
                .settings
                .update(cx, |settings, cx| settings.close(window, cx)),
            Scene::JumpPalette | Scene::JumpActions => self
                .jump
                .update(cx, |palette, cx| palette.close(window, cx)),
            Scene::PrSwitcher => self
                .pr_switcher
                .update(cx, |switcher, cx| switcher.close(window, cx)),
            Scene::FinishReview => {
                self.session.update(cx, |session, cx| {
                    session.type_in(&TextBox::FinishBody, "", cx);
                });
                cx.set_global(StagedFinishReview(false));
            }
            Scene::CommentComposer => {
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
            Scene::StickyStepBar => self.scroll_to_row(0),
            Scene::Toasts => {
                if let Some(toaster) = cx.try_global::<Toaster>().cloned() {
                    toaster.0.update(cx, ToastShelf::clear);
                }
            }
            Scene::AllRead => {
                self.session.update(cx, ReviewSession::unstage);
                self.go_to_first_open_code(cx);
            }
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
