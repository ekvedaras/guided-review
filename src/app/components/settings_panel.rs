use std::path::PathBuf;

use crate::app_folders::AppFolders;
use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, PathPromptOptions, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::app::code_links::open_file_in_ide;
use crate::config::{AskEffortChoice, AskModelChoice, AskSettings};
use crate::ide::{IdeChoice, IdeTemplate};
use crate::instructions::{InstructionsChoice, InstructionsKind, SkillName};
use crate::navigation::ServerAnswer;
use crate::viewer::{
    CloneRowProblem, MergeMethod, MoreLines, Motion, Presence, SaveOutcome, SendAction, SendRole,
    SettingProblem, Settings, SettingsPage,
};

use super::button::{
    ButtonKind, ButtonSize, button, icon_button, icon_label_button, toggle_button,
};
use super::chip::{build_chip, chip, dropping_in, notice_band};
use super::icon::{Icon, sized_icon};
use super::motion::{BOX_RISE, Edge, OVERLAY, STATE, fade_in, pop_when_shown, slide_in};
use super::overlay::{close_presence, picker_row_at, rise_for, scrim_fade, touching_places};
use super::palette::{
    CARD_RADIUS, CODE_FONT, CONTROL_RADIUS, Colors, INNER_RADIUS, TEXT_FONT, Tone, inside_edge,
};
use super::segmented::{Segment, SegmentLayout, Segmented};
use super::text_box::{Field, TextBox};
use super::text_input::Lines;
use super::title_bar::{TitleBarArea, title_inset};
use super::version_link::version_link;

mod keys;

/// The list of pages on the sheet's left.
const PAGE_LIST_WIDTH: f32 = 180.;
/// The widest a sheet over a review grows on a wide window.
const SHEET_WIDTH: f32 = PAGE_LIST_WIDTH + 860.;
/// The column the header, the sections and the action bar's contents line up in.
const COLUMN_WIDTH: f32 = 780.;

/// What the panel tells the layout. The panel writes the config; what the new settings do to the PR on
/// screen is the layout's to decide.
pub enum SettingsEvent {
    /// The config file was written, so everything that read it before is out of date.
    Saved,
    Closed,
}

/// What the panel covers: a sheet over the whole review, sidebar too, so nothing behind it is live;
/// or a window that holds only Settings, as its whole page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    SheetOverReview,
    WholeWindow,
}

impl EventEmitter<SettingsEvent> for SettingsPanel {}

/// The setting a red line belongs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Instructions(InstructionsKind),
    Ide,
    AskModel,
    AskEffort,
    Clone(usize),
    MoreLines,
    Keys,
}

/// Which half of a clone row a problem is about, so that half wears the danger edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClonePart {
    Repo,
    Folder,
}

impl ClonePart {
    fn of(problem: CloneRowProblem) -> Self {
        match problem {
            CloneRowProblem::NotOwnerSlashRepo | CloneRowProblem::SameRepoTwice => Self::Repo,
            CloneRowProblem::FolderGone | CloneRowProblem::NoRemoteOfRepo => Self::Folder,
        }
    }
}

struct ShownProblem {
    page: SettingsPage,
    slot: Slot,
    clone_part: Option<ClonePart>,
    message: SharedString,
}

impl ShownProblem {
    fn of(problem: &SettingProblem) -> Self {
        let clone_part = match problem {
            SettingProblem::Clone { problem, .. } => Some(ClonePart::of(*problem)),
            _ => None,
        };
        let slot = match problem {
            SettingProblem::Clone { row, .. } => Slot::Clone(*row),
            SettingProblem::InstructionsFileGone(kind, _)
            | SettingProblem::InstructionsUnreadable(kind, _) => Slot::Instructions(*kind),
            SettingProblem::AskEffortUnknown(_) => Slot::AskEffort,
            SettingProblem::AskModelBlank => Slot::AskModel,
            SettingProblem::IdeTemplateBlank => Slot::Ide,
            SettingProblem::MoreLinesNotACount(_) => Slot::MoreLines,
            SettingProblem::Key(_) => Slot::Keys,
        };
        Self {
            page: SettingsPage::of(problem),
            slot,
            clone_part,
            message: problem.message().into(),
        }
    }
}

/// A "Custom" choice with its text box. The box stays open while the reviewer types, even when the text
/// happens to equal a preset the model would read back as that preset.
#[derive(Default)]
struct CustomEntry {
    is_open: bool,
    text: String,
    focus: Option<FocusHandle>,
}

/// What the panel keeps beside one setting of instructions, besides the setting.
struct InstructionsEntry {
    skill: CustomEntry,
    default_file: SharedString,
    default_file_exists: bool,
}

impl InstructionsEntry {
    fn load(settings: &Settings, kind: InstructionsKind, cx: &mut Context<SettingsPanel>) -> Self {
        let skill_text = settings.instructions(kind).skill_text();
        Self {
            skill: CustomEntry {
                is_open: skill_text.is_some(),
                text: skill_text.unwrap_or_default().to_owned(),
                focus: Some(cx.focus_handle()),
            },
            default_file: settings.default_file(kind).display().to_string().into(),
            default_file_exists: settings.default_file_exists(kind),
        }
    }
}

/// A config that loaded, being edited.
struct Editing {
    settings: Settings,
    skills: Vec<(SkillName, SharedString)>,
    summary: InstructionsEntry,
    review: InstructionsEntry,
    /// What the Default segments stand for, said beside the setting's name.
    default_model_note: SharedString,
    default_effort_note: SharedString,
    default_effort_level: usize,
    default_more_lines_note: SharedString,
    ide: CustomEntry,
    model: CustomEntry,
    /// The text the reviewer typed for the number, kept as typed while it is not a number yet.
    more_lines: CustomEntry,
    folder_labels: Vec<SharedString>,
    problems: Vec<ShownProblem>,
    /// Why the last Save or file creation went wrong, when it is not a problem of one setting.
    note: Option<SharedString>,
    is_confirming_discard: bool,
}

impl Editing {
    fn load(folders: &AppFolders, cx: &mut Context<SettingsPanel>) -> anyhow::Result<Self> {
        let settings = Settings::load(folders)?;
        let skills = settings
            .skills()
            .into_iter()
            .map(|skill| {
                let label = SharedString::from(skill.as_str().to_owned());
                (skill, label)
            })
            .collect();
        let summary = InstructionsEntry::load(&settings, InstructionsKind::Summary, cx);
        let review = InstructionsEntry::load(&settings, InstructionsKind::Review, cx);
        let entry = |is_open: bool, text: &str| CustomEntry {
            is_open,
            text: text.to_owned(),
            focus: Some(cx.focus_handle()),
        };
        let ide = entry(
            settings.ide().custom_text().is_some(),
            settings.ide_custom_text(),
        );
        let model = entry(
            settings.ask_model().custom_text().is_some(),
            settings.ask_model_custom_text(),
        );
        let more_lines = entry(true, &settings.more_lines_text());
        let problems = settings
            .problems_on_open()
            .iter()
            .map(ShownProblem::of)
            .collect();
        let mut editing = Self {
            default_model_note: format!("Default is {}", AskSettings::default().model).into(),
            default_effort_note: format!("Default is {}", AskSettings::default().effort).into(),
            default_effort_level: effort_level(&AskEffortChoice::of(Some(
                &AskSettings::default().effort,
            ))),
            default_more_lines_note: format!(
                "Default is {}; from {} to {}",
                MoreLines::default().get(),
                MoreLines::MIN,
                MoreLines::MAX
            )
            .into(),
            settings,
            skills,
            summary,
            review,
            ide,
            model,
            more_lines,
            folder_labels: Vec::new(),
            problems,
            note: None,
            is_confirming_discard: false,
        };
        editing.refresh_folder_labels();
        Ok(editing)
    }

