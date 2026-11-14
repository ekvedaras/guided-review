#[cfg(target_os = "macos")]
pub mod dock_icon;
#[cfg(target_os = "macos")]
pub mod word_keys;

pub const TRIPLE_SYSTEM: &str = "apple-darwin";

/// Both come with macOS.
pub const TEXT_FONTS: &[&str] = &["Helvetica Neue"];
pub const CODE_FONTS: &[&str] = &["Menlo"];
