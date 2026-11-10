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
            "icons/check.svg" => include_bytes!("../../assets/icons/check.svg"),
            "icons/minus.svg" => include_bytes!("../../assets/icons/minus.svg"),
            "icons/chevron-right.svg" => include_bytes!("../../assets/icons/chevron-right.svg"),
            "icons/chevron-down.svg" => include_bytes!("../../assets/icons/chevron-down.svg"),
            "icons/arrow-left.svg" => include_bytes!("../../assets/icons/arrow-left.svg"),
            "icons/arrow-right.svg" => include_bytes!("../../assets/icons/arrow-right.svg"),
            "icons/x.svg" => include_bytes!("../../assets/icons/x.svg"),
            "icons/arrow-up.svg" => include_bytes!("../../assets/icons/arrow-up.svg"),
            "icons/arrow-down.svg" => include_bytes!("../../assets/icons/arrow-down.svg"),
            "icons/corner-down-left.svg" => {
                include_bytes!("../../assets/icons/corner-down-left.svg")
            }
            "icons/ellipsis.svg" => include_bytes!("../../assets/icons/ellipsis.svg"),
            "icons/list-checks.svg" => include_bytes!("../../assets/icons/list-checks.svg"),
            "icons/plus.svg" => include_bytes!("../../assets/icons/plus.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
