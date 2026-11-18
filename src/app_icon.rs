//! The app's icon as pictures: for the Dock while the app runs, and for the bundle's `.icns`.

use resvg::tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};
use sha2::{Digest, Sha256};

use crate::update::Build;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppIcon {
    build: Build,
    look: Look,
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

/// The SHA-256 of [`ICON_COMPOSER_FILES`]' contents, in order, as `scripts/app-icon-pictures` takes it.
pub fn icon_composer_digest() -> String {
    let mut digest = Sha256::new();
    for (_, content) in ICON_COMPOSER_FILES {
        digest.update(content.as_bytes());
    }
    crate::hex::lower(&digest.finalize())
}

/// What `scripts/app-icon-pictures` noted beside the pictures it exported.
pub fn digest_the_pictures_were_exported_from() -> &'static str {
    include_str!("../assets/app-icon-pictures.sha256").trim()
}

impl AppIcon {
    pub const fn of(build: Build, look: Look) -> Self {
        Self { build, look }
    }

    /// A local build's icon carries the `DEV` badge, so it is not taken for the release install in
    /// the Dock. The snapshot build stays out of the Dock, so it needs none.
    const fn wears_dev_badge(self) -> bool {
        matches!(self.build, Build::Local)
    }

    /// The look as Icon Composer draws it, exported by `scripts/app-icon-pictures`: the glass, corners
    /// and shine macOS gives the bundle's icon, which a picture of the app's own would not match.
    const fn exported(self) -> &'static [u8] {
        match self.look {
            Look::Light => include_bytes!("../assets/app-icon-light.png"),
            Look::Dark => include_bytes!("../assets/app-icon-dark.png"),
        }
    }

    /// A square PNG `size` pixels wide.
    pub fn png(self, size: u16) -> anyhow::Result<Vec<u8>> {
        let mut pixmap = Pixmap::new(u32::from(size), u32::from(size))
            .ok_or_else(|| anyhow::anyhow!("no picture can be {size} pixels wide"))?;
        place_on_icon_grid(&Pixmap::decode_png(self.exported())?, &mut pixmap);
        if self.wears_dev_badge() {
            let mut options = usvg::Options::default();
            options.fontdb_mut().load_system_fonts();
            paint(DEV_BADGE, &options, &mut pixmap)?;
        }
        Ok(pixmap.encode_png()?)
    }
}

/// macOS draws an icon's shape 824 of 1024 pixels wide, centred; the export fills its whole picture.
fn place_on_icon_grid(exported: &Pixmap, pixmap: &mut Pixmap) {
    const SHAPE_SHARE: f32 = 824. / 1024.;
    #[expect(
        clippy::cast_precision_loss,
        reason = "an icon is at most a few thousand pixels wide"
    )]
    let (width, exported_width) = (pixmap.width() as f32, exported.width() as f32);
    let scale = width * SHAPE_SHARE / exported_width;
    let margin = width * (1. - SHAPE_SHARE) / 2.;
    pixmap.draw_pixmap(
        0,
        0,
        exported.as_ref(),
        &PixmapPaint {
            quality: FilterQuality::Bicubic,
            ..PixmapPaint::default()
        },
        Transform::from_row(scale, 0., 0., scale, margin, margin),
        None,
    );
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
