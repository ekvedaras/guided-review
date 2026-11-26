//! One recipe per managed server: the versions it pins, the folders they download into, how it is
//! fetched and how it starts. Node is no server but the runtime the npm servers share, and Go the
//! toolchain gopls is built with and runs.

use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context as _;
use flate2::read::GzDecoder;
use serde_json::json;
use sha2::{Digest as _, Sha512};

use crate::codebase::Codebase;
use crate::platform::Os;
use crate::update::{Cpu, Target, check_digest};

use super::downloads::{Downloads, place, write_program};
use super::{FallbackReason, ServerEnvironment, ServerLaunch, ServerName, StatusReports, Waiting};

/// One release of a server: its tag and the sha256 of its download for each target it is built for.
#[derive(Debug, Clone)]
pub struct ReleasePin {
    pub tag: String,
    pub sha256: Vec<(Target, String)>,
}

/// One version of an npm package and the sha512 of its registry tarball, in hex: the hash npm's
/// `integrity` gives in base64.
#[derive(Debug, Clone)]
pub struct PackagePin {
    pub version: String,
    pub sha512: String,
}

/// One version of a Go module and its hash as `go.sum` and sum.golang.org name it (`h1:`).
#[derive(Debug, Clone)]
pub struct ModulePin {
    pub version: String,
    pub h1: String,
}

/// The server versions this build of the app knows.
#[derive(Debug, Clone)]
pub struct ServerPins {
    pub rust_analyzer: ReleasePin,
    /// Runs the servers that are npm packages.
    pub node: ReleasePin,
    pub intelephense: PackagePin,
    pub typescript_language_server: PackagePin,
    /// The tsserver typescript-language-server runs, whatever TypeScript the reviewed repo installs.
    pub typescript: PackagePin,
    pub pyright: PackagePin,
    /// Builds gopls and runs the `go` commands gopls runs.
    pub go: ReleasePin,
    pub gopls: ModulePin,
}

