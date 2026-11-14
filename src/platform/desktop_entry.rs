use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::Os;
use super::linux::desktop_file::{self, DesktopFile};
use super::macos::app_bundle::{self, AppBundle};
use crate::update::Version;

/// Names the app to the OS: the macOS bundle id, the Linux desktop file and window app id.
pub const APP_ID: &str = "io.github.ekvedaras.guided-review";

/// Set on the run the login shell makes, so it does not relaunch again.
pub const IN_LOGIN_SHELL: &str = "GUIDED_REVIEW_IN_LOGIN_SHELL";

pub use app_bundle::info_plist;

/// What the OS launcher starts: the App bundle on macOS, a desktop file and its icon on Linux. A
/// release carries its files; placing them from the release the bin link names keeps it current.
#[derive(Debug, Clone)]
pub struct DesktopEntry {
    form: Form,
}

#[derive(Debug, Clone)]
enum Form {
    AppBundle(AppBundle),
    DesktopFile(DesktopFile),
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
    pub fn desktop_entry(self, home: &Path) -> DesktopEntry {
        let form = match self {
            Self::MacOs => Form::AppBundle(AppBundle::in_home(home)),
            Self::Linux => Form::DesktopFile(DesktopFile::in_home(home)),
        };
        DesktopEntry { form }
    }
}

impl DesktopEntry {
    /// What the reviewer calls the entry, as messages name it.
    pub fn name(&self) -> &'static str {
        match self.form {
            Form::AppBundle(_) => app_bundle::BUNDLE_FOLDER,
            Form::DesktopFile(_) => desktop_file::NAME,
        }
    }

    /// Where the entry is put, as messages name it.
    pub fn folder(&self) -> &'static str {
        match self.form {
            Form::AppBundle(_) => "~/Applications",
            Form::DesktopFile(_) => desktop_file::FOLDER,
        }
    }

    /// Makes the entry match the one carried by the `release` folder.
    pub fn place_from(&self, release: &Path) -> anyhow::Result<EntryPlaced> {
        match &self.form {
            Form::AppBundle(bundle) => bundle.place_from(release),
            Form::DesktopFile(file) => file.place_from(release),
        }
    }

    /// What to start `exe` as when it starts the app.
    pub fn launcher_for(&self, exe: &Path) -> PathBuf {
        match &self.form {
            Form::AppBundle(bundle) => bundle.launcher_for(exe),
            Form::DesktopFile(_) => exe.to_path_buf(),
        }
    }

    /// The environment variable that names the app a launch from the OS launcher started.
    pub fn launched_app_variable(&self) -> &'static str {
        match self.form {
            Form::AppBundle(_) => app_bundle::LAUNCHED_APP,
            Form::DesktopFile(_) => desktop_file::LAUNCHED_APP,
        }
    }

    /// A launch from the OS launcher gets the session's short `PATH`, without `gh`, `claude` or `hc`.
    /// So it runs the entry's binary again through the reviewer's login shell, which reads their
    /// profile as a terminal does.
    pub fn relaunch_in_login_shell(
        &self,
        launched_app: Option<&OsStr>,
        shell: Option<&OsStr>,
        is_in_login_shell: bool,
    ) -> Option<Relaunch> {
        let is_our_app = launched_app == Some(OsStr::new(APP_ID));
        if !is_our_app || is_in_login_shell {
            return None;
        }
        let (executable, login_shell) = match &self.form {
            Form::AppBundle(bundle) => (bundle.executable(), app_bundle::LOGIN_SHELL),
            Form::DesktopFile(file) => (file.executable(), desktop_file::LOGIN_SHELL),
        };
        let args = [OsStr::new("-l"), OsStr::new("-i"), OsStr::new("-c")]
            .into_iter()
            .chain([OsStr::new("exec \"$0\""), executable.as_os_str()])
            .map(OsStr::to_os_string)
            .collect();
        Some(Relaunch {
            program: PathBuf::from(shell.unwrap_or_else(|| OsStr::new(login_shell))),
            args,
        })
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