    fn refresh_folder_labels(&mut self) {
        self.folder_labels = self
            .settings
            .clones()
            .iter()
            .map(|clone| SharedString::from(clone.folder_label()))
            .collect();
    }

    /// Red lines that were up follow the edit, so the one just fixed goes away.
    fn recheck(&mut self) {
        self.note = None;
        if !self.problems.is_empty() {
            self.problems = self.settings.check().iter().map(ShownProblem::of).collect();
        }
    }

    fn problems_under(&self, slot: Slot) -> impl Iterator<Item = &SharedString> {
        self.problems
            .iter()
            .filter(move |problem| problem.slot == slot)
            .map(|problem| &problem.message)
    }

    fn instructions_entry(&self, kind: InstructionsKind) -> &InstructionsEntry {
        match kind {
            InstructionsKind::Summary => &self.summary,
            InstructionsKind::Review => &self.review,
        }
    }

    fn instructions_entry_mut(&mut self, kind: InstructionsKind) -> &mut InstructionsEntry {
        match kind {
            InstructionsKind::Summary => &mut self.summary,
            InstructionsKind::Review => &mut self.review,
        }
    }

    fn has_problem_under(&self, slot: Slot) -> bool {
        self.problems.iter().any(|problem| problem.slot == slot)
    }

    fn has_clone_problem(&self, row: usize, part: ClonePart) -> bool {
        self.problems
            .iter()
            .any(|problem| problem.slot == Slot::Clone(row) && problem.clone_part == Some(part))
    }
}

enum Loaded {
    Failed(SharedString),
    Ready(Box<Editing>),
}

/// The Settings panel: it covers what its `Placement` says. It draws a `Settings`
/// model and passes clicks on to it; what is allowed, wrong or written is the model's.
pub struct SettingsPanel {
    folders: AppFolders,
    /// What was loaded stays while the sheet leaves, so it is drawn until it is gone.
    loaded: Option<Loaded>,
    presence: Presence,
    focus: FocusHandle,
    owner_focus: FocusHandle,
    placement: Placement,
    /// What the panel's owner has to say that is not about a setting, such as a PR that failed to open.
    notice: Option<SharedString>,
    clone_focus: Vec<FocusHandle>,
    recording: Option<keys::Recording>,
    page: SettingsPage,
    hovered_page: Option<SettingsPage>,
    scroll: ScrollHandle,
    /// Where this app's binary is, so the reviewer can tell which install runs.
    running_binary: SharedString,
}

impl SettingsPanel {
    pub const KEY_CONTEXT: &str = "SettingsPanel";