impl ServerPins {
    /// The versions of this build.
    pub fn pinned() -> Self {
        let target = |os, cpu, sha: &str| (Target { os, cpu }, sha.to_owned());
        Self {
            rust_analyzer: ReleasePin {
                tag: "2026-11-23".to_owned(),
                sha256: vec![
                    target(
                        Os::MacOs,
                        Cpu::Arm64,
                        "9d996784c9bc811642edd9d993c8c85e12c7111463a8e426d5e74c27da1bc0a5",
                    ),
                    target(
                        Os::MacOs,
                        Cpu::X86_64,
                        "4673e51749f937567b77813ef12621b6e11e6ba9c1b591c58f8a98fbbc9f5bac",
                    ),
                    target(
                        Os::Linux,
                        Cpu::Arm64,
                        "cf7e9c2cd34540da92473d47f0866c9515425da01116e7aaca988c9b19408285",
                    ),
                    target(
                        Os::Linux,
                        Cpu::X86_64,
                        "99748126069cc1360e2729e45ae7b563a2b885def41c0d600de8e5977a7adf13",
                    ),
                ],
            },
            node: ReleasePin {
                tag: "v26.13.1".to_owned(),
                sha256: vec![
                    target(
                        Os::MacOs,
                        Cpu::Arm64,
                        "aec2514ea1ceb2ba52455e9ec65986f6749a06dd759e5db2a72e6889668c9735",
                    ),
                    target(
                        Os::MacOs,
                        Cpu::X86_64,
                        "82bb96078a24ca50b9c9a38f2379e6be6d87171fcd400fc14c8934b3bce96e2f",
                    ),
                    target(
                        Os::Linux,
                        Cpu::Arm64,
                        "80a330b35415382878df18d41afbd106ea89bfb3be0087d6608e289a14c5cc7d",
                    ),
                    target(
                        Os::Linux,
                        Cpu::X86_64,
                        "518674de2b621728a5e4486f5fb3b7d2900bb3cc4aa97969edaa463db99f9778",
                    ),
                ],
            },
            intelephense: PackagePin {
                version: "1.18.5".to_owned(),
                sha512: "76a087d583424651c604b343fa25058c12651b0338a4f8a65f68e6f0068fffa2b659934cd8adbe13c74e5820f765830fdb30b6202043f52a1eda8c4ab54f5df4".to_owned(),
            },
            typescript_language_server: PackagePin {
                version: "6.1.0".to_owned(),
                sha512: "fcefddaf609bbc34b55bd8ca25c050c64e166a0510f7ba016fc93f7aaf5c3ac96c6ed02e0b60433e991c50bc44d9211988f209bd534b78d8cb65ef61425d7309".to_owned(),
            },
            // TypeScript 7 is a native program with no tsserver.js, so 6.0 is the last one
            // typescript-language-server can run.
            typescript: PackagePin {
                version: "6.0.3".to_owned(),
                sha512: "cb64efbb14993c3c906a490544f6472859be28a56a222b1d83dfc26709bd7edbca5cb3fc3515a3dfcfce0e335baf8dd2b285ea36e022b047f519d0b1a9671d07".to_owned(),
            },
            pyright: PackagePin {
                version: "1.1.414".to_owned(),
                sha512: "14f6596f9d637a90d7e1e3fb4c460378d16d984dd683092411c269bc7dff426512b23d04032dcb5eefb10784ff1560de8edb179427c5affadb5858f0bb0b97c3".to_owned(),
            },
            go: ReleasePin {
                tag: "1.27.4".to_owned(),
                sha256: vec![
                    target(
                        Os::MacOs,
                        Cpu::Arm64,
                        "91a44a2b8a716600ad94a4f143e156116620db8828086a6e3602291be9bd0bb4",
                    ),
                    target(
                        Os::MacOs,
                        Cpu::X86_64,
                        "2b799377704ef12db39bd254995d5c348c9cc8d86562ca817724390e56c38f43",
                    ),
                    target(
                        Os::Linux,
                        Cpu::Arm64,
                        "3392582988a273a87ab5043ee1b2c9c17a2c3c00580bf9326c47a2e0aaaeac92",
                    ),
                    target(
                        Os::Linux,
                        Cpu::X86_64,
                        "7226736c2339719cda28fbf3a54d71aa63bcefe376e98cef00034268dab24840",
                    ),
                ],
            },
            gopls: ModulePin {
                version: "v0.24.0".to_owned(),
                h1: "h1:nPO/LffYa4sP62Ueuema5PPnZPGaZjGl+vZGvbTek4g=".to_owned(),
            },
        }
    }
}

impl ReleasePin {
    fn sha256_for(&self, target: Target) -> anyhow::Result<&str> {
        self.sha256
            .iter()
            .find(|(pinned, _)| *pinned == target)
            .map(|(_, sha256)| sha256.as_str())
            .with_context(|| format!("none is pinned for {}", target.triple()))
    }
}

/// What the app needs to download and start one language's server.
pub(super) trait ServerRecipe: Send + Sync {
    /// The download folders of the pinned versions, which removing unpinned downloads keeps.
    fn folders(&self) -> Vec<String>;

    fn is_downloaded(&self, downloads: &Downloads) -> bool;

    /// Downloads what is missing, telling `waiting`, and says how to start the server.
    fn launch(
        &self,
        downloads: &Downloads,
        codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason>;
}

pub(super) struct RustAnalyzer {
    pub(super) pin: ReleasePin,
}

impl RustAnalyzer {
    fn folder(&self) -> String {
        format!("rust-analyzer-{}", self.pin.tag)
    }

    fn binary(&self, downloads: &Downloads) -> PathBuf {
        downloads.folder(&self.folder()).join("rust-analyzer")
    }

