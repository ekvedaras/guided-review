use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;

use super::Os;
use super::linux::desktop_file;
use super::macos::app_bundle;
use crate::update::Version;

/// Names the app to the OS: the macOS bundle id, the Linux desktop file and window app id.
pub const APP_ID: &str = "io.github.ekvedaras.guided-review";

/// Set on the run the login shell makes, so it does not relaunch again.
pub const IN_LOGIN_SHELL: &str = "GUIDED_REVIEW_IN_LOGIN_SHELL";

pub use app_bundle::info_plist;

/// What the OS launcher starts: the App bundle on macOS, a desktop file and its icon on Linux. A
/// release carries its files; placing them from the release the bin link names keeps it current.
pub trait DesktopEntry: std::fmt::Debug + Send + Sync {
    /// What the reviewer calls the entry, as messages name it.
    fn name(&self) -> &'static str;

    /// Where the entry is put, as messages name it.
    fn folder(&self) -> &'static str;

    /// Makes the entry match the one carried by the `release` folder.
    fn place_from(&self, release: &Path) -> anyhow::Result<EntryPlaced>;

    /// What to start `exe` as when it starts the app.
    fn launcher_for(&self, exe: &Path) -> PathBuf;

    /// The environment variable that names the app a launch from the OS launcher started.
    fn launched_app_variable(&self) -> &'static str;

    /// The binary the entry starts.
    fn executable(&self) -> PathBuf;

    /// The shell a relaunch runs through when `$SHELL` is unset.
    fn login_shell(&self) -> &'static str;

    /// A launch from the OS launcher gets the session's short `PATH`, without `gh`, `claude` or `hc`.
    /// So it runs the entry's binary again through the reviewer's login shell, which reads their
    /// profile as a terminal does.
    fn relaunch_in_login_shell(
        &self,
        launched_app: Option<&OsStr>,
        shell: Option<&OsStr>,
        is_in_login_shell: bool,
    ) -> Option<Relaunch> {
        let is_our_app = launched_app == Some(OsStr::new(APP_ID));
        if !is_our_app || is_in_login_shell {
            return None;
        }
        let executable = self.executable();
        let args = [OsStr::new("-l"), OsStr::new("-i"), OsStr::new("-c")]
            .into_iter()
            .chain([OsStr::new("exec \"$0\""), executable.as_os_str()])
            .map(OsStr::to_os_string)
            .collect();
        Some(Relaunch {
            program: PathBuf::from(shell.unwrap_or_else(|| OsStr::new(self.login_shell()))),
            args,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryPlaced {
    /// The entry was already as the release has it.
    Current,
    /// Some of its files were put in place.
    Refreshed,
    /// The release carries no entry for this OS, as releases before 0.7.0 carry none.
    NotInRelease,
}

/// How to run the binary again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relaunch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl Os {
    /// This OS's Desktop entry in `home`, starting the release install's `bin_link`.
    pub fn desktop_entry(self, home: &Path, bin_link: &Path) -> Arc<dyn DesktopEntry> {
        (self.adapter().desktop_entry)(home, bin_link)
    }
}

/// What the release workflow packs for every OS's entry, so each archive takes its own.
pub fn write_desktop_entry_templates(out: &Path, version: Version) -> anyhow::Result<()> {
    app_bundle::write_template(out, version)?;
    desktop_file::write_template(out)
}

/// Puts `content` at `path` unless it is already there; says whether it wrote.
pub(in crate::platform) fn put_file(path: &Path, content: &[u8]) -> anyhow::Result<bool> {
    if fs::read(path).is_ok_and(|there| there == content) {
        return Ok(false);
    }
    replace(path, |next| fs::write(next, content))?;
    Ok(true)
}

/// Makes `next` beside `path` and renames it over `path`, so `path` is never missing.
pub(in crate::platform) fn replace(
    path: &Path,
    make: impl FnOnce(&Path) -> std::io::Result<()>,
) -> anyhow::Result<()> {
    let folder = path.parent().context("an entry file has no folder")?;
    fs::create_dir_all(folder).with_context(|| format!("could not make {}", folder.display()))?;
    let name = path.file_name().context("an entry file has no name")?;
    let next = folder.join(format!(".{}.next", name.to_string_lossy()));
    if fs::symlink_metadata(&next).is_ok() {
        fs::remove_file(&next).with_context(|| format!("could not remove {}", next.display()))?;
    }
    make(&next).with_context(|| format!("could not write {}", next.display()))?;
    fs::rename(&next, path).with_context(|| format!("could not put {} in place", path.display()))
}
