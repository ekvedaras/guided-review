use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::app_icon::{AppIcon, Look};
use crate::build::Build;
use crate::platform::desktop_entry::{APP_ID, DesktopEntry, EntryPlaced, put_file};

/// Linux launchers name no app they start, so the desktop file's Exec sets this to the app id.
const LAUNCHED_APP: &str = "GUIDED_REVIEW_LAUNCHED_APP";
/// Each is copied from the release folder to the same path under `~/.local/share`.
const ICON_SIZES: [u16; 2] = [256, 512];

/// `~/.local/share/applications/<app id>.desktop` and its icons in the hicolor theme. The desktop
/// file is written here, not carried by the release, as its Exec names the bin link by its full
/// path: desktop files expand no `~`.
#[derive(Debug, Clone)]
pub struct DesktopFile {
    share: PathBuf,
    bin_link: PathBuf,
}

impl DesktopFile {
    pub fn in_home(home: &Path, bin_link: &Path) -> Self {
        Self {
            share: home.join(".local/share"),
            bin_link: bin_link.to_path_buf(),
        }
    }

    fn contents(&self) -> anyhow::Result<String> {
        let bin_link = self
            .bin_link
            .to_str()
            .with_context(|| format!("{} is not UTF-8", self.bin_link.display()))?;
        Ok(format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Guided Review\n\
             Comment={}\n\
             Exec=env {LAUNCHED_APP}={APP_ID} {}\n\
             Icon={APP_ID}\n\
             Terminal=false\n\
             Categories=Development;\n\
             StartupWMClass={APP_ID}\n",
            env!("CARGO_PKG_DESCRIPTION"),
            exec_argument(bin_link)
        ))
    }
}

impl DesktopEntry for DesktopFile {
    fn name(&self) -> &'static str {
        "Guided Review's desktop entry"
    }

    fn folder(&self) -> &'static str {
        "~/.local/share/applications"
    }

    fn executable(&self) -> PathBuf {
        self.bin_link.clone()
    }

    fn login_shell(&self) -> &'static str {
        "/bin/bash"
    }

    fn launched_app_variable(&self) -> &'static str {
        LAUNCHED_APP
    }

    fn start_program_for(&self, exe: &Path) -> PathBuf {
        exe.to_path_buf()
    }

    fn place_from(&self, release: &Path) -> anyhow::Result<EntryPlaced> {
        let icons = ICON_SIZES.map(icon);
        if !icons.iter().all(|icon| release.join(icon).is_file()) {
            return Ok(EntryPlaced::NotInRelease);
        }
        let mut placed = EntryPlaced::Current;
        for icon in icons {
            let from = release.join(&icon);
            let content =
                fs::read(&from).with_context(|| format!("could not read {}", from.display()))?;
            if put_file(&self.share.join(icon), &content)? {
                placed = EntryPlaced::Refreshed;
            }
        }
        let file = self
            .share
            .join("applications")
            .join(format!("{APP_ID}.desktop"));
        if put_file(&file, self.contents()?.as_bytes())? {
            placed = EntryPlaced::Refreshed;
        }
        Ok(placed)
    }
}

/// Quoted as the desktop entry spec reads an Exec argument, then escaped as its string values are.
fn exec_argument(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' | '`' | '$' => quoted.extend(['\\', '\\', character]),
            '\\' => quoted.push_str("\\\\\\\\"),
            '%' => quoted.push_str("%%"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn icon(size: u16) -> PathBuf {
    PathBuf::from(format!("icons/hicolor/{size}x{size}/apps/{APP_ID}.png"))
}

/// The icons the release archive carries for the Linux entry, at the paths they take under
/// `~/.local/share`.
pub fn write_template(out: &Path) -> anyhow::Result<()> {
    for size in ICON_SIZES {
        let path = out.join(icon(size));
        let folder = path.parent().context("an icon has no folder")?;
        fs::create_dir_all(folder)
            .with_context(|| format!("could not make {}", folder.display()))?;
        fs::write(&path, AppIcon::of(Build::Release, Look::Dark).png(size)?)
            .with_context(|| format!("could not write {}", path.display()))?;
    }
    Ok(())
}