    /// The release is one gzipped binary.
    fn download(&self, downloads: &Downloads, binary: &Path) -> anyhow::Result<()> {
        let target = downloads.target();
        let url = format!(
            "https://github.com/rust-lang/rust-analyzer/releases/download/{}/rust-analyzer-{}.gz",
            self.pin.tag,
            target.triple()
        );
        let archive = downloads.checked_download(&url, |archive| {
            check_digest(archive, self.pin.sha256_for(target)?)
        })?;
        let mut unpacked = Vec::new();
        GzDecoder::new(archive.as_slice())
            .read_to_end(&mut unpacked)
            .with_context(|| format!("could not unpack {url}"))?;
        let folder = binary.parent().context("the binary has no folder")?;
        place(folder, |unpacking| {
            write_program(&unpacking.join("rust-analyzer"), &unpacked)
        })
    }
}

impl ServerRecipe for RustAnalyzer {
    fn folders(&self) -> Vec<String> {
        vec![self.folder()]
    }

    fn is_downloaded(&self, downloads: &Downloads) -> bool {
        self.binary(downloads).is_file()
    }

    fn launch(
        &self,
        downloads: &Downloads,
        _codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        let binary = self.binary(downloads);
        downloads.downloaded(&ServerName::rust_analyzer(), &binary, waiting, || {
            self.download(downloads, &binary)
        })?;
        Ok(ServerLaunch::rust_analyzer(binary).reporting_status())
    }
}

/// Runs the servers that are npm packages.
#[derive(Clone)]
pub(super) struct Node {
    pub(super) pin: ReleasePin,
}

impl Node {
    fn folder(&self) -> String {
        format!("node-{}", self.pin.tag)
    }

    fn binary(&self, downloads: &Downloads) -> PathBuf {
        downloads.folder(&self.folder()).join("node")
    }

    fn program(
        &self,
        downloads: &Downloads,
        waiting: &dyn Fn(Waiting),
    ) -> Result<PathBuf, FallbackReason> {
        let binary = self.binary(downloads);
        downloads.downloaded(&ServerName::new("Node"), &binary, waiting, || {
            self.download(downloads, &binary)
        })?;
        Ok(binary)
    }

    /// Of Node's release archive only the `node` program is kept.
    fn download(&self, downloads: &Downloads, binary: &Path) -> anyhow::Result<()> {
        let target = downloads.target();
        let platform = match target.os {
            Os::MacOs => "darwin",
            Os::Linux => "linux",
        };
        let arch = match target.cpu {
            Cpu::Arm64 => "arm64",
            Cpu::X86_64 => "x64",
        };
        let url = format!(
            "https://nodejs.org/dist/{tag}/node-{tag}-{platform}-{arch}.tar.gz",
            tag = self.pin.tag
        );
        let archive = downloads.checked_download(&url, |archive| {
            check_digest(archive, self.pin.sha256_for(target)?)
        })?;
        let mut node = None;
        let mut entries = tar::Archive::new(GzDecoder::new(archive.as_slice()));
        for entry in entries
            .entries()
            .with_context(|| format!("could not unpack {url}"))?
        {
            let mut entry = entry.with_context(|| format!("could not unpack {url}"))?;
            let path = entry
                .path()
                .with_context(|| format!("could not unpack {url}"))?;
            if path.ends_with("bin/node") && path.components().count() == 3 {
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .with_context(|| format!("could not unpack {url}"))?;
                node = Some(bytes);
                break;
            }
        }
        let node = node.with_context(|| format!("{url} has no bin/node"))?;
        let folder = binary.parent().context("the binary has no folder")?;
        place(folder, |unpacking| {
            write_program(&unpacking.join("node"), &node)
        })
    }
}

/// An npm package that runs on Node: the registry tarball holds it under `package/`.
pub(super) struct NpmPackage {
    pub(super) name: &'static str,
    pub(super) pin: PackagePin,
    /// The file the server runs, inside the package's download folder.
    pub(super) script: &'static str,
}

impl NpmPackage {
    /// The registry tarball holds the package under `package/`, and `script` is in it. Intelephense,
    /// typescript-language-server, typescript and pyright are each bundled with no dependencies, so no
    /// npm installs anything.
    fn download(&self, downloads: &Downloads, script: &Path) -> anyhow::Result<()> {
        let (package, pin) = (self.name, &self.pin);
        let url = format!(
            "https://registry.npmjs.org/{package}/-/{package}-{}.tgz",
            pin.version
        );
        let archive =
            downloads.checked_download(&url, |archive| check_sha512(archive, &pin.sha512))?;
        let folder = downloads.folder(&self.folder());
        place(&folder, |unpacking| {
            tar::Archive::new(GzDecoder::new(archive.as_slice()))
                .unpack(unpacking)
                .with_context(|| format!("could not unpack {url}"))?;
            let inside = script.strip_prefix(&folder)?;
            anyhow::ensure!(
                unpacking.join(inside).is_file(),
                "{url} has no {}",
                inside.display()
            );
            Ok(())
        })
    }

