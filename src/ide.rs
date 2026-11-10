use std::fmt::Write;
use std::path::PathBuf;

use crate::file_path::FilePath;
use crate::line_number::LineNumber;

/// The `ide` line of `~/.guided-review/config.toml`, with `{path}` and `{line}` to fill in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeTemplate(String);

impl IdeTemplate {
    pub fn new(text: String) -> Self {
        Self(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn fill(&self, path: &str, line: LineNumber) -> String {
        self.0
            .replace("{path}", path)
            .replace("{line}", &line.to_string())
    }

    fn is_url(&self) -> bool {
        let scheme = self.0.split("://").next().unwrap_or_default();
        self.0.contains("://")
            && !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }
}

/// The IDE the reviewer picks in Settings: one with a known URL scheme, none, or their own template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdeChoice {
    JetBrains,
    VsCode,
    Zed,
    None,
    Custom(IdeTemplate),
}

impl IdeChoice {
    pub fn custom_text(&self) -> Option<&str> {
        match self {
            Self::Custom(template) => Some(template.as_str()),
            _ => None,
        }
    }

    /// The template a preset hands to the system, so the reviewer sees what it opens.
    pub fn shown_template(&self) -> Option<&'static str> {
        match self {
            Self::JetBrains => Some("idea://open?file={path}&line={line}"),
            Self::VsCode => Some("vscode://file{path}:{line}"),
            Self::Zed => Some("zed://file{path}:{line}"),
            Self::None | Self::Custom(_) => None,
        }
    }

    const PRESETS: [Self; 3] = [Self::JetBrains, Self::VsCode, Self::Zed];

    pub fn template(&self) -> Option<IdeTemplate> {
        match self {
            Self::Custom(template) => Some(template.clone()),
            preset => preset
                .shown_template()
                .map(|url| IdeTemplate::new(url.to_owned())),
        }
    }

    /// A template equal to a preset's is that preset, so a config written by hand and one written by Settings read alike.
    pub fn of(template: Option<&IdeTemplate>) -> Self {
        let Some(template) = template else {
            return Self::None;
        };
        Self::PRESETS
            .into_iter()
            .find(|preset| preset.template().as_ref() == Some(template))
            .unwrap_or_else(|| Self::Custom(template.clone()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdeLaunch {
    /// Handed to the system, which starts the IDE that owns the scheme.
    Url(String),
    /// Run through `sh -c`.
    Shell(String),
}

impl IdeLaunch {
    /// A URL is handed to `open_url`, because only the caller knows the system's opener.
    pub fn run(&self, open_url: impl FnOnce(&str)) -> std::io::Result<()> {
        match self {
            Self::Url(url) => {
                open_url(url);
                Ok(())
            }
            Self::Shell(command) => std::process::Command::new("sh")
                .args(["-c", command])
                .spawn()
                .map(drop),
        }
    }
}

/// What keeps code from opening in the IDE at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoIde {
    NoTemplate,
    NoClone,
}

/// Code opens in the IDE only with a local clone and a template; without one it opens on GitHub.
#[derive(Debug, Clone)]
pub struct Ide {
    template: IdeTemplate,
    clone_root: PathBuf,
}

impl Ide {
    pub fn for_clone(
        clone_root: Option<PathBuf>,
        template: Option<IdeTemplate>,
    ) -> Result<Self, NoIde> {
        Ok(Self {
            template: template.ok_or(NoIde::NoTemplate)?,
            clone_root: clone_root.ok_or(NoIde::NoClone)?,
        })
    }

    /// Whether the working copy has the file; it may be on another branch than the PR.
    pub fn holds(&self, path: &FilePath) -> bool {
        path.under(&self.clone_root).is_file()
    }

    /// `path` is relative to the clone root.
    pub fn launch(&self, path: &FilePath, line: LineNumber) -> IdeLaunch {
        let absolute = path.under(&self.clone_root).to_string_lossy().into_owned();
        if self.template.is_url() {
            IdeLaunch::Url(self.template.fill(&percent_encode_path(&absolute), line))
        } else {
            IdeLaunch::Shell(self.template.fill(&shell_quote(&absolute), line))
        }
    }
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            write!(encoded, "%{byte:02X}").ok();
        }
    }
    encoded
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