    pub fn new(
        folders: AppFolders,
        owner_focus: FocusHandle,
        placement: Placement,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            folders,
            loaded: None,
            presence: Presence::lasting(OVERLAY),
            focus: cx.focus_handle(),
            owner_focus,
            placement,
            notice: None,
            clone_focus: Vec::new(),
            recording: None,
            page: SettingsPage::Agents,
            hovered_page: None,
            scroll: ScrollHandle::new(),
            running_binary: crate::platform::running_binary()
                .map_or_else(|_| "unknown".into(), |exe| exe.display().to_string().into()),
        }
    }

    pub fn show_notice(&mut self, notice: SharedString, cx: &mut Context<Self>) {
        self.notice = Some(notice);
        cx.notify();
    }

    /// The window that held only Settings now holds a PR: the panel stays as it is, with what is being
    /// edited, over that PR's page area.
    pub fn move_to(
        &mut self,
        owner_focus: FocusHandle,
        placement: Placement,
        cx: &mut Context<Self>,
    ) {
        self.owner_focus = owner_focus;
        self.placement = placement;
        self.notice = None;
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    /// Reads the config again, so the panel never shows what an earlier opening left.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loaded = Some(match Editing::load(&self.folders, cx) {
            Ok(editing) => Loaded::Ready(Box::new(editing)),
            Err(error) => Loaded::Failed(format!("{error:#}").into()),
        });
        self.page = match &self.loaded {
            Some(Loaded::Ready(editing)) => {
                SettingsPage::Agents.after_refusal(&editing.settings.problems_on_open())
            }
            _ => SettingsPage::Agents,
        };
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.presence.open(Instant::now());
        self.recording = None;
        self.clone_focus.clear();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Asks first when there are unsaved changes.
    pub fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        match self.editing_mut() {
            Some(editing) if editing.settings.has_unsaved_changes() => {
                editing.is_confirming_discard = true;
                window.focus(&self.focus, cx);
                cx.notify();
            }
            _ => self.close(window, cx),
        }
    }

    fn keep_editing(&mut self, cx: &mut Context<Self>) {
        self.edit(cx, |editing| editing.is_confirming_discard = false);
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        self.recording = None;
        match self.placement {
            Placement::SheetOverReview => close_presence(self, cx, |this| &mut this.presence),
            // The window goes with it, so there is nothing to leave to.
            Placement::WholeWindow => {
                self.presence.close(Motion::Reduced, Instant::now());
            }
        }
        window.focus(&self.owner_focus, cx);
        cx.emit(SettingsEvent::Closed);
        cx.notify();
    }

    fn editing_mut(&mut self) -> Option<&mut Editing> {
        if !self.presence.is_open() {
            return None;
        }
        match self.loaded.as_mut()? {
            Loaded::Ready(editing) => Some(editing),
            Loaded::Failed(_) => None,
        }
    }

    fn edit(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Editing)) {
        if let Some(editing) = self.editing_mut() {
            change(editing);
            editing.refresh_folder_labels();
            editing.recheck();
            cx.notify();
        }
    }

    /// Whoever placed the panel closes it once saved; with nothing to save it closes here.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.editing_mut() else {
            return;
        };
        let mut refused = None;
        match editing.settings.save() {
            Ok(SaveOutcome::NothingChanged) => {
                self.close(window, cx);
                return;
            }
            Ok(SaveOutcome::Written) => {
                crate::app::apply_keymap(editing.settings.keymap(), cx);
                crate::app::language_servers::answers_saved(
                    editing.settings.language_servers(),
                    cx,
                );
                editing.problems.clear();
                editing.note = None;
                cx.emit(SettingsEvent::Saved);
            }
            Ok(SaveOutcome::Refused(problems)) => {
                editing.note = None;
                editing.problems = problems.iter().map(ShownProblem::of).collect();
                refused = Some(problems);
            }
            Err(error) => editing.note = Some(format!("{error:#}").into()),
        }
        if let Some(problems) = refused {
            self.show_page(self.page.after_refusal(&problems), cx);
        }
        cx.notify();
    }

    fn show_page(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        cx.notify();
    }

    fn page_list(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let pages = SettingsPage::ALL;
        let keys_clash = !editing.settings.key_problems().is_empty();
        div()
            .flex_none()
            .w(px(PAGE_LIST_WIDTH))
            .pt_6()
            .border_r_1()
            .border_color(colors.subtle_border)
            .flex()
            .flex_col()
            .children(pages.iter().enumerate().map(|(index, &page)| {
                let above = index
                    .checked_sub(1)
                    .and_then(|above| pages.get(above))
                    .copied();
                let below = pages.get(index + 1).copied();
                let (place, hovered_touching) =
                    touching_places(page, above, below, self.page, self.hovered_page);
                let has_problem = editing.problems.iter().any(|problem| problem.page == page)
                    || (page == SettingsPage::Keys && keys_clash);
                picker_row_at(
                    colors,
                    SharedString::from(format!("settings-page-{}", page.title())),
                    place,
                    hovered_touching,
                )
                .h(px(32.))
                .on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
                    if *is_hovered {
                        this.hovered_page = Some(page);
                    } else if this.hovered_page == Some(page) {
                        this.hovered_page = None;
                    }
                    cx.notify();
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.show_page(page, cx)))
                .child(div().flex_1().child(page.title()))
                .when(has_problem, |row| {
                    row.child(sized_icon(Icon::CircleAlert, colors.danger, px(14.)))
                })
            }))
    }

    fn open_default_file(&mut self, kind: InstructionsKind, cx: &mut Context<Self>) {
        let Some(editing) = self.editing_mut() else {
            return;
        };
        match editing.settings.create_default_file(kind) {
            Ok(file) => {
                editing.instructions_entry_mut(kind).default_file_exists = true;
                // In the reviewer's IDE when one is set: the system's app for `.md` may be any.
                match editing.settings.ide().template() {
                    Some(template) => {
                        if let Err(error) = open_file_in_ide(&template.launch_file(&file), cx) {
                            editing.note = Some(format!("the IDE did not start: {error}").into());
                        }
                    }
                    None => cx.open_with_system(&file),
                }
            }
            Err(error) => editing.note = Some(format!("{error:#}").into()),
        }
        cx.notify();
    }

    /// The picker is the system's, so the answer comes back later.
    fn pick_path(
        &mut self,
        options: PathPromptOptions,
        cx: &mut Context<Self>,
        picked: impl FnOnce(&mut Editing, PathBuf) + 'static,
    ) {
        let answer = cx.prompt_for_paths(options);
        cx.spawn(async move |this, cx| {
            let picked_path = match answer.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        if let Some(editing) = this.editing_mut() {
                            editing.note = Some(format!("{error:#}").into());
                        }
                        cx.notify();
                    })
                    .ok();
                    None
                }
            };
            if let Some(path) = picked_path {
                this.update(cx, |this, cx| {
                    this.edit(cx, |editing| picked(editing, path));
                })
                .ok();
            }
        })
        .detach();
    }

    fn pick_instructions_file(&mut self, kind: InstructionsKind, cx: &mut Context<Self>) {
        let options = PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        };
        self.pick_path(options, cx, move |editing, path| {
            editing
                .settings
                .set_instructions(kind, InstructionsChoice::OtherFile(path));
        });
    }

    fn pick_clone_folder(&mut self, row: usize, cx: &mut Context<Self>) {
        let options = PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        };
        self.pick_path(options, cx, move |editing, path| {
            editing.settings.pick_clone_folder(row, path);
        });
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        self.escape(window, cx);
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.editing_mut() {
            Some(editing) if editing.is_confirming_discard => self.keep_editing(cx),
            _ => self.request_close(window, cx),
        }
    }

    fn text_box(
        &self,
        id: impl Into<gpui::ElementId>,
        focus: &FocusHandle,
        text: &str,
        is_invalid: bool,
        cx: &Context<Self>,
        on_edit: impl Fn(&mut Editing, String) + 'static,
    ) -> Field {
        let (panel, cancel) = (cx.entity(), cx.entity());
        Field::new(focus)
            .invalid(is_invalid)
            .px_3()
            .h(ButtonSize::Regular.height())
            .flex()
            .items_center()
            .text_sm()
            .child(
                TextBox::new(id, focus, text, Lines::Single)
                    .on_edit(move |text, _, cx| {
                        panel.update(cx, |this, cx| {
                            this.edit(cx, |editing| on_edit(editing, text));
                        });
                    })
                    .on_cancel(move |window, cx| {
                        cancel.update(cx, |this, cx| this.escape(window, cx));
                    }),
            )
    }

    /// A preset's template or command, to read and not to edit.
    fn shown_text(colors: &Colors, text: impl Into<SharedString>) -> gpui::Div {
        div()
            .text_xs()
            .font_family(*CODE_FONT)
            .text_color(colors.muted_text)
            .child(text.into())
    }

    fn problem_lines(editing: &Editing, slot: Slot, colors: &Colors) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(editing.problems_under(slot).map(|message| {
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(sized_icon(Icon::CircleAlert, colors.danger, px(13.)))
                    .child(message.clone())
            }))
    }

    /// A group of settings under its title, straight on the sheet's ground: the sheet is the box,
    /// so a section draws none of its own, and a hairline between sections splits them.
    fn section(
        colors: &Colors,
        title: &'static str,
        about: &'static str,
        body: impl IntoElement,
    ) -> impl IntoElement {
        column(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(div().text_sm().text_color(colors.muted_text).child(about)),
                )
                .child(body),
        )
    }

    fn version(&self, colors: &Colors) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .child(version_link(colors, "settings-version", colors.text)),
            )
            .child(Self::shown_text(colors, self.running_binary.clone()))
    }

    fn section_rule(colors: &Colors) -> impl IntoElement {
        column(div().h(px(1.)).bg(colors.subtle_border))
    }

    /// Only the shown page's sections are built, so the long Keys list costs nothing elsewhere.
    fn page_sections(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let sections: Vec<gpui::AnyElement> = match self.page {
            SettingsPage::Agents => vec![
                Self::section(
                    colors,
                    "Summary instructions",
                    "How the tour writer writes the reviewer summary at the start of a tour.",
                    self.instructions(InstructionsKind::Summary, editing, colors, cx),
                )
                .into_any_element(),
                Self::section(
                    colors,
                    "Review instructions",
                    "What the review agent looks for when it reviews a change.",
                    self.instructions(InstructionsKind::Review, editing, colors, cx),
                )
                .into_any_element(),
                Self::section(
                    colors,
                    "Ask",
                    "The model and effort Ask uses.",
                    self.ask(editing, colors, cx),
                )
                .into_any_element(),
            ],
            SettingsPage::Code => vec![
                Self::section(
                    colors,
                    "Open code in",
                    "The editor a code link opens. With none, code opens on GitHub.",
                    self.ide(editing, colors, cx),
                )
                .into_any_element(),
                Self::section(
                    colors,
                    "More lines",
                    "How many lines the arrows on a gap and the [ and ] keys show at a time.",
                    self.more_lines(editing, colors, cx),
                )
                .into_any_element(),
                Self::section(
                    colors,
                    "Language servers",
                    "Whether language servers may answer the Definition card in a repo. Until yes, and after no, it answers by name, and nothing is downloaded or started.",
                    Self::language_servers(editing, colors, cx),
                )
                .into_any_element(),
                Self::section(
                    colors,
                    "Clones",
                    "Where each repo is cloned on this machine.",
                    self.clones(editing, colors, cx),
                )
                .into_any_element(),
            ],
            SettingsPage::PullRequests => vec![
                Self::section(
                    colors,
                    "Comments",
                    "Which send actions of a comment or reply box you reach for most. Where a box lacks one, the nearest it has stands in.",
                    Self::comment_actions(editing, colors, cx),
                )
                .into_any_element(),
                Self::section(
                    colors,
                    "Merge",
                    "How auto-merge and Merge now merge a PR.",
                    Self::merge_method(editing, colors, cx),
                )
                .into_any_element(),
            ],
            SettingsPage::Keys => vec![
                Self::section(
                    colors,
                    "Keys",
                    "What each key does. Change takes the next key you press; Esc leaves it as it was.",
                    self.keys(editing, colors, cx),
                )
                .into_any_element(),
            ],
            SettingsPage::About => vec![
                Self::section(
                    colors,
                    "Version",
                    "The build this app runs and where its binary is.",
                    self.version(colors),
                )
                .into_any_element(),
            ],
        };
        let mut shown = Vec::with_capacity(sections.len() * 2);
        for (index, section) in sections.into_iter().enumerate() {
            if index > 0 {
                shown.push(Self::section_rule(colors).into_any_element());
            }
            shown.push(section);
        }
        shown
    }

    fn instructions(
        &self,
        kind: InstructionsKind,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let id = |suffix: &str| SharedString::from(format!("{}-{suffix}", kind.label()));
        let entry = editing.instructions_entry(kind);
        let choice = editing.settings.instructions(kind);
        let is_skill = entry.skill.is_open;
        let picked = if is_skill {
            Some(2)
        } else {
            match choice {
                InstructionsChoice::DefaultFile => Some(0),
                InstructionsChoice::OtherFile(_) => Some(1),
                InstructionsChoice::Skill(_) => Some(2),
                InstructionsChoice::Off => Some(3),
                InstructionsChoice::Unreadable(_) => None,
            }
        };
        let segments = vec![
            Segment::new(
                id("default"),
                "Default file",
                cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.instructions_entry_mut(kind).skill.is_open = false;
                        editing
                            .settings
                            .set_instructions(kind, InstructionsChoice::DefaultFile);
                    });
                }),
            ),
            Segment::new(
                id("other-file"),
                "Other file…",
                cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.instructions_entry_mut(kind).skill.is_open = false;
                    });
                    this.pick_instructions_file(kind, cx);
                }),
            ),
            Segment::new(
                id("skill"),
                "Skill",
                cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.instructions_entry_mut(kind).skill.is_open = true;
                    });
                }),
            ),
            Segment::new(
                id("off"),
                "Off",
                cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.instructions_entry_mut(kind).skill.is_open = false;
                        editing
                            .settings
                            .set_instructions(kind, InstructionsChoice::Off);
                    });
                }),
            ),
        ];
        let has_problem = editing.has_problem_under(Slot::Instructions(kind));
        let control = Segmented::new(
            id("mode"),
            SegmentLayout::Row {
                segment_width: px(116.),
            },
            segments,
        )
        .picked(picked)
        .invalid(matches!(choice, InstructionsChoice::Unreadable(_)));
        let detail = if is_skill {
            Some(
                self.skill_picker(kind, editing, colors, cx)
                    .into_any_element(),
            )
        } else {
            match choice {
                    InstructionsChoice::DefaultFile => {
                        let (status, action) = if entry.default_file_exists {
                            (
                                (Tone::Success, "Found"),
                                icon_label_button(
                                    colors,
                                    id("default-open"),
                                    ButtonKind::Secondary,
                                    ButtonSize::Small,
                                    Icon::FileText,
                                    "Open",
                                ),
                            )
                        } else {
                            (
                                (Tone::Warning, "Not created yet"),
                                icon_label_button(
                                    colors,
                                    id("default-open"),
                                    ButtonKind::Tinted,
                                    ButtonSize::Small,
                                    Icon::FilePlus,
                                    "Create and open",
                                ),
                            )
                        };
                        Some(
                            file_row(colors, entry.default_file.clone(), has_problem)
                                .child(chip(colors, status.0).py_0p5().child(status.1))
                                .child(action.on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.open_default_file(kind, cx);
                                    }),
                                ))
                                .into_any_element(),
                        )
                    }
                    InstructionsChoice::OtherFile(path) => Some(
                        file_row(colors, path.display().to_string().into(), has_problem)
                            .when(has_problem, |row| {
                                row.child(chip(colors, Tone::Danger).py_0p5().child("Missing"))
                            })
                            .child(
                                icon_label_button(
                                    colors,
                                    id("other-file-pick"),
                                    ButtonKind::Secondary,
                                    ButtonSize::Small,
                                    Icon::FolderOpen,
                                    "Choose…",
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.pick_instructions_file(kind, cx);
                                })),
                            )
                            .into_any_element(),
                    ),
                    InstructionsChoice::Skill(_) => {
                        Some(self.skill_picker(kind, editing, colors, cx).into_any_element())
                    }
                    InstructionsChoice::Off => Some(
                        div()
                            .text_sm()
                            .text_color(colors.muted_text)
                            .child(match kind {
                                InstructionsKind::Summary => "The tour writer gets no summary instructions.",
                                InstructionsKind::Review => {
                                    "The review agent gets no guidance, only how to report what it finds."
                                }
                            })
                            .into_any_element(),
                    ),
                    InstructionsChoice::Unreadable(_) => None,
                }
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(control)
            .children(detail)
            .child(Self::problem_lines(
                editing,
                Slot::Instructions(kind),
                colors,
            ))
    }

    fn skill_picker(
        &self,
        kind: InstructionsKind,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let chosen = match editing.settings.instructions(kind) {
            InstructionsChoice::Skill(name) => Some(name),
            _ => None,
        };
        let skills = editing
            .skills
            .iter()
            .enumerate()
            .map(|(index, (name, label))| {
                let picked = name.clone();
                toggle_button(
                    colors,
                    SharedString::from(format!("{}-skill-{index}", kind.label())),
                    chosen == Some(name),
                    ButtonSize::Small,
                )
                .child(label.clone())
                .on_click(cx.listener(move |this, _, _, cx| {
                    let picked = picked.clone();
                    this.edit(cx, |editing| {
                        picked
                            .as_str()
                            .clone_into(&mut editing.instructions_entry_mut(kind).skill.text);
                        editing
                            .settings
                            .set_instructions(kind, InstructionsChoice::Skill(picked));
                    });
                }))
            });
        let skill = &editing.instructions_entry(kind).skill;
        let typed = skill.focus.as_ref().map(|focus| {
            self.text_box(
                SharedString::from(format!("{}-skill-name", kind.label())),
                focus,
                &skill.text,
                false,
                cx,
                move |editing, text| {
                    editing
                        .settings
                        .set_instructions(kind, InstructionsChoice::skill_named(&text));
                    editing.instructions_entry_mut(kind).skill.text = text;
                },
            )
        });
        rising(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_xs().text_color(colors.muted_text).child(
                    "Skills in ~/.claude/skills, or type any skill name, such as plugin:skill",
                ))
                .child(div().flex().flex_wrap().gap_2().children(skills))
                .children(typed),
            match kind {
                InstructionsKind::Summary => "summary-skill-rise",
                InstructionsKind::Review => "review-skill-rise",
            },
        )
    }

    fn ide(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let current = editing.settings.ide();
        let is_custom = editing.ide.is_open;
        let preset = |id: &'static str, name: &'static str, glyph: Glyph, choice: IdeChoice| {
            let is_on = !is_custom && current == choice;
            ide_tile(colors, id, name, glyph, is_on).on_click(cx.listener(move |this, _, _, cx| {
                let choice = choice.clone();
                this.edit(cx, |editing| {
                    editing.ide.is_open = false;
                    editing.settings.set_ide(&choice);
                });
            }))
        };
        let custom = ide_tile(
            colors,
            "ide-custom",
            "Custom",
            Glyph::Icon(Icon::Terminal),
            is_custom,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.edit(cx, |editing| {
                if !editing.ide.is_open {
                    editing.ide.is_open = true;
                    editing.ide.text = editing.settings.ide_custom_text().to_owned();
                    editing
                        .settings
                        .set_ide(&IdeChoice::Custom(IdeTemplate::new(
                            editing.ide.text.clone(),
                        )));
                }
            });
        }));
        let tiles = div()
            .flex()
            .flex_wrap()
            .gap_3()
            .child(preset(
                "ide-jetbrains",
                "JetBrains",
                Glyph::Monogram("JB"),
                IdeChoice::JetBrains,
            ))
            .child(preset(
                "ide-vscode",
                "VS Code",
                Glyph::Monogram("VS"),
                IdeChoice::VsCode,
            ))
            .child(preset(
                "ide-zed",
                "Zed",
                Glyph::Monogram("Z"),
                IdeChoice::Zed,
            ))
            .child(preset(
                "ide-none",
                "None",
                Glyph::Icon(Icon::Globe),
                IdeChoice::None,
            ))
            .child(custom);
        let detail = if is_custom {
            editing.ide.focus.as_ref().map(|focus| {
                rising(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            self.text_box(
                                "ide-template",
                                focus,
                                &editing.ide.text,
                                editing.has_problem_under(Slot::Ide),
                                cx,
                                |editing, text| {
                                    editing
                                        .settings
                                        .set_ide(&IdeChoice::Custom(IdeTemplate::new(text.clone())));
                                    editing.ide.text = text;
                                },
                            )
                            .font_family(*CODE_FONT),
                        )
                        .child(div().text_xs().text_color(colors.muted_text).child(
                            "A link or a shell command. {path} and {line} become the file and the line.",
                        )),
                    "ide-custom-rise",
                )
                .into_any_element()
            })
        } else {
            Some(match current.shown_template() {
                Some(template) => note_row(colors, Icon::ExternalLink)
                    .child(Self::shown_text(colors, template))
                    .into_any_element(),
                None => note_row(colors, Icon::Globe)
                    .child("Code links open the file on GitHub.")
                    .into_any_element(),
            })
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(tiles)
            .children(detail)
            .child(Self::problem_lines(editing, Slot::Ide, colors))
    }

    fn ask(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let model = editing.settings.ask_model();
        let is_custom = editing.model.is_open;
        let model_picked = if is_custom {
            Some(4)
        } else {
            Some(match model {
                AskModelChoice::AppDefault => 0,
                AskModelChoice::Opus => 1,
                AskModelChoice::Sonnet => 2,
                AskModelChoice::Haiku => 3,
                AskModelChoice::Custom(_) => 4,
            })
        };
        let model_segment = |id: &'static str, label: &'static str, choice: AskModelChoice| {
            Segment::new(
                id,
                label,
                cx.listener(move |this, _, _, cx| {
                    let choice = choice.clone();
                    this.edit(cx, |editing| {
                        editing.model.is_open = false;
                        editing.settings.set_ask_model(&choice);
                    });
                }),
            )
        };
        let model_segments = vec![
            model_segment("ask-model-default", "Default", AskModelChoice::AppDefault),
            model_segment("ask-model-opus", "opus", AskModelChoice::Opus),
            model_segment("ask-model-sonnet", "sonnet", AskModelChoice::Sonnet),
            model_segment("ask-model-haiku", "haiku", AskModelChoice::Haiku),
            Segment::new(
                "ask-model-custom",
                "Custom",
                cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| {
                        if !editing.model.is_open {
                            editing.model.is_open = true;
                            editing.model.text =
                                editing.settings.ask_model_custom_text().to_owned();
                            editing
                                .settings
                                .set_ask_model(&AskModelChoice::Custom(editing.model.text.clone()));
                        }
                    });
                }),
            ),
        ];
        let model_note = if is_custom {
            div()
                .text_xs()
                .text_color(colors.muted_text)
                .child("Any model name claude accepts")
        } else {
            match model.shown_command() {
                Some(command) => Self::shown_text(colors, command),
                None => div()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(editing.default_model_note.clone()),
            }
        };
        let custom_model = editing
            .model
            .focus
            .as_ref()
            .filter(|_| is_custom)
            .map(|focus| {
                rising(
                    self.text_box(
                        "ask-model-name",
                        focus,
                        &editing.model.text,
                        editing.has_problem_under(Slot::AskModel),
                        cx,
                        |editing, text| {
                            editing
                                .settings
                                .set_ask_model(&AskModelChoice::Custom(text.clone()));
                            editing.model.text = text;
                        },
                    ),
                    "ask-model-rise",
                )
            });
        let effort = editing.settings.ask_effort();
        let effort_picked = match effort {
            AskEffortChoice::AppDefault => Some(0),
            AskEffortChoice::Low => Some(1),
            AskEffortChoice::Medium => Some(2),
            AskEffortChoice::High => Some(3),
            AskEffortChoice::XHigh => Some(4),
            AskEffortChoice::Max => Some(5),
            AskEffortChoice::Unknown(_) => None,
        };
        let effort_segments =
            std::iter::once(("ask-effort-default", "Default", AskEffortChoice::AppDefault))
                .chain([
                    ("ask-effort-low", "low", AskEffortChoice::Low),
                    ("ask-effort-medium", "medium", AskEffortChoice::Medium),
                    ("ask-effort-high", "high", AskEffortChoice::High),
                    ("ask-effort-xhigh", "xhigh", AskEffortChoice::XHigh),
                    ("ask-effort-max", "max", AskEffortChoice::Max),
                ])
                .map(|(id, label, choice)| {
                    Segment::new(
                        id,
                        label,
                        cx.listener(move |this, _, _, cx| {
                            let choice = choice.clone();
                            this.edit(cx, |editing| editing.settings.set_ask_effort(&choice));
                        }),
                    )
                })
                .collect();
        let level = match effort {
            AskEffortChoice::AppDefault => editing.default_effort_level,
            ref chosen => effort_level(chosen),
        };
        let effort_note = div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(colors.muted_text)
            .child(effort_meter(colors, level))
            .when(effort == AskEffortChoice::AppDefault, |note| {
                note.child(editing.default_effort_note.clone())
            });
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(setting_row(
                        "Model",
                        model_note,
                        Segmented::new(
                            "ask-model",
                            SegmentLayout::Row {
                                segment_width: px(80.),
                            },
                            model_segments,
                        )
                        .picked(model_picked),
                    ))
                    .children(custom_model)
                    .child(Self::problem_lines(editing, Slot::AskModel, colors)),
            )
            .child(div().h_px().bg(colors.subtle_border))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(setting_row(
                        "Effort",
                        effort_note,
                        Segmented::new(
                            "ask-effort",
                            SegmentLayout::Row {
                                segment_width: px(70.),
                            },
                            effort_segments,
                        )
                        .picked(effort_picked)
                        .invalid(editing.has_problem_under(Slot::AskEffort)),
                    ))
                    .child(Self::problem_lines(editing, Slot::AskEffort, colors)),
            )
    }

    /// The Comments section: the two send actions that hold the main keys.
    fn comment_actions(editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::send_action_row(
                editing,
                SendRole::Primary,
                colors,
                cx,
            ))
            .child(Self::send_action_row(
                editing,
                SendRole::Secondary,
                colors,
                cx,
            ))
    }

    fn send_action_row(
        editing: &Editing,
        role: SendRole,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let settings = &editing.settings;
        let (name, id, chosen, other, about) = match role {
            SendRole::Primary => (
                "Primary",
                "primary-comment-action",
                settings.primary_comment_action(),
                settings.secondary_comment_action(),
                "The accent button, first in the row",
            ),
            SendRole::Secondary | SendRole::Other => (
                "Secondary",
                "secondary-comment-action",
                settings.secondary_comment_action(),
                settings.primary_comment_action(),
                "The second button in the row",
            ),
        };
        let segments = SendAction::ALL
            .into_iter()
            .map(|action| {
                Segment::new(
                    action.config_text(),
                    action.label(),
                    cx.listener(move |this, _, _, cx| {
                        this.edit(cx, |editing| match role {
                            SendRole::Primary => {
                                editing.settings.set_primary_comment_action(action);
                            }
                            SendRole::Secondary | SendRole::Other => {
                                editing.settings.set_secondary_comment_action(action);
                            }
                        });
                    }),
                )
            })
            .collect();
        let key = chosen
            .command()
            .default_chords_for(settings.send_choice())
            .first()
            .map_or_default(|chord| chord.spelled());
        let note = div().text_xs().text_color(colors.muted_text).child(format!(
            "{about}, with {key}. Picking {} here swaps the two. A key set under Keys wins.",
            other.label()
        ));
        setting_row(
            name,
            note,
            Segmented::new(
                id,
                SegmentLayout::Row {
                    segment_width: px(116.),
                },
                segments,
            )
            .picked(SendAction::ALL.iter().position(|action| *action == chosen)),
        )
    }

    /// The repos the reviewer was asked about, each with its answer.
    fn language_servers(
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let rows = editing.settings.language_servers();
        if rows.is_empty() {
            return div()
                .text_sm()
                .text_color(colors.muted_text)
                .child("No repo yet. The Definition card asks on the first hover in a repo.")
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .children(rows.iter().enumerate().map(|(index, row)| {
                let segments = [ServerAnswer::Yes, ServerAnswer::No]
                    .into_iter()
                    .map(|answer| {
                        Segment::new(
                            if answer.is_yes() { "yes" } else { "no" },
                            if answer.is_yes() { "Use" } else { "Do not use" },
                            cx.listener(move |this, _, _, cx| {
                                this.edit(cx, |editing| {
                                    editing.settings.change_language_servers(index, answer);
                                });
                            }),
                        )
                    })
                    .collect();
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_x_6()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(*CODE_FONT)
                            .text_sm()
                            .child(SharedString::from(row.codebase.to_string())),
                    )
                    .child(
                        Segmented::new(
                            SharedString::from(format!("language-servers-{index}")),
                            SegmentLayout::Row {
                                segment_width: px(110.),
                            },
                            segments,
                        )
                        .picked(Some(usize::from(!row.answer.is_yes()))),
                    )
            }))
            .into_any_element()
    }

    fn merge_method(editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let chosen = editing.settings.merge_method();
        let segments = MergeMethod::ALL
            .into_iter()
            .map(|method| {
                Segment::new(
                    method.config_text(),
                    method.label(),
                    cx.listener(move |this, _, _, cx| {
                        this.edit(cx, |editing| editing.settings.set_merge_method(method));
                    }),
                )
            })
            .collect();
        let note = div()
            .text_xs()
            .text_color(colors.muted_text)
            .child("Where a repo does not allow it, its first allowed method. [merge_methods] in config.toml sets one per repo.");
        setting_row(
            "Method",
            note,
            Segmented::new(
                "merge-method",
                SegmentLayout::Row {
                    segment_width: px(110.),
                },
                segments,
            )
            .picked(MergeMethod::ALL.iter().position(|method| *method == chosen)),
        )
    }

    fn more_lines(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let focus = editing.more_lines.focus.clone();
        let note = div()
            .text_xs()
            .text_color(colors.muted_text)
            .child(editing.default_more_lines_note.clone());
        let input = focus.map(|focus| {
            div().w(px(120.)).child(self.text_box(
                "more-lines-count",
                &focus,
                &editing.more_lines.text,
                editing.has_problem_under(Slot::MoreLines),
                cx,
                |editing, text| {
                    editing.settings.set_more_lines_text(&text);
                    editing.more_lines.text = text;
                },
            ))
        });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(setting_row("Lines per step", note, div().children(input)))
            .child(Self::problem_lines(editing, Slot::MoreLines, colors))
    }

    fn clones(&self, editing: &Editing, colors: &Colors, cx: &Context<Self>) -> impl IntoElement {
        let add_clone = |kind: ButtonKind, size: ButtonSize| {
            icon_label_button(colors, "clone-add", kind, size, Icon::Plus, "Add clone").on_click(
                cx.listener(|this, _, _, cx| {
                    this.edit(cx, |editing| {
                        editing.settings.add_clone();
                    });
                }),
            )
        };
        if editing.settings.clones().is_empty() {
            return div()
                .py_4()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .rounded(px(CONTROL_RADIUS))
                .border_1()
                .border_dashed()
                .border_color(colors.border)
                .child(
                    div()
                        .size(px(40.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(colors.inset)
                        .child(sized_icon(Icon::FolderGit, colors.muted_text, px(20.))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1()
                        .child(div().font_weight(FontWeight::MEDIUM).child("No clones yet"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.muted_text)
                                .child("Add one so code links open files from your own checkout."),
                        ),
                )
                .child(add_clone(ButtonKind::Tinted, ButtonSize::Regular))
                .into_any_element();
        }
        let rows = editing
            .settings
            .clones()
            .iter()
            .enumerate()
            .filter_map(|(row, clone)| {
                let focus = self.clone_focus.get(row)?;
                let label = editing.folder_labels.get(row)?.clone();
                let has_folder = !clone.folder.as_os_str().is_empty();
                let is_folder_wrong = editing.has_clone_problem(row, ClonePart::Folder);
                let folder_color = if is_folder_wrong {
                    colors.danger
                } else if has_folder {
                    colors.text
                } else {
                    colors.muted_text
                };
                Some(
                    div()
                        .px_3()
                        .py_2p5()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .when(row > 0, |line| {
                            line.border_t_1().border_color(colors.subtle_border)
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    self.text_box(
                                        SharedString::from(format!("clone-repo-{row}")),
                                        focus,
                                        &clone.repo,
                                        editing.has_clone_problem(row, ClonePart::Repo),
                                        cx,
                                        move |editing, text| {
                                            editing.settings.set_clone_repo(row, text);
                                        },
                                    )
                                    .w(px(210.))
                                    .flex_none(),
                                )
                                .child(sized_icon(Icon::ArrowRight, colors.muted_text, px(14.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(sized_icon(Icon::FolderOpen, folder_color, px(14.)))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis_middle()
                                                .text_xs()
                                                .when(has_folder, |path| {
                                                    path.font_family(*CODE_FONT)
                                                })
                                                .text_color(folder_color)
                                                .child(label),
                                        ),
                                )
                                .child(
                                    icon_button(
                                        colors,
                                        SharedString::from(format!("clone-folder-{row}")),
                                        ButtonKind::Secondary,
                                        ButtonSize::Regular,
                                        Icon::FolderOpen,
                                        "Choose folder",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.pick_clone_folder(row, cx);
                                        },
                                    )),
                                )
                                .child(
                                    icon_button(
                                        colors,
                                        SharedString::from(format!("clone-remove-{row}")),
                                        ButtonKind::Danger,
                                        ButtonSize::Regular,
                                        Icon::Trash,
                                        "Remove clone",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.edit(cx, |editing| {
                                                editing.settings.remove_clone(row);
                                            });
                                        },
                                    )),
                                ),
                        )
                        .child(Self::problem_lines(editing, Slot::Clone(row), colors)),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(px(CONTROL_RADIUS))
                    .border_1()
                    .border_color(colors.subtle_border)
                    .bg(colors.background)
                    .children(rows),
            )
            .child(
                div()
                    .flex()
                    .child(add_clone(ButtonKind::Ghost, ButtonSize::Small)),
            )
            .into_any_element()
    }

    /// Pinned under the form, so Save is always in reach: what state the edits are in on the left,
    /// the ways out on the right. While the discard question is up, it takes the bar.
    fn action_bar(
        &self,
        editing: &Editing,
        colors: &Colors,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let content = if editing.is_confirming_discard {
            slide_in(
                bar_row()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(colors.text)
                            .child(sized_icon(Icon::CircleAlert, colors.warning, px(16.)))
                            .child("Close Settings and lose your changes?"),
                    )
                    .child(
                        button(
                            colors,
                            "settings-keep-editing",
                            ButtonKind::Secondary,
                            ButtonSize::Regular,
                        )
                        .child("Keep editing")
                        .on_click(cx.listener(|this, _, _, cx| this.keep_editing(cx))),
                    )
                    .child(
                        icon_label_button(
                            colors,
                            "settings-discard",
                            ButtonKind::Danger,
                            ButtonSize::Large,
                            Icon::Trash,
                            "Discard changes",
                        )
                        .bg(colors.danger_background)
                        .border_color(colors.danger_border)
                        .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
                    ),
                "settings-bar-question",
                STATE,
                Edge::Bottom,
                BOX_RISE,
            )
            .into_any_element()
        } else {
            let state = if let Some(note) = editing.note.clone() {
                Some(
                    bar_state(colors, colors.danger)
                        .child(note)
                        .into_any_element(),
                )
            } else if !editing.problems.is_empty() {
                let count = editing.problems.len();
                Some(
                    bar_state(colors, colors.danger)
                        .child(if count == 1 {
                            "1 setting to fix before saving".to_owned()
                        } else {
                            format!("{count} settings to fix before saving")
                        })
                        .into_any_element(),
                )
            } else if editing.settings.has_unsaved_changes() {
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(colors.text)
                        .child(div().size(px(8.)).rounded_full().bg(colors.accent))
                        .child("Unsaved changes")
                        .into_any_element(),
                )
            } else {
                None
            };
            bar_row()
                .child(div().flex_1().min_w_0().children(state))
                .child(
                    button(
                        colors,
                        "settings-close",
                        ButtonKind::Ghost,
                        ButtonSize::Regular,
                    )
                    .child("Close")
                    .on_click(cx.listener(|this, _, window, cx| this.request_close(window, cx))),
                )
                .child(
                    icon_label_button(
                        colors,
                        "settings-save",
                        ButtonKind::Primary,
                        ButtonSize::Large,
                        Icon::Save,
                        "Save",
                    )
                    .min_w(px(112.))
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                )
                .into_any_element()
        };
        div()
            .flex_none()
            .flex()
            .justify_center()
            .pl(px(PAGE_LIST_WIDTH + 32.))
            .pr_8()
            .bg(colors.overlay)
            .when(
                matches!(self.placement, Placement::SheetOverReview),
                |bar| bar.rounded_b(px(inside_edge(CARD_RADIUS))),
            )
            .border_t_1()
            .border_color(colors.border)
            .shadow(colors.bar_shadow())
            .child(content)
    }

    /// A made-up config being edited, for the pictures: a custom IDE, another model and effort,
    /// two clones, one of them wrong, and nothing saved yet.
    #[cfg(feature = "snapshots")]
    pub fn stage_sample_edits(&mut self, cx: &mut Context<Self>) {
        const TEMPLATE: &str = "code --goto {path}:{line}";
        self.edit(cx, |editing| {
            editing.ide.is_open = true;
            TEMPLATE.clone_into(&mut editing.ide.text);
            editing
                .settings
                .set_ide(&IdeChoice::Custom(IdeTemplate::new(TEMPLATE.to_owned())));
            editing.settings.set_ask_model(&AskModelChoice::Sonnet);
            editing.settings.set_ask_effort(&AskEffortChoice::High);
            "30".clone_into(&mut editing.more_lines.text);
            editing.settings.set_more_lines_text("30");
            for (repo, folder) in [
                ("acme/widgets", "~/code/widgets"),
                ("acme/billing-service", "~/code/archive/billing-service"),
            ] {
                let row = editing.settings.add_clone();
                editing.settings.set_clone_repo(row, repo.to_owned());
                editing
                    .settings
                    .set_clone_folder(row, PathBuf::from(folder));
            }
        });
        if let Some(editing) = self.editing_mut()
            && let Some(clone) = editing.settings.clones().get(1).cloned()
        {
            editing.problems = vec![ShownProblem::of(&SettingProblem::Clone {
                row: 1,
                clone,
                problem: CloneRowProblem::FolderGone,
            })];
        }
        cx.notify();
    }

    /// A made-up key change, one key waiting to be pressed and one clash, for the pictures.
    #[cfg(feature = "snapshots")]
    pub fn stage_sample_keys(&mut self, cx: &mut Context<Self>) {
        self.edit(cx, |editing| {
            use crate::viewer::Chord;
            for (command, text) in [
                (crate::viewer::Command::WrapLines, "y"),
                (crate::viewer::Command::Jump, "n"),
            ] {
                if let Some(chord) = Chord::parse(text) {
                    editing.settings.set_key(command, chord);
                }
            }
        });
        self.start_recording(crate::viewer::Command::ToggleRead, cx);
    }

    /// Shows one page, for the pictures.
    #[cfg(feature = "snapshots")]
    pub fn stage_page(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        self.show_page(page, cx);
    }

    /// Picks Comment now as the primary action and Suggest change as the secondary one, for the pictures.
    #[cfg(feature = "snapshots")]
    pub fn stage_comment_now_primary(&mut self, cx: &mut Context<Self>) {
        self.edit(cx, |editing| {
            editing
                .settings
                .set_primary_comment_action(SendAction::CommentNow);
            editing
                .settings
                .set_secondary_comment_action(SendAction::SuggestChange);
        });
    }

    /// Scrolls the form to its end, for the pictures.
    #[cfg(feature = "snapshots")]
    pub fn scroll_to_end(&self) {
        self.scroll.scroll_to_bottom();
    }
}

