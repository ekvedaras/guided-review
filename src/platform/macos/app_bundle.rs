use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::app_icon::ICON_COMPOSER_FILES;
use crate::platform::desktop_entry::{APP_ID, DesktopEntry, EntryPlaced, put_file, replace};
use crate::update::Version;

/// The bundle's folder name, in a release archive and in [`APPLICATIONS`].
pub const BUNDLE_FOLDER: &str = "Guided Review.app";
const APPLICATIONS: &str = "~/Applications";
const APP_NAME: &str = "Guided Review";
const EXECUTABLE: &str = "guided-review";
const ICON: &str = "AppIcon";
const MINIMUM_MACOS: &str = "11.0";
/// The files a release's bundle carries; the executable is not one, as it links to the bin link.
const INFO_PLIST: &str = "Contents/Info.plist";
const CARRIED: [&str; 3] = [
    INFO_PLIST,
    "Contents/Resources/AppIcon.icns",
    "Contents/Resources/Assets.car",
];
const EXECUTABLE_PATH: &str = "Contents/MacOS/guided-review";

/// Launch Services names the app it launched here. The bundle's executable is a link to the bin link,
/// which macOS follows before it runs it, so the binary's own path does not tell a launch of the app.
const LAUNCHED_APP: &str = "__CFBundleIdentifier";

/// `~/Applications/Guided Review.app`: what the Dock, Spotlight and Raycast start. A real folder, as
/// Spotlight does not index a linked one; its binary is a link to the bin link, so it runs whatever
/// release the bin link names.
#[derive(Debug, Clone)]
pub struct AppBundle {
    folder: PathBuf,
    bin_link: PathBuf,
}

impl AppBundle {
    pub fn in_home(home: &Path, bin_link: &Path) -> Self {
        let applications = home.join(APPLICATIONS.trim_start_matches("~/"));
        Self {
            folder: applications.join(BUNDLE_FOLDER),
            bin_link: bin_link.to_path_buf(),
        }
    }

    /// Finder and Launch Services read a bundle's Info.plist again only once the bundle folder itself
    /// looks changed.
    fn mark_changed(&self) -> anyhow::Result<()> {
        fs::File::open(&self.folder)
            .and_then(|folder| folder.set_modified(std::time::SystemTime::now()))
            .with_context(|| format!("could not mark {} changed", self.folder.display()))
    }
}

impl DesktopEntry for AppBundle {
    fn name(&self) -> &'static str {
        BUNDLE_FOLDER
    }

    fn folder(&self) -> &'static str {
        APPLICATIONS
    }

    fn executable(&self) -> PathBuf {
        self.folder.join(EXECUTABLE_PATH)
    }

    fn login_shell(&self) -> &'static str {
        "/bin/zsh"
    }

    fn launched_app_variable(&self) -> &'static str {
        LAUNCHED_APP
    }

    /// The bundle's executable when that is `exe`, so the Dock names the app Guided Review and
    /// shows its icon even when the CLI starts it, else `exe` itself.
    fn start_program_for(&self, exe: &Path) -> PathBuf {
        let bundled = self.executable();
        let is_same = match (fs::canonicalize(&bundled), fs::canonicalize(exe)) {
            (Ok(bundled), Ok(exe)) => bundled == exe,
            _ => false,
        };
        if is_same { bundled } else { exe.to_path_buf() }
    }

    /// One file at a time, so the bundle never lacks a file, and a file already the same is left
    /// alone.
    fn place_from(&self, release: &Path) -> anyhow::Result<EntryPlaced> {
        let template = release.join(BUNDLE_FOLDER);
        if !template.join(INFO_PLIST).is_file() {
            return Ok(EntryPlaced::NotInRelease);
        }
        let mut placed = EntryPlaced::Current;
        for file in CARRIED {
            let from = template.join(file);
            let content =
                fs::read(&from).with_context(|| format!("could not read {}", from.display()))?;
            if put_file(&self.folder.join(file), &content)? {
                placed = EntryPlaced::Refreshed;
            }
        }
        let executable = self.executable();
        if fs::read_link(&executable).ok().as_deref() != Some(self.bin_link.as_path()) {
            replace(&executable, |next| symlink(&self.bin_link, next))?;
            placed = EntryPlaced::Refreshed;
        }
        if placed == EntryPlaced::Refreshed {
            self.mark_changed()?;
        }
        Ok(placed)
    }
}

/// What macOS reads to name the app, find its binary and icon, and tell its version.
pub fn info_plist(version: Version) -> String {
    let entries = [
        ("CFBundleName", APP_NAME.to_owned()),
        ("CFBundleDisplayName", APP_NAME.to_owned()),
        ("CFBundleIdentifier", APP_ID.to_owned()),
        ("CFBundleExecutable", EXECUTABLE.to_owned()),
        ("CFBundleIconFile", ICON.to_owned()),
        ("CFBundleIconName", ICON.to_owned()),
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
/// `AppIcon.icon` that `actool` compiles into the bundle's `Assets.car` and `AppIcon.icns`.
pub fn write_template(out: &Path, version: Version) -> anyhow::Result<()> {
    let contents = out.join(BUNDLE_FOLDER).join("Contents");
    fs::create_dir_all(contents.join("Resources"))
        .with_context(|| format!("could not make {}", contents.display()))?;
    fs::write(contents.join("Info.plist"), info_plist(version))
        .context("could not write the Info.plist")?;
    let icon = out.join(format!("{ICON}.icon"));
    for (path, content) in ICON_COMPOSER_FILES {
        let file = icon.join(path);
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)
                .with_context(|| format!("could not make {}", folder.display()))?;
        }
        fs::write(&file, content).with_context(|| format!("could not write {}", file.display()))?;
    }
    Ok(())
}
