//! The app's icon as pictures: for the Dock while the app runs, and for the bundle's `.icns`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppIcon {
    Release,
    /// A debug build's icon carries a badge, so it is not taken for the installed app in the Dock.
    Dev,
}

impl AppIcon {
    /// The one a build of this kind shows in the Dock.
    pub const RUNNING: Self = if cfg!(debug_assertions) {
        Self::Dev
    } else {
        Self::Release
    };

    fn svg(self) -> &'static str {
        match self {
            Self::Release => include_str!("../assets/app-icon.svg"),
            Self::Dev => include_str!("../assets/app-icon-dev.svg"),
        }
    }

    /// A square PNG `size` pixels wide.
    pub fn png(self, size: u16) -> anyhow::Result<Vec<u8>> {
        let mut options = usvg::Options::default();
        if self == Self::Dev {
            options.fontdb_mut().load_system_fonts();
        }
        let tree = usvg::Tree::from_str(self.svg(), &options)?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(u32::from(size), u32::from(size))
            .ok_or_else(|| anyhow::anyhow!("no picture can be {size} pixels wide"))?;
        let scale = f32::from(size) / tree.size().width();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        Ok(pixmap.encode_png()?)
    }
}