/// The Settings column: as wide as the window allows, up to `COLUMN_WIDTH`, centred.
fn column(content: impl IntoElement) -> gpui::Div {
    div()
        .w_full()
        .px_8()
        .flex()
        .justify_center()
        .child(div().w_full().max_w(px(COLUMN_WIDTH)).child(content))
}

fn bar_row() -> gpui::Div {
    div()
        .w_full()
        .max_w(px(COLUMN_WIDTH))
        .py_3()
        .flex()
        .items_center()
        .gap_3()
}

fn bar_state(colors: &Colors, tone: gpui::Rgba) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .truncate()
        .text_color(tone)
        .child(sized_icon(Icon::CircleAlert, colors.danger, px(16.)))
}

/// An inner part of a section opening (a custom box, the skill picker) rises into place.
fn rising(content: impl IntoElement + Styled + 'static, id: &'static str) -> impl IntoElement {
    slide_in(content, id, STATE, Edge::Bottom, BOX_RISE)
}

/// A setting's name with a note under it on the left, its control on the right; on a narrow
/// window the control goes under.
fn setting_row(name: &'static str, note: gpui::Div, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_x_6()
        .gap_y_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().font_weight(FontWeight::MEDIUM).child(name))
                .child(note),
        )
        .child(control)
}

/// A file a setting points at: its icon and path, then what the caller adds (a status, an action).
fn file_row(colors: &Colors, path: SharedString, is_invalid: bool) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .pl_3()
        .pr_1p5()
        .py_1p5()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .border_color(if is_invalid {
            colors.danger
        } else {
            colors.subtle_border
        })
        .bg(colors.background)
        .child(sized_icon(Icon::FileText, colors.muted_text, px(16.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .font_family(*CODE_FONT)
                .text_xs()
                .child(path),
        )
}

/// A quiet line that says what a choice does.
fn note_row(colors: &Colors, icon: Icon) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(CONTROL_RADIUS))
        .bg(colors.inset)
        .text_sm()
        .text_color(colors.muted_text)
        .child(sized_icon(icon, colors.muted_text, px(14.)))
}

