//! Everything that differs by operating system. The rest of the app asks this module and never
//! checks `target_os` itself; `scripts/check` fails a `target_os` check anywhere else.
//!
//! Each OS is an adapter module. An adapter that is plain data compiles on every host, so tests
//! check both adapters wherever they run. `Os::RUNNING` is the one place that picks the adapter;
//! only code that needs one OS's libraries is behind `cfg` instead.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_folders::AppFolders;

mod desktop_entry;
mod linux;
mod macos;

pub use desktop_entry::{
    APP_ID, DesktopEntry, EntryPlaced, IN_LOGIN_SHELL, Relaunch, info_plist,
    write_desktop_entry_templates,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
}

impl Os {
    pub const RUNNING: Self = if cfg!(target_os = "macos") {
        Self::MacOs
    } else {
        Self::Linux
    };

    /// Every fact of this OS, from its adapter.
    fn adapter(self) -> &'static Adapter {
        match self {
            Self::MacOs => &macos::ADAPTER,
            Self::Linux => &linux::ADAPTER,
        }
    }

    /// The vendor and system half of a Rust target triple, as release archives are named.
    pub fn triple_system(self) -> &'static str {
        self.adapter().triple_system
    }

    /// The family for the app's text: the first of this OS's choices that is installed, or its
    /// last choice unasked.
    pub fn text_font(self, installed: impl Fn(&str) -> bool) -> &'static str {
        first_installed(self.adapter().text_fonts, installed)
    }

    /// The family for code, picked like [`Self::text_font`].
    pub fn code_font(self, installed: impl Fn(&str) -> bool) -> &'static str {
        first_installed(self.adapter().code_fonts, installed)
    }

    /// The key held for the app's own shortcuts, as gpui's `secondary` picks it.
    pub fn primary_key(self) -> PrimaryKey {
        self.adapter().primary_key
    }

    /// How keys are written in a line of text and on key caps.
    pub fn key_spelling(self) -> KeySpelling {
        self.adapter().key_spelling
    }

    /// The text box keys that differ by OS, as gpui keystroke text. The keys every OS shares
    /// (arrows, Home, End, Backspace …) are the text box's own.
    pub fn text_keys(self) -> &'static [(&'static str, TextEdit)] {
        self.adapter().text_keys
    }

    /// The window's own title bar options. With window buttons the app draws the title bar
    /// under them; without, the window manager decides the decorations.
    pub fn titlebar(self, title: gpui::SharedString) -> gpui::TitlebarOptions {
        let buttons = self.adapter().window_buttons;
        gpui::TitlebarOptions {
            title: Some(title),
            appears_transparent: buttons.is_some(),
            traffic_light_position: buttons
                .map(|buttons| gpui::point(gpui::px(buttons.corner), gpui::px(buttons.corner))),
        }
    }

    /// Room kept free at the left of the title bar, so nothing is drawn under the window buttons.
    pub fn window_buttons_inset(self) -> f32 {
        self.adapter()
            .window_buttons
            .map_or(0., |buttons| buttons.inset)
    }

    /// The menus the OS shows for the app, holding Quit beside its key.
    pub fn app_menus(self, quit: impl gpui::Action) -> Vec<gpui::Menu> {
        self.adapter()
            .app_menu
            .map(|menu| gpui::Menu {
                name: menu.name.into(),
                items: vec![gpui::MenuItem::action(menu.quit, quit)],
                disabled: false,
            })
            .into_iter()
            .collect()
    }

    /// What brings the Herdr window to the front once a terminal in it is focused.
    pub fn herdr_raise(self) -> HerdrRaise {
        self.adapter().herdr_raise
    }
}

/// What the app knows of one OS, as plain data. Another OS is one more of these.
struct Adapter {
    triple_system: &'static str,
    /// Best first.
    text_fonts: &'static [&'static str],
    code_fonts: &'static [&'static str],
    primary_key: PrimaryKey,
    key_spelling: KeySpelling,
    text_keys: &'static [(&'static str, TextEdit)],
    window_buttons: Option<WindowButtons>,
    app_menu: Option<AppMenu>,
    herdr_raise: HerdrRaise,
    /// This OS's Desktop entry in a home, starting a bin link.
    desktop_entry: fn(&Path, &Path) -> Arc<dyn DesktopEntry>,
}

/// The app menu, holding only Quit.
#[derive(Debug, Clone, Copy)]
struct AppMenu {
    name: &'static str,
    quit: &'static str,
}

/// The XDG folders, on every OS so far: the variables when set to an absolute path, the XDG
/// defaults under home otherwise. The socket falls back to the state folder with no runtime folder.
/// A Windows adapter would pick its own here.
pub fn app_folders(home: &Path, env: impl Fn(&str) -> Option<OsString>) -> AppFolders {
    let base = |name: &str| {
        env(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let ours = |folder: PathBuf| folder.join("guided-review");
    let state = ours(base("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state")));
    AppFolders::new(
        home,
        ours(base("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"))),
        state.clone(),
        ours(base("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"))),
        base("XDG_RUNTIME_DIR").map_or(state, ours),
    )
}

/// The OS's close, minimise and zoom buttons, drawn on the app's own title bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowButtons {
    /// From the window's top left corner to the buttons, both ways.
    pub corner: f32,
    /// From the window's left edge to past the buttons' right edge.
    pub inset: f32,
}

fn first_installed(choices: &[&'static str], installed: impl Fn(&str) -> bool) -> &'static str {
    let Some((last, before)) = choices.split_last() else {
        return "";
    };
    before
        .iter()
        .find(|family| installed(family))
        .unwrap_or(last)
}

/// Which key the Primary modifier is. On Linux Super belongs to the desktop, so Control is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryKey {
    Command,
    Control,
}

/// The face of each modifier, and what stands between the caps when a chord is spelled in text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpelling {
    pub primary: &'static str,
    pub ctrl: &'static str,
    pub alt: &'static str,
    pub shift: &'static str,
    pub joiner: &'static str,
}

/// A command that brings the Herdr window to the front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HerdrRaise {
    pub program: &'static str,
    pub args: &'static [&'static str],
    /// Only the desktop can raise a window on Linux, and not every one lets a program ask; there
    /// a raise that cannot run or fails leaves Herdr behind instead of failing the hand-off.
    pub best_effort: bool,
}

/// An edit or caret move a text box key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEdit {
    BackspaceWord,
    BackspaceLine,
    DeleteWord,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    TextStart,
    TextEnd,
    SelectWordLeft,
    SelectWordRight,
    SelectLineStart,
    SelectLineEnd,
    SelectAll,
    Copy,
    Cut,
    Paste,
}

/// What the OS gets from the app as it starts. On macOS: the Dock icon a binary with no app
/// bundle lacks, or no Dock icon and no turn at the front when the app must not take the focus;
/// Option with an arrow or backspace, taken before macOS swallows it on some layouts; and the app
/// menu with `quit`. Nothing on Linux.
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        unused_variables,
        clippy::needless_pass_by_value,
        reason = "only the macOS adapter needs these"
    )
)]
pub fn start(takes_focus: bool, quit: impl gpui::Action, cx: &mut gpui::App) {
    #[cfg(target_os = "macos")]
    {
        if takes_focus {
            macos::dock_icon::show();
        } else {
            macos::dock_icon::stay_in_background();
        }
        macos::word_keys::take(cx);
        cx.set_menus(Os::MacOs.app_menus(quit));
    }
}