    fn folder(&self) -> String {
        format!("{}-{}", self.name, self.pin.version)
    }

    fn script(&self, downloads: &Downloads) -> PathBuf {
        downloads.folder(&self.folder()).join(self.script)
    }

    /// `server` is what the card says is downloading.
    fn fetched(
        &self,
        server: &ServerName,
        downloads: &Downloads,
        waiting: &dyn Fn(Waiting),
    ) -> Result<PathBuf, FallbackReason> {
        let script = self.script(downloads);
        downloads.downloaded(server, &script, waiting, || {
            self.download(downloads, &script)
        })?;
        Ok(script)
    }
}

pub(super) struct Intelephense {
    pub(super) node: Node,
    pub(super) package: NpmPackage,
    /// One folder per codebase, kept across idle stops and app restarts.
    pub(super) indexes: PathBuf,
}

impl ServerRecipe for Intelephense {
    fn folders(&self) -> Vec<String> {
        vec![self.node.folder(), self.package.folder()]
    }

    fn is_downloaded(&self, downloads: &Downloads) -> bool {
        self.node.binary(downloads).is_file() && self.package.script(downloads).is_file()
    }

    /// Intelephense keeps its index of the repo in a folder of its own, so a restart reads it back.
    fn launch(
        &self,
        downloads: &Downloads,
        codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        let node = self.node.program(downloads, waiting)?;
        let script = self
            .package
            .fetched(&ServerName::intelephense(), downloads, waiting)?;
        let index = self
            .indexes
            .join(codebase.state_folder())
            .join("intelephense");
        Ok(ServerLaunch::intelephense(node, &script, &index))
    }
}

pub(super) struct TypeScriptLanguageServer {
    pub(super) node: Node,
    pub(super) server: NpmPackage,
    /// The tsserver the server runs, whatever TypeScript the reviewed repo installs.
    pub(super) typescript: NpmPackage,
}

impl ServerRecipe for TypeScriptLanguageServer {
    fn folders(&self) -> Vec<String> {
        vec![
            self.node.folder(),
            self.server.folder(),
            self.typescript.folder(),
        ]
    }

    fn is_downloaded(&self, downloads: &Downloads) -> bool {
        self.node.binary(downloads).is_file()
            && self.server.script(downloads).is_file()
            && self.typescript.script(downloads).is_file()
    }

    /// typescript-language-server would take the TypeScript of the repo's `node_modules` unless it is
    /// told the pinned one, and that would run code the reviewed PR ships.
    fn launch(
        &self,
        downloads: &Downloads,
        _codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        let node = self.node.program(downloads, waiting)?;
        let name = ServerName::typescript_language_server();
        let script = self.server.fetched(&name, downloads, waiting)?;
        let tsserver = self.typescript.fetched(&name, downloads, waiting)?;
        Ok(ServerLaunch::typescript_language_server(
            node, &script, &tsserver,
        ))
    }
}

pub(super) struct Pyright {
    pub(super) node: Node,
    pub(super) package: NpmPackage,
}

impl Pyright {
    /// A path in the app's own download folder where nothing is.
    fn no_interpreter(&self, downloads: &Downloads) -> PathBuf {
        downloads
            .folder(&self.package.folder())
            .join("no-python-interpreter")
    }
}

impl ServerRecipe for Pyright {
    fn folders(&self) -> Vec<String> {
        vec![self.node.folder(), self.package.folder()]
    }

