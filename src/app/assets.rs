use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

/// The app's own pictures, built into the binary, so `cargo install` needs nothing beside it.
pub struct Assets;

impl Assets {
    pub const APP_ICON: &str = include_str!("../../assets/app-icon.svg");
    /// A debug build's icon carries a badge, so it is not taken for the installed app in the Dock.
    pub const DEV_APP_ICON: &str = include_str!("../../assets/app-icon-dev.svg");
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
            "icons/sparkles.svg" => include_bytes!("../../assets/icons/sparkles.svg"),
            "icons/message-square.svg" => include_bytes!("../../assets/icons/message-square.svg"),
            "icons/reply.svg" => include_bytes!("../../assets/icons/reply.svg"),
            "icons/circle-check.svg" => include_bytes!("../../assets/icons/circle-check.svg"),
            "icons/rotate-ccw.svg" => include_bytes!("../../assets/icons/rotate-ccw.svg"),
            "icons/pencil.svg" => include_bytes!("../../assets/icons/pencil.svg"),
            "icons/trash.svg" => include_bytes!("../../assets/icons/trash.svg"),
            "icons/file-text.svg" => include_bytes!("../../assets/icons/file-text.svg"),
            "icons/file-plus.svg" => include_bytes!("../../assets/icons/file-plus.svg"),
            "icons/folder-open.svg" => include_bytes!("../../assets/icons/folder-open.svg"),
            "icons/folder-git.svg" => include_bytes!("../../assets/icons/folder-git.svg"),
            "icons/save.svg" => include_bytes!("../../assets/icons/save.svg"),
            "icons/refresh.svg" => include_bytes!("../../assets/icons/refresh.svg"),
            "icons/terminal.svg" => include_bytes!("../../assets/icons/terminal.svg"),
            "icons/wrap-text.svg" => include_bytes!("../../assets/icons/wrap-text.svg"),
            "icons/space.svg" => include_bytes!("../../assets/icons/space.svg"),
            "icons/code.svg" => include_bytes!("../../assets/icons/code.svg"),
            "icons/circle-alert.svg" => include_bytes!("../../assets/icons/circle-alert.svg"),
            "icons/send.svg" => include_bytes!("../../assets/icons/send.svg"),
            "icons/diff.svg" => include_bytes!("../../assets/icons/diff.svg"),
            "icons/globe.svg" => include_bytes!("../../assets/icons/globe.svg"),
            "icons/copy.svg" => include_bytes!("../../assets/icons/copy.svg"),
            "icons/square.svg" => include_bytes!("../../assets/icons/square.svg"),
            "icons/flag.svg" => include_bytes!("../../assets/icons/flag.svg"),
            "icons/keyboard.svg" => include_bytes!("../../assets/icons/keyboard.svg"),
            "icons/zap.svg" => include_bytes!("../../assets/icons/zap.svg"),
            "icons/smile-plus.svg" => include_bytes!("../../assets/icons/smile-plus.svg"),
            "icons/file-pen.svg" => include_bytes!("../../assets/icons/file-pen.svg"),
            "icons/chevrons-up-down.svg" => {
                include_bytes!("../../assets/icons/chevrons-up-down.svg")
            }
            "icons/milestone.svg" => include_bytes!("../../assets/icons/milestone.svg"),
            "icons/braces.svg" => include_bytes!("../../assets/icons/braces.svg"),
            "icons/circle-dot.svg" => include_bytes!("../../assets/icons/circle-dot.svg"),
            "icons/chevron-up.svg" => include_bytes!("../../assets/icons/chevron-up.svg"),
            "icons/clock.svg" => include_bytes!("../../assets/icons/clock.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
