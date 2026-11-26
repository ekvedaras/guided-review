//! The language servers the app downloads itself, the Node that runs the npm ones and the Go that
//! builds and runs gopls: pinned versions, checked by sha256 (sha512 for npm packages, the module
//! hash for gopls), kept in the cache folder.

use std::path::PathBuf;
use std::sync::Arc;

use crate::codebase::Codebase;
use crate::language::Language;
use crate::update::{ReleasesPort, Target};

use super::downloads::Downloads;
use super::recipes::{
    GoToolchain, Gopls, Intelephense, Node, NpmPackage, Pyright, RustAnalyzer, ServerPins,
    ServerRecipe, TypeScriptLanguageServer,
};
use super::{FallbackReason, ServerCatalog, ServerLaunch, Waiting};

/// Where managed servers keep what they need: downloads can be fetched again, indexes are costly to
/// rebuild.
#[derive(Debug, Clone)]
pub struct ServerFolders {
    /// One folder per server and pinned version.
    pub downloads: PathBuf,
    /// One folder per owner and repo, kept across idle stops and app restarts.
    pub indexes: PathBuf,
}

/// Serves the pinned servers from the cache folder, downloading one the first time it is asked for.
pub struct ManagedServers {
    downloads: Downloads,
    rust_analyzer: RustAnalyzer,
    intelephense: Intelephense,
    typescript_language_server: TypeScriptLanguageServer,
    pyright: Pyright,
    gopls: Gopls,
}

impl ManagedServers {
    pub fn new(
        folders: ServerFolders,
        releases: Arc<dyn ReleasesPort>,
        pins: ServerPins,
        target: Target,
    ) -> Self {
        let node = Node { pin: pins.node };
        Self {
            downloads: Downloads::new(folders.downloads, releases, target),
            rust_analyzer: RustAnalyzer {
                pin: pins.rust_analyzer,
            },
            intelephense: Intelephense {
                node: node.clone(),
                package: NpmPackage {
                    name: "intelephense",
                    pin: pins.intelephense,
                    script: "package/lib/intelephense.js",
                },
                indexes: folders.indexes.clone(),
            },
            gopls: Gopls {
                go: GoToolchain { pin: pins.go },
                pin: pins.gopls,
                indexes: folders.indexes,
            },
            pyright: Pyright {
                node: node.clone(),
                package: NpmPackage {
                    name: "pyright",
                    pin: pins.pyright,
                    script: "package/langserver.index.js",
                },
            },
            typescript_language_server: TypeScriptLanguageServer {
                node,
                server: NpmPackage {
                    name: "typescript-language-server",
                    pin: pins.typescript_language_server,
                    script: "package/lib/cli.mjs",
                },
                typescript: NpmPackage {
                    name: "typescript",
                    pin: pins.typescript,
                    script: "package/lib/tsserver.js",
                },
            },
        }
    }

    fn recipe(&self, language: Language) -> &dyn ServerRecipe {
        match language {
            Language::Rust => &self.rust_analyzer,
            Language::Php => &self.intelephense,
            Language::TypeScript | Language::Tsx => &self.typescript_language_server,
            Language::Python => &self.pyright,
            Language::Go => &self.gopls,
        }
    }

    fn recipes(&self) -> [&dyn ServerRecipe; 5] {
        [
            &self.rust_analyzer,
            &self.intelephense,
            &self.typescript_language_server,
            &self.pyright,
            &self.gopls,
        ]
    }

    /// Whether the server of `language` is in the cache folder, which asking for it never checks.
    pub fn is_downloaded(&self, language: Language) -> bool {
        self.recipe(language).is_downloaded(&self.downloads)
    }

    /// Removes every download of a version this build does not pin, and what a stopped unpacking left.
    pub fn remove_unpinned(&self) -> anyhow::Result<()> {
        let kept: Vec<String> = self
            .recipes()
            .iter()
            .flat_map(|recipe| recipe.folders())
            .collect();
        self.downloads.remove_all_but(&kept)
    }
}

impl ServerCatalog for ManagedServers {
    fn launch_for(
        &self,
        language: Language,
        codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        self.recipe(language)
            .launch(&self.downloads, codebase, waiting)
    }
}