/// What an IDE tile shows above its name: a neutral monogram, or a drawn icon.
#[derive(Debug, Clone, Copy)]
enum Glyph {
    Monogram(&'static str),
    Icon(Icon),
}

fn ide_tile(
    colors: &Colors,
    id: &'static str,
    name: &'static str,
    glyph: Glyph,
    is_on: bool,
) -> gpui::Stateful<gpui::Div> {
    let ink = if is_on { colors.accent } else { colors.text };
    let mark = div()
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(INNER_RADIUS))
        .border_1()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(ink)
        .map(|mark| {
            if is_on {
                mark.bg(colors.icon_tile.diagonal())
                    .border_color(colors.current_border)
            } else {
                mark.bg(colors.inset).border_color(colors.subtle_border)
            }
        })
        .child(match glyph {
            Glyph::Monogram(letters) => letters.into_any_element(),
            Glyph::Icon(icon) => sized_icon(icon, ink, px(16.)).into_any_element(),
        });
    div()
        .id(id)
        .relative()
        .flex_1()
        .min_w(px(120.))
        .flex()
        .flex_col()
        .gap_2p5()
        .p_3()
        .rounded(px(CONTROL_RADIUS))
        .border_1()
        .cursor_pointer()
        .map(|tile| {
            if is_on {
                tile.bg(colors.current).border_color(colors.accent_border)
            } else {
                tile.bg(colors.raised_fill())
                    .border_color(colors.border)
                    .shadow(colors.control_shadow())
                    .hover(|tile| tile.border_color(colors.accent_border))
                    .active(|tile| tile.bg(colors.pressed).shadow_none())
            }
        })
        .child(mark)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(ink)
                .child(name),
        )
        .child(pop_when_shown(
            sized_icon(Icon::CircleCheck, colors.accent, px(16.))
                .absolute()
                .top(px(10.))
                .right(px(10.)),
            "tile-check",
            is_on,
        ))
}

