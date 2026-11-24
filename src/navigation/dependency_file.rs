//! The files that say which packages a repo needs, which a language server resolves names against.

use crate::file_path::FilePath;

const NAMES: [&str; 9] = [
    "composer.json",
    "composer.lock",
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "bun.lockb",
];

/// `package.json` counts in any folder, since a monorepo has many; so do the Cargo files.
pub fn is_dependency_file(path: &FilePath) -> bool {
    let name = path.as_str().rsplit('/').next().unwrap_or_default();
    NAMES.contains(&name) || matches!(name, "Cargo.toml" | "Cargo.lock")
}
