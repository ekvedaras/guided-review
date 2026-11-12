use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::Version;
use crate::app_icon::AppIcon;

/// The bundle's folder name, in a release archive and in `~/Applications`.
pub const BUNDLE_FOLDER: &str = "Guided Review.app";
const APP_NAME: &str = "Guided Review";
const BUNDLE_ID: &str = "io.github.ekvedaras.guided-review";
const EXECUTABLE: &str = "guided-review";
const ICON: &str = "AppIcon";
const MINIMUM_MACOS: &str = "11.0";
/// The files a release's bundle carries; the executable is not one, as it links to the bin link.
const INFO_PLIST: &str = "Contents/Info.plist";
const CARRIED: [&str; 2] = [INFO_PLIST, "Contents/Resources/AppIcon.icns"];
const EXECUTABLE_PATH: &str = "Contents/MacOS/guided-review";

/// `~/Applications/Guided Review.app`: what the Dock, Spotlight and Raycast start. A real folder, as
/// Spotlight does not index a linked one; its binary is a link to the bin link, so it runs whatever
/// release the bin link names.
#[derive(Debug, Clone)]
pub struct AppBundle {
    folder: PathBuf,
    bin_link: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundlePlaced {
    /// The bundle was already as the release has it.
    Current,
    /// Some of its files were put in place.
    Refreshed,
    /// The release carries no bundle, as releases before 0.7.0 do not.
    NotInRelease,
}

impl AppBundle {
    pub fn in_home(home: &Path) -> Self {
        Self {
            folder: home.join("Applications").join(BUNDLE_FOLDER),
            bin_link: home.join(".local/bin/guided-review"),
        }
    }

    pub fn executable(&self) -> PathBuf {
        self.folder.join(EXECUTABLE_PATH)
    }

    /// What to start `exe` as: the bundle's executable when that is `exe`, so the Dock names the app
    /// Guided Review and shows its icon even when the CLI starts it, else `exe` itself.
    pub fn launcher_for(&self, exe: &Path) -> PathBuf {
        let bundled = self.executable();
        let is_same = match (fs::canonicalize(&bundled), fs::canonicalize(exe)) {
            (Ok(bundled), Ok(exe)) => bundled == exe,
            _ => false,
        };
        if is_same { bundled } else { exe.to_path_buf() }
    }