    fn is_downloaded(&self, downloads: &Downloads) -> bool {
        self.node.binary(downloads).is_file() && self.package.script(downloads).is_file()
    }

    fn launch(
        &self,
        downloads: &Downloads,
        _codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        let node = self.node.program(downloads, waiting)?;
        let script = self
            .package
            .fetched(&ServerName::pyright(), downloads, waiting)?;
        Ok(ServerLaunch::pyright(
            node,
            &script,
            &self.no_interpreter(downloads),
        ))
    }
}

/// The Go toolchain from go.dev: gopls is built with it and runs its `go` command.
#[derive(Clone)]
pub(super) struct GoToolchain {
    pub(super) pin: ReleasePin,
}

impl GoToolchain {
    fn folder(&self) -> String {
        format!("go-{}", self.pin.tag)
    }

    /// GOROOT: the release archive holds the toolchain under `go/`.
    fn root(&self, downloads: &Downloads) -> PathBuf {
        downloads.folder(&self.folder()).join("go")
    }

    fn binary(&self, downloads: &Downloads) -> PathBuf {
        self.root(downloads).join("bin").join("go")
    }

    /// Downloads the toolchain unless it is there, and gives its root.
    fn root_fetched(
        &self,
        downloads: &Downloads,
        waiting: &dyn Fn(Waiting),
    ) -> Result<PathBuf, FallbackReason> {
        let binary = self.binary(downloads);
        downloads.downloaded(&ServerName::new("Go"), &binary, waiting, || {
            self.download(downloads)
        })?;
        Ok(self.root(downloads))
    }

    fn download(&self, downloads: &Downloads) -> anyhow::Result<()> {
        let target = downloads.target();
        let platform = match target.os {
            Os::MacOs => "darwin",
            Os::Linux => "linux",
        };
        let arch = match target.cpu {
            Cpu::Arm64 => "arm64",
            Cpu::X86_64 => "amd64",
        };
        let url = format!(
            "https://go.dev/dl/go{}.{platform}-{arch}.tar.gz",
            self.pin.tag
        );
        let archive = downloads.checked_download(&url, |archive| {
            check_digest(archive, self.pin.sha256_for(target)?)
        })?;
        place(&downloads.folder(&self.folder()), |unpacking| {
            tar::Archive::new(GzDecoder::new(archive.as_slice()))
                .unpack(unpacking)
                .with_context(|| format!("could not unpack {url}"))?;
            anyhow::ensure!(
                unpacking.join("go/bin/go").is_file(),
                "{url} has no go/bin/go"
            );
            Ok(())
        })
    }
}

/// gopls has no release binaries: it is built from its module with the pinned Go, once.
pub(super) struct Gopls {
    pub(super) go: GoToolchain,
    pub(super) pin: ModulePin,
    /// One folder per codebase for the Go caches, kept across idle stops and app restarts.
    pub(super) indexes: PathBuf,
}

const GOPLS_MODULE: &str = "golang.org/x/tools/gopls";

impl Gopls {
    fn folder(&self) -> String {
        format!("gopls-{}", self.pin.version)
    }

    fn binary(&self, downloads: &Downloads) -> PathBuf {
        downloads.folder(&self.folder()).join("gopls")
    }

