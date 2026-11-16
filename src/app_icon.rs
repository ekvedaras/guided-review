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

    fn ground(self) -> &'static str {
        match self.look {
            Look::Light => include_str!("../assets/app-icon-light.svg"),
            Look::Dark => include_str!("../assets/app-icon-dark.svg"),
        }
    }

    /// A square PNG `size` pixels wide.
    pub fn png(self, size: u16) -> anyhow::Result<Vec<u8>> {
        let mut pixmap = Pixmap::new(u32::from(size), u32::from(size))
            .ok_or_else(|| anyhow::anyhow!("no picture can be {size} pixels wide"))?;
        paint(self.ground(), &usvg::Options::default(), &mut pixmap)?;
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