/// How much thinking an effort asks for, from 1 (low) to 5 (max); 0 for an effort Claude does not know.
fn effort_level(effort: &AskEffortChoice) -> usize {
    AskEffortChoice::PICKABLE
        .iter()
        .position(|pickable| pickable == effort)
        .map_or(0, |index| index + 1)
}

/// Five bars rising from low to max, filled up to the chosen effort.
fn effort_meter(colors: &Colors, level: usize) -> impl IntoElement {
    const HEIGHTS: [f32; 5] = [4., 6., 8., 10., 12.];
    div()
        .flex()
        .items_end()
        .gap(px(2.))
        .h(px(12.))
        .children(HEIGHTS.into_iter().enumerate().map(move |(index, height)| {
            div()
                .w(px(3.))
                .h(px(height))
                .rounded(px(1.))
                .bg(if index < level {
                    colors.accent
                } else {
                    colors.border
                })
        }))
}

impl Render for SettingsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.presence.shown().is_none() {
            return div().into_any_element();
        }
        let Some(loaded) = &self.loaded else {
            return div().into_any_element();
        };
        let colors = Colors::active(cx);
        let rows_needed = match loaded {
            Loaded::Ready(editing) => editing.settings.clones().len(),
            Loaded::Failed(_) => 0,
        };
        while self.clone_focus.len() < rows_needed {
            self.clone_focus.push(cx.focus_handle());
        }
        let Some(loaded) = &self.loaded else {
            return div().into_any_element();
        };
        let close = icon_button(
            &colors,
            "settings-close-sheet",
            ButtonKind::Ghost,
            ButtonSize::Regular,
            Icon::Close,
            "Close Settings",
        )
        .on_click(cx.listener(|this, _, window, cx| this.request_close(window, cx)));
        let header = div()
            .flex_none()
            .w_full()
            .pl(px(PAGE_LIST_WIDTH))
            .pt_5()
            .pb_4()
            .border_b_1()
            .border_color(colors.subtle_border)
            .child(column(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Settings"),
                            )
                            .child(
                                div()
                                    .text_color(colors.muted_text)
                                    .child("How agents work, where code opens, how PRs are commented on and merged, and which keys do what."),
                            ),
                    )
                    .child(close),
            ));
        let (body, bar) = match loaded {
            Loaded::Failed(reason) => (
                div()
                    .flex_1()
                    .pt_6()
                    .child(column(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(colors.danger)
                            .child(sized_icon(Icon::CircleAlert, colors.danger, px(16.)))
                            .child(reason.clone()),
                    ))
                    .into_any_element(),
                None,
            ),
            Loaded::Ready(editing) => {
                let form = div()
                    .id("settings-form")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .gap_6()
                    .pt_6()
                    .pb_10()
                    .children(self.page_sections(editing, &colors, cx));
                (
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(self.page_list(editing, &colors, cx))
                        .child(form)
                        .into_any_element(),
                    Some(self.action_bar(editing, &colors, cx)),
                )
            }
        };
        let notice = self.notice.clone().map(|notice| {
            dropping_in(
                notice_band(&colors, Tone::Danger).py_3().child(notice),
                "settings-notice-band",
            )
        });
        let panel = div()
            .id("settings-panel")
            .key_context(Self::KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .font_family(*TEXT_FONT)
            .text_sm()
            .text_color(colors.text);
        match self.placement {
            Placement::WholeWindow => fade_in(
                panel
                    .absolute()
                    .inset_0()
                    .bg(colors.background)
                    .occlude()
                    .child(
                        TitleBarArea::new("settings-title-bar", colors.background).children(
                            build_chip(&colors).map(|chip| div().pl(title_inset()).child(chip)),
                        ),
                    )
                    .child(header)
                    .children(notice)
                    .child(body)
                    .children(bar),
                "settings-panel-fade",
                OVERLAY,
            )
            .into_any_element(),
            Placement::SheetOverReview => {
                let scrim = div()
                    .id("settings-scrim")
                    .absolute()
                    .inset_0()
                    .bg(colors.scrim)
                    .on_click(cx.listener(|this, _, window, cx| this.request_close(window, cx)));
                let scrim = scrim_fade(scrim, "settings-scrim-fade", &self.presence);
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .occlude()
                    .child(scrim)
                    .child(rise_for(
                        panel
                            .relative()
                            .w_full()
                            .max_w(px(SHEET_WIDTH))
                            .h(relative(0.9))
                            .mx_8()
                            .overflow_hidden()
                            .bg(colors.overlay)
                            .border_1()
                            .border_color(colors.border)
                            .rounded(px(CARD_RADIUS))
                            .shadow(colors.overlay_shadow())
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(header)
                            .children(notice)
                            .child(body)
                            .children(bar),
                        "settings-sheet-rise",
                        &self.presence,
                    ))
                    .into_any_element()
            }
        }
    }
}