    /// The module is fetched from proxy.golang.org and checked against sum.golang.org by the go
    /// command, and against the pinned hash here before it is built; its `go.sum` pins what it needs.
    /// The module and build caches are removed once gopls is built.
    fn build(&self, go_root: &Path, binary: &Path) -> anyhow::Result<()> {
        let folder = binary.parent().context("gopls has no folder")?;
        place(folder, |building| {
            let caches = building.join("build");
            let environment = GoEnvironment::of(go_root, &caches)
                .with("GOPROXY", "https://proxy.golang.org")
                .with("GOSUMDB", "sum.golang.org")
                .with("GOBIN", building)
                .into_server_environment();
            let module = format!("{GOPLS_MODULE}@{}", self.pin.version);
            let fetched = run_go(
                go_root,
                building,
                &environment,
                &["mod", "download", "-json", &module],
            )?;
            let hash = serde_json::from_slice::<serde_json::Value>(&fetched)
                .with_context(|| format!("go mod download said no JSON for {module}"))?
                .get("Sum")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .with_context(|| format!("go mod download gave no hash of {module}"))?;
            anyhow::ensure!(
                hash == self.pin.h1,
                "{module} does not match its pinned hash: {hash} is not {}",
                self.pin.h1
            );
            run_go(go_root, building, &environment, &["install", &module])?;
            if caches.exists() {
                std::fs::remove_dir_all(&caches)
                    .with_context(|| format!("could not remove {}", caches.display()))?;
            }
            anyhow::ensure!(building.join("gopls").is_file(), "go install made no gopls");
            Ok(())
        })
    }

    fn caches(&self, codebase: &Codebase) -> PathBuf {
        self.indexes.join(codebase.state_folder()).join("gopls")
    }
}

impl ServerRecipe for Gopls {
    fn folders(&self) -> Vec<String> {
        vec![self.go.folder(), self.folder()]
    }

    fn is_downloaded(&self, downloads: &Downloads) -> bool {
        self.go.binary(downloads).is_file() && self.binary(downloads).is_file()
    }

