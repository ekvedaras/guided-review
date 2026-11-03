use std::path::{Path, PathBuf};

/// The `ide` line of `~/.guided-review/config.toml`, with `{path}` and `{line}` to fill in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeTemplate(String);

impl IdeTemplate {
    pub fn new(text: String) -> Self {
        Self(text)
    }

    fn fill(&self, path: &str, line: usize) -> String {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdeLaunch {
    /// Handed to the system, which starts the IDE that owns the scheme.
    Url(String),
    /// Run through `sh -c`.
    Shell(String),
}

impl IdeLaunch {
    /// A URL is handed to `open_url`, because only the caller knows the system's opener.
    pub fn run(&self, open_url: impl FnOnce(&str)) {
        match self {
            Self::Url(url) => open_url(url),
            Self::Shell(command) => {
                if let Err(error) = std::process::Command::new("sh")
                    .args(["-c", command])
                    .spawn()
                {
                    eprintln!("could not run the IDE command: {error}");
                }
            }
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
    pub fn holds(&self, path: &Path) -> bool {
        self.clone_root.join(path).is_file()
    }

    /// `path` is relative to the clone root.
    pub fn launch(&self, path: &Path, line: usize) -> IdeLaunch {
        let absolute = self.clone_root.join(path).to_string_lossy().into_owned();
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
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