    /// Makes the bundle match `template`, the bundle inside a release folder, one file at a time: each
    /// is written beside its place and renamed over it, so the bundle never lacks a file, and a file
    /// already the same is left alone.
    pub fn place_from(&self, template: &Path) -> anyhow::Result<BundlePlaced> {
        if !template.join(INFO_PLIST).is_file() {
            return Ok(BundlePlaced::NotInRelease);
        }
        let mut placed = BundlePlaced::Current;
        for file in CARRIED {
            let (from, to) = (template.join(file), self.folder.join(file));
            let content =
                fs::read(&from).with_context(|| format!("could not read {}", from.display()))?;
            if fs::read(&to).is_ok_and(|there| there == content) {
                continue;
            }
            replace(&to, |next| fs::write(next, &content))?;
            placed = BundlePlaced::Refreshed;
        }
        let executable = self.executable();
        if fs::read_link(&executable).ok().as_deref() != Some(self.bin_link.as_path()) {
            replace(&executable, |next| symlink(&self.bin_link, next))?;
            placed = BundlePlaced::Refreshed;
        }
        if placed == BundlePlaced::Refreshed {
            self.mark_changed()?;
        }
        Ok(placed)
    }
}

impl AppBundle {
    /// Finder and Launch Services read a bundle's Info.plist again only once the bundle folder itself
    /// looks changed.
    fn mark_changed(&self) -> anyhow::Result<()> {
        fs::File::open(&self.folder)
            .and_then(|folder| folder.set_modified(std::time::SystemTime::now()))
            .with_context(|| format!("could not mark {} changed", self.folder.display()))
    }
}

/// Makes `next` beside `path` and renames it over `path`.
fn replace(path: &Path, make: impl FnOnce(&Path) -> std::io::Result<()>) -> anyhow::Result<()> {
    let folder = path.parent().context("a bundle file has no folder")?;
    fs::create_dir_all(folder).with_context(|| format!("could not make {}", folder.display()))?;
    let name = path.file_name().context("a bundle file has no name")?;
    let next = folder.join(format!(".{}.next", name.to_string_lossy()));
    if fs::symlink_metadata(&next).is_ok() {
        fs::remove_file(&next).with_context(|| format!("could not remove {}", next.display()))?;
    }
    make(&next).with_context(|| format!("could not write {}", next.display()))?;
    fs::rename(&next, path).with_context(|| format!("could not put {} in place", path.display()))
}

/// Set on the run the login shell makes, so it does not relaunch again.
pub const IN_LOGIN_SHELL: &str = "GUIDED_REVIEW_IN_LOGIN_SHELL";

/// How to run the binary again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relaunch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// A launch from the Dock, Spotlight or Raycast gets launchd's short `PATH`, without `gh`, `claude` or
/// `hc`. So a binary started from inside a bundle runs itself again through the reviewer's login
/// shell, which reads their profile as a terminal does, keeping the bundle's path so the Dock still
/// names the app.
pub fn relaunch_in_login_shell(
    exe: &Path,
    shell: Option<&OsStr>,
    is_in_login_shell: bool,
) -> Option<Relaunch> {
    let is_bundled = exe
        .parent()
        .is_some_and(|folder| folder.ends_with("Contents/MacOS"))
        && exe
            .ancestors()
            .nth(3)
            .and_then(Path::extension)
            .is_some_and(|extension| extension == "app");
    if !is_bundled || is_in_login_shell {
        return None;
    }
    let args = [OsStr::new("-l"), OsStr::new("-i"), OsStr::new("-c")]
        .into_iter()
        .chain([OsStr::new("exec \"$0\""), exe.as_os_str()])
        .map(OsStr::to_os_string)
        .collect();
    Some(Relaunch {
        program: PathBuf::from(shell.unwrap_or_else(|| OsStr::new("/bin/zsh"))),
        args,
    })
}

/// What macOS reads to name the app, find its binary and icon, and tell its version.
pub fn info_plist(version: Version) -> String {
    let entries = [
        ("CFBundleName", APP_NAME.to_owned()),
        ("CFBundleDisplayName", APP_NAME.to_owned()),
        ("CFBundleIdentifier", BUNDLE_ID.to_owned()),
        ("CFBundleExecutable", EXECUTABLE.to_owned()),
        ("CFBundleIconFile", ICON.to_owned()),
        ("CFBundleShortVersionString", version.to_string()),
        ("CFBundleVersion", version.to_string()),
        ("CFBundlePackageType", "APPL".to_owned()),
        ("LSMinimumSystemVersion", MINIMUM_MACOS.to_owned()),
    ];
    let strings = entries
        .iter()
        .map(|(key, value)| format!("\t<key>{key}</key>\n\t<string>{value}</string>\n"))
        .collect::<Vec<_>>()
        .concat();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         {strings}\
         \t<key>NSHighResolutionCapable</key>\n\t<true/>\n</dict>\n</plist>\n"
    )
}

/// What the release workflow packs: `Guided Review.app` with its Info.plist, and beside it the
/// `AppIcon.iconset` that `iconutil` turns into the bundle's `AppIcon.icns`.
pub fn write_bundle_template(out: &Path, version: Version) -> anyhow::Result<()> {
    let contents = out.join(BUNDLE_FOLDER).join("Contents");
    fs::create_dir_all(contents.join("Resources"))
        .with_context(|| format!("could not make {}", contents.display()))?;
    fs::write(contents.join("Info.plist"), info_plist(version))
        .context("could not write the Info.plist")?;
    let iconset = out.join(format!("{ICON}.iconset"));
    fs::create_dir_all(&iconset)
        .with_context(|| format!("could not make {}", iconset.display()))?;
    for size in [16, 32, 128, 256, 512] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let picture = AppIcon::Release.png(size * scale)?;
            let name = format!("icon_{size}x{size}{suffix}.png");
            fs::write(iconset.join(&name), picture)
                .with_context(|| format!("could not write {name}"))?;
        }
    }
    Ok(())
}
