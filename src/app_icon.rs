//! The app's icon as pictures: for the Dock while the app runs, and for the bundle's `.icns`.

use resvg::tiny_skia::{Pixmap, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppIcon {
    build: Build,
    look: Look,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Build {
    Release,
    /// A debug build's icon carries a badge, so it is not taken for the installed app in the Dock.
    Dev,
}

/// Light or dark, as macOS shows its windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Light,
    Dark,
}

const DEV_BADGE: &str = include_str!("../assets/app-icon-dev-badge.svg");
const TRAIL_LIGHT: &str = include_str!("../assets/AppIcon.icon/Assets/trail-light.svg");
const TRAIL_DARK: &str = include_str!("../assets/AppIcon.icon/Assets/trail-dark.svg");

/// The icon as Icon Composer keeps it, which `actool` compiles into the bundle's `Assets.car`, so
/// macOS draws it light or dark itself: each file's path inside `AppIcon.icon`, and its content.
pub const ICON_COMPOSER_FILES: [(&str, &str); 3] = [
    (
        "icon.json",
        include_str!("../assets/AppIcon.icon/icon.json"),
    ),
    ("Assets/trail-light.svg", TRAIL_LIGHT),
    ("Assets/trail-dark.svg", TRAIL_DARK),
];

impl AppIcon {
    pub const fn release(look: Look) -> Self {
        Self {
            build: Build::Release,
            look,
        }
    }

    pub const fn dev(look: Look) -> Self {
        Self {
            build: Build::Dev,
            look,
        }
    }

    /// The one a build of this kind shows in the Dock.
    pub const fn running(look: Look) -> Self {
        if cfg!(debug_assertions) {
            Self::dev(look)
        } else {
            Self::release(look)
        }
    }

    /// The rounded square Icon Composer draws from `icon.json`'s fill, for a picture made without it.
    fn ground(self) -> &'static str {
        match self.look {
            Look::Light => include_str!("../assets/app-icon-ground-light.svg"),
            Look::Dark => include_str!("../assets/app-icon-ground-dark.svg"),
        }
    }

    fn trail(self) -> &'static str {
        match self.look {
            Look::Light => TRAIL_LIGHT,
            Look::Dark => TRAIL_DARK,
        }
    }

    /// A square PNG `size` pixels wide.
    pub fn png(self, size: u16) -> anyhow::Result<Vec<u8>> {
        let mut pixmap = Pixmap::new(u32::from(size), u32::from(size))
            .ok_or_else(|| anyhow::anyhow!("no picture can be {size} pixels wide"))?;
        let options = usvg::Options::default();
        paint(self.ground(), &options, &mut pixmap)?;
        paint(self.trail(), &options, &mut pixmap)?;
        if self.build == Build::Dev {
            let mut options = usvg::Options::default();
            options.fontdb_mut().load_system_fonts();
            paint(DEV_BADGE, &options, &mut pixmap)?;
        }
        Ok(pixmap.encode_png()?)
    }
}

fn paint(svg: &str, options: &usvg::Options, pixmap: &mut Pixmap) -> anyhow::Result<()> {
    let tree = usvg::Tree::from_str(svg, options)?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "an icon is at most a few thousand pixels wide"
    )]
    let scale = pixmap.width() as f32 / tree.size().width();
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(())
}