    fn launch(
        &self,
        downloads: &Downloads,
        codebase: &Codebase,
        waiting: &dyn Fn(Waiting),
    ) -> Result<ServerLaunch, FallbackReason> {
        let go_root = self.go.root_fetched(downloads, waiting)?;
        let binary = self.binary(downloads);
        downloads.downloaded(&ServerName::gopls(), &binary, waiting, || {
            self.build(&go_root, &binary)
        })?;
        Ok(ServerLaunch::gopls(
            binary,
            &go_root,
            &self.caches(codebase),
        ))
    }
}

/// The only variables a Go command the app runs sees: the pinned toolchain, which never switches to
/// one a `go.mod` or `go.work` names, its caches in `caches`, no C compiler, and none of the
/// reviewer's Go settings (`GOFLAGS`, `GOPROXY`, `CC`, `GOPACKAGESDRIVER`, the `go env -w` file).
struct GoEnvironment(Vec<(&'static str, OsString)>);

impl GoEnvironment {
    fn of(go_root: &Path, caches: &Path) -> Self {
        let gopath = caches.join("gopath");
        Self(vec![
            ("PATH", go_root.join("bin").into_os_string()),
            ("GOROOT", go_root.as_os_str().to_owned()),
            ("GOTOOLCHAIN", "local".into()),
            ("GOENV", "off".into()),
            // Module cache files are read-only unless this is set, and the app removes its folders.
            ("GOFLAGS", "-modcacherw".into()),
            ("CGO_ENABLED", "0".into()),
            (
                "GOMODCACHE",
                gopath.join("pkg").join("mod").into_os_string(),
            ),
            ("GOPATH", gopath.into_os_string()),
            ("GOCACHE", caches.join("gocache").into_os_string()),
        ])
    }

    fn with(mut self, name: &'static str, value: impl Into<OsString>) -> Self {
        self.0.push((name, value.into()));
        self
    }

    fn into_server_environment(self) -> ServerEnvironment {
        ServerEnvironment::Only(self.0)
    }
}

fn run_go(
    go_root: &Path,
    folder: &Path,
    environment: &ServerEnvironment,
    args: &[&str],
) -> anyhow::Result<Vec<u8>> {
    let mut command = Command::new(go_root.join("bin").join("go"));
    environment.apply_to(&mut command);
    let ran = command
        .args(args)
        .current_dir(folder)
        .output()
        .with_context(|| format!("could not run go {}", args.join(" ")))?;
    anyhow::ensure!(
        ran.status.success(),
        "go {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&ran.stderr).trim()
    );
    Ok(ran.stdout)
}

impl ServerLaunch {
    /// Build scripts, proc macros and check on save are off, so it builds nothing in the repo.
    pub fn rust_analyzer(program: PathBuf) -> Self {
        Self {
            name: ServerName::rust_analyzer(),
            program,
            args: Vec::new(),
            status: StatusReports::No,
            environment: ServerEnvironment::Inherited,
            options: json!({
                "cargo": {"buildScripts": {"enable": false}},
                "procMacro": {"enable": false},
                "checkOnSave": false,
            }),
        }
    }

    /// Intelephense on the app's Node, keeping its index in `index`.
    pub fn intelephense(node: PathBuf, script: &Path, index: &Path) -> Self {
        Self {
            name: ServerName::intelephense(),
            program: node,
            args: vec![script.to_string_lossy().into_owned(), "--stdio".to_owned()],
            status: StatusReports::No,
            environment: ServerEnvironment::Inherited,
            options: json!({
                "storagePath": index.join("storage"),
                "globalStoragePath": index.join("global"),
            }),
        }
    }

    /// typescript-language-server on the app's Node, running the tsserver at `tsserver`. Typings
    /// acquisition is off, since it runs npm to install `@types` packages the repo names.
    pub fn typescript_language_server(node: PathBuf, script: &Path, tsserver: &Path) -> Self {
        Self {
            name: ServerName::typescript_language_server(),
            program: node,
            args: vec![script.to_string_lossy().into_owned(), "--stdio".to_owned()],
            status: StatusReports::No,
            environment: ServerEnvironment::Inherited,
            options: json!({
                "tsserver": {"path": tsserver},
                "disableAutomaticTypingAcquisition": true,
            }),
        }
    }

    /// Pyright on the app's Node. Pyright runs a Python to learn the import paths and the version:
    /// the client's `pythonPath`, else `python3` from PATH, in the server checkout. `no_interpreter`,
    /// where nothing is, makes it run none; it then reads the stubs it ships and the repo's files.
    /// Workspace files cannot name a Python (pyright 1.1.414 refuses `pythonPath` there) and a venv
    /// they name is only read. Pyright reads `pythonPath` from the `python` section, which these
    /// options answer as every other.
    pub fn pyright(node: PathBuf, script: &Path, no_interpreter: &Path) -> Self {
        Self {
            name: ServerName::pyright(),
            program: node,
            args: vec![script.to_string_lossy().into_owned(), "--stdio".to_owned()],
            status: StatusReports::No,
            environment: ServerEnvironment::Inherited,
            options: json!({"pythonPath": no_interpreter}),
        }
    }

    /// gopls with only the app's Go settings (`GoEnvironment`), its caches in `caches`, and no
    /// module downloads: `go list` and `go mod tidy` would otherwise fetch the modules the PR's
    /// `go.mod` names, sending private module paths to proxy.golang.org. With no `HOME` gopls
    /// keeps no telemetry and asks nothing about it.
    pub fn gopls(program: PathBuf, go_root: &Path, caches: &Path) -> Self {
        Self {
            name: ServerName::gopls(),
            program,
            args: Vec::new(),
            status: StatusReports::No,
            options: json!({"linksInHover": false}),
            environment: GoEnvironment::of(go_root, caches)
                .with("GOPLSCACHE", caches.join("goplscache"))
                .with("GOPROXY", "off")
                .with("GOPACKAGESDRIVER", "off")
                .into_server_environment(),
        }
    }
}

/// Fails unless `bytes` has the sha512 `expected`, in hex.
fn check_sha512(bytes: &[u8], expected: &str) -> anyhow::Result<()> {
    let actual = crate::hex::lower(&Sha512::digest(bytes));
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(expected),
        "the archive does not match its checksum: {actual} is not {expected}"
    );
    Ok(())
}
