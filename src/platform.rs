//! Everything that differs by operating system. The rest of the app asks this module and never
//! checks `target_os` itself; `scripts/check` fails a `target_os` check anywhere else.
//!
//! Each OS is an adapter module. An adapter that is plain data compiles on every host, so tests
//! check both adapters wherever they run. `Os::RUNNING` is the one place that picks the adapter;
//! only code that needs one OS's libraries is behind `cfg` instead.

mod linux;
mod macos;

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

    /// The vendor and system half of a Rust target triple, as release archives are named.
    pub fn triple_system(self) -> &'static str {
        match self {
            Self::MacOs => macos::TRIPLE_SYSTEM,
            Self::Linux => linux::TRIPLE_SYSTEM,
        }
    }

    /// The family for the app's text: the first of this OS's choices that is installed, or its
    /// last choice unasked.
    pub fn text_font(self, installed: impl Fn(&str) -> bool) -> &'static str {
        first_installed(
            match self {
                Self::MacOs => macos::TEXT_FONTS,
                Self::Linux => linux::TEXT_FONTS,
            },
            installed,
        )
    }

    /// The family for code, picked like [`Self::text_font`].
    pub fn code_font(self, installed: impl Fn(&str) -> bool) -> &'static str {
        first_installed(
            match self {
                Self::MacOs => macos::CODE_FONTS,
                Self::Linux => linux::CODE_FONTS,
            },
            installed,
        )
    }
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

/// The app's icon in the Dock, which a binary with no app bundle lacks. Nothing on Linux.
pub fn show_app_icon() {
    #[cfg(target_os = "macos")]
    macos::dock_icon::show();
}

/// No Dock icon and no turn at the front, for the snapshot run. Nothing on Linux.
pub fn stay_in_background() {
    #[cfg(target_os = "macos")]
    macos::dock_icon::stay_in_background();
}

/// Option with an arrow or backspace, taken before macOS swallows it on some layouts. Nothing on
/// Linux.
#[cfg_attr(
    not(target_os = "macos"),
    expect(unused_variables, reason = "only the macOS adapter needs the app")
)]
pub fn take_word_keys(cx: &mut gpui::App) {
    #[cfg(target_os = "macos")]
    macos::word_keys::take(cx);
}
