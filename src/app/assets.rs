use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

/// The app's own pictures, built into the binary, so `cargo install` needs nothing beside it.
pub struct Assets;

impl Assets {
    pub const APP_ICON: &str = include_str!("../../assets/app-icon.svg");
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/search.svg" => include_bytes!("../../assets/icons/search.svg"),
            "icons/external-link.svg" => include_bytes!("../../assets/icons/external-link.svg"),
            "icons/git-pull-request.svg" => {
                include_bytes!("../../assets/icons/git-pull-request.svg")
            }
            "icons/settings.svg" => include_bytes!("../../assets/icons/settings.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
