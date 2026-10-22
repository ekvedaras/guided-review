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

/// Open in IDE exists only with a local clone and a template; a `None` of this hides the button.
#[derive(Debug, Clone)]
pub struct Ide {
    template: IdeTemplate,
    clone_root: PathBuf,
}

impl Ide {
    pub fn for_clone(clone_root: Option<PathBuf>, template: Option<IdeTemplate>) -> Option<Self> {
        Some(Self {
            template: template?,
            clone_root: clone_root?,
        })
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
