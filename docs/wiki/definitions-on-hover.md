# Definitions on hover

2026-11-24 (issue #139). Hover a name in a diff line of a Rust, PHP, TypeScript, Python or Go file and a Definition card lists where the name is defined and where it is used, with a code pane beside the lists. A language server answers when the reviewer said yes for the codebase; otherwise a tree-sitter lookup by name answers. Check the cited code before trusting a number here.

## Language servers first, by name as the fallback (2026-11-24)

The first build looked names up with tree-sitter tags queries only. It needs no installed project, no server process and works for any sha over the git objects, but it matches by name only. On 2026-11-24 the maintainer tried it on a Laravel PHP repo and rejected the results: a function-local `$cartTotal` was grepped across the repo (1558 usages in 274 files, with unrelated `cartTotal()` methods as definitions), and `$this->schedule()` listed 9 `schedule` definitions from other classes. Scope rules on top of tree-sitter were weighed and turned down as a weak copy of a type checker.

Decided the same day:

- Language servers answer first: rust-analyzer, Intelephense, typescript-language-server, and since 2026-11-26 Pyright and gopls. The by-name lookup stays as the fallback, and the card says when it answered by name and why.
- The app downloads, pins and updates the servers and the runtimes they need (Node, Go), so the reviewer installs nothing, and a server the reviewer installed is never used.
- One port, the code navigator, with a by-name adapter and a language server adapter. The language server adapter is tested against a scripted fake server; each real server has a smoke test that skips when the server is not downloaded.

## The by-name lookup

- Commit: the review's head for added and context lines, the base for removed lines (`NameKey` in `src/viewer/name_desk.rs`).
- Files: with a local clone, `git grep -w -l <word> <sha>` over the languages' globs (`src/navigation/git_grep.rs`); at most 300 files are parsed (`FILES_PARSED_AT_MOST`, `src/navigation/lookup.rs`), and the card says when more hold the name. Without a clone only the review's changed files are searched, and the card says "Usages from the PR's files only".
- Definitions and usages come from `queries/<language>/tags.scm` (`@definition.<kind>`, `@reference.<kind>`). The Python and Go queries are our own, in the shape of the Rust one: every identifier is a reference. A Go `type_spec` matches both `struct`/`interface` and `type`; `DefinitionKind::narrowness` makes struct and interface win.
- Ranking: the hovered file first, then the module the hovered file imports the name from (`Import::leads_to` in `src/navigation/import.rs`: a Rust `use` path through `crate`/`super`, `mod.rs` and the file stem; a PHP namespace's tail against the folders; a TypeScript module path from the hovered file; a Python `from shop.cart import Cart` or relative `.cart`, not a plain `import shop.cart`), then the rest, ties by path. Go code names packages, not definitions, so a definition in the hovered file's folder ranks as imported (`Import::not_needed_between`).
- One lookup per (sha, word) is kept for the review's life, since a commit never changes. It runs on a background thread (`NameWork`); the card reads "Looking up" until it lands. A failed lookup is asked again on the next hover.
- A word is a name unless the line's highlights call it a keyword, literal, comment or built-in; before the highlights are ready, unless the grammar spells it as one of its own tokens (`Language::is_keyword`).

## Locals stay in their function (2026-11-24)

Issue #183, commit 8bd96a8. `Lookups::look_up_at` first asks `Local::at` (`src/navigation/local.rs`) whether the name at the hovered line and column is a local. If so it parses only the hovered file and runs no file search (`Reach::EnclosingFunction`).

- Locals: a PHP `$variable` other than `$this`, not a property declaration or `self::$x`; a Rust identifier that does not start with a capital (a capital in a pattern is a variant or constant); a TypeScript identifier or shorthand property; a Python or Go identifier that a function binds. Member access (`$this->x`, `self.x`, `obj.x`), Python keyword argument names and import paths, Go fields, methods, functions, classes and module-level names go to the repo search.
- Scope: walking up from the name, the first function-like node that binds the name. A PHP function, method or closure, a Rust `fn` and a Go `func` or method see nothing around them, so they stop the walk (`Scope::Wall`); a PHP arrow function, a Rust closure, every TypeScript function, a Python function or lambda and a Go `func` literal see their parent's locals (`Scope::Open`). A Rust `fn` that does not bind the name means the name is a constant, function or unit struct, looked up by name; a Python name no function binds is a module name.
- A nested function that binds the name itself, or is a wall, has its own local and is left out of the usages.
- The definition is the first binding in source order, kind `variable` or `parameter`. A local nothing binds (a PHP superglobal) has usages only.
- The hovered name carries its column (`navigation::Column`, a byte offset in the line), so `$total` and `->total` on one line are two cards. Cards are kept per `NameKey`, which carries the column.

## Rust, Python and Go are parsed, not cut into pieces

Rust, Python and Go files stay one piece each and the outline is unchanged: `Language::cuts_into_pieces` is false for them and `Language::for_pieces` filters on it. `tests/it/outline_symbols.rs` pins the Rust outline. Teaching the outline their symbols is a separate change.

## The card in the app

- Which name: `HoveredName::under_pointer` (`src/viewer/name_desk.rs`) takes the run of letters, digits and `_` at the pointer, so `$order` is `order` and `Cart::total` is the part under the pointer. A pointer just past the end of a name still counts for it. Files of no known language give none.
- When: `NameHover` (`src/viewer/name_hover.rs`, timings in `src/app/layouts/one_step/name_card_hover.rs`) opens the card after the pointer rests 500 ms on a name, as a tooltip waits. It stays while the pointer is on the name or the card, and closes 150 ms after it leaves both, long enough to cross to the card. Escape, a press on the code, a scroll of the page or a change of screen close it; it stays closed until the pointer goes to another name. A move with a button held never opens it.
- Where: under the name, or over it when the window has no room below, through `overlay::popover_by`, so it occludes like every floating panel ([Floating panels](floating-panels.md)). The lists are 640 px wide and scroll above 400 px (`name_card.rs`). The card lists up to 20 usages (`USAGES_SHOWN`), those in the PR's files first, and counts the rest.
- A click on a row jumps to the line's own row (`JumpPlace::Line`) on the first screen that shows it, base or head side, removed lines included; any other line opens through the `CodeOpener` (`NameTarget::Code`). A location outside the repo, or in a file git does not track at head, opens from disk through the IDE template (`NameTarget::OnDisk`); its label is the absolute path. A context line is shown only once the files at head are read, so before that it opens as code.
- Snapshot scenes: `96-name-card`, `97-name-card-swiped-sideways` and `98-name-card-usage-shift-wheel` (`src/app/layouts/one_step/scenes.rs`), staged with a made-up lookup.

## Code pane (2026-11-27)

Maintainer's decision. The card has a code pane at its right: 520 px are added when any row has code (`FoundNames::shows_code`); with none, the card keeps its width.

- A definition row's pane is the declaration from its first leading line to its end, cut after 40 lines with a "…" line (`BODY_LINES_AT_MOST` in `src/viewer/name_code.rs`). `declaration_lines` (`src/navigation/declaration.rs`) finds the largest tree-sitter node whose first token is the line's first token, never the file's root, and adds the doc comment, comments and attributes right above it, up to a blank line. A declaration under PHP attributes or TypeScript or Python decorators is the node that starts above the line when all before the line's first token is attributes, decorators or comments. Every definition shows its leading lines, since the doc says what the body does. `tests/it/declaration_lines.rs` holds one test per shape.
- A usage row's pane is the 3 lines above and below it (`USAGE_CONTEXT_LINES`) with the usage line marked.
- The pane follows the last row the pointer entered (`FoundNames::code_pane`) and keeps it when the pointer leaves, so the reviewer can cross to the pane and scroll it. Before any row is hovered, and for a row with no code, it shows the first definition's. The card has no keyboard row cursor.
- The panes are built on the lookup's background thread from the file reads `NameHighlights` already does for the colours, so drawing parses nothing. A file that cannot be read or has no highlight language has no pane. Leading tabs show as 4 spaces (`PaneLine::of`).
- The pane scrolls both ways above 400 px. Its scroll area is `flex().items_start()` (`code_pane` in `src/app/components/name_card.rs`), since gpui's `div` scrolls only as far as its children's bounds reach past its own: a block child is as wide as the area, so a block area could not scroll sideways at all. Scenes 97 and 98 send a trackpad swipe and a shift wheel through `Window::dispatch_event` and fail when the pane's `ScrollHandle` offset did not move.

## Opt-in per codebase (2026-11-24)

Issues #190 and #191, commits b50cc4e, 0a1c841 and 94b7a44. See Language server and Codebase in [GLOSSARY.md](../../GLOSSARY.md).

- The question (`FoundNames::question`) is set on a found card of a head line while the codebase has no answer. It names the hovered file's server (`ServerName::of`). Base-side names are by name always, so they ask nothing. The by-name answer shows under the question, so a hover is not blocked on it.
- `config.toml` holds `[language_servers]` with the codebase's text form `= true|false` (`Config::language_servers`; a value that is not a bool reads as not asked). Settings' Code page lists the rows and changes each; a row cannot be removed, so the app never asks again.
- A `Codebase` (`src/codebase.rs`) is a hosted repo or, for a local folder with no origin on a known host, the folder's absolute path. A PR review and a local review of one repo share the answer and the server checkout. It replaced a fake owner `local folder`, which showed as "local folder/…" in the question and in Settings.
- The answer is shared by every open review: `ServerOptIns` (`src/navigation/navigator.rs`) in the app's one pool, seeded from `config.toml` at open. An answer on a card or a Settings save goes to `LanguageServers::answered`, and a No stops the codebase's running servers. After a change to yes the cards are forgotten and asked of the adapter; after a no they lose the question.
- A local review asks its snapshot commit, which the server checkout checks out like a head; Update moves the checkout and the server is not restarted.

## Code navigator port (2026-11-24)

Issue #184. `CodeNavigator` (`src/navigation/navigator.rs`) is the one thing the viewer asks: `navigate(code, &NavigationRequest, waiting)` gives an `Answer { lookup, answered_by }`. `Lookups` is the by-name adapter; `LanguageServers::navigator(by_name)` gives each review the language server adapter, which falls back to its own by-name lookup. What the adapter waits on (`Waiting::Downloading`, `Starting`, `Indexing`) turns the card into `DefinitionCard::Waiting`. `AnsweredBy::ByName { because: Some(..) }` adds the note "Answered by name: ...". Tests swap the adapter with `Review::use_navigator` and `FakeNavigator` (`tests/it/support/fake_ports.rs`).

## Language servers

Module `src/navigation/language_server/`. Each managed server is one `ServerRecipe` (`recipes.rs`) that owns its pin, download folder, download and `ServerLaunch`; adding a server is a new recipe and one arm in `ManagedServers::recipe` (`managed.rs`). The app builds one pool for every review (`src/app/language_servers.rs`, off under the `snapshots` feature so the scenes show no question) and stops it on quit.

- When a server answers: the line is a head line, the answer for the codebase is yes, and the review has a local clone. Otherwise the by-name lookup answers with a note, for instance "<server> needs a local clone, and the code is read from the PR's host"; nothing is downloaded.
- Downloads: on the first hover after a yes, into `<cache>/language-servers/<name>-<version>/`, each announced on the card ("Downloading Node…"). Pins (`recipes.rs`): rust-analyzer release `2026-11-23` (sha256 per OS and CPU); Node `v26.13.1` (sha256 from nodejs.org's `SHASUMS256.txt`, only `bin/node` kept); Intelephense 1.18.5, typescript-language-server 6.1.0, typescript 6.0.3 and Pyright 1.1.414 as npm tarballs checked by sha512; Go 1.27.4 (sha256 from go.dev) and gopls v0.24.0 by its module hash. A bump is a new pin and its digests. A file is checked before it is unpacked, and unpacked beside its place and renamed, so the folder holds a whole download or nothing. A wrong digest or a failed download is not run; the card falls back with "<server> could not be downloaded: ..." and the next hover tries again. One download runs at a time (`DownloadLock`). `ManagedServers::remove_unpinned` deletes versions no longer pinned when the pool is built.
- No npm: each npm package runs from its tarball, since each is one bundle that requires only Node built-ins (checked per pin on 2026-11-24 and 2026-11-26). A Rust-only reviewer never downloads Node.
- typescript 6, not 7: typescript 7 is the native Go port with no `tsserver.js`, which typescript-language-server cannot run.
- gopls has no release binaries and runs the `go` command while it works, so the app downloads a pinned Go and builds gopls with it (decided 2026-11-26). Building gopls in this repo's CI was turned down: the Go download stays anyway, and it adds a release job for four targets and an artifact of our own to trust. The build checks the module's `Sum` against the pin; gopls's `go.sum` and sum.golang.org check its dependencies.
- Protocol (`rpc.rs`): Content-Length framed JSON-RPC over stdio, a reader thread per server. The server's `positionEncoding` (UTF-8 or UTF-16) decides how the byte column is counted. After `initialized` one round trip of `$/guidedReviewBarrier` (`BARRIER_REQUEST`; LSP 3.17 has a server answer an unknown `$/` request with MethodNotFound) lets progress begun on `initialized` arrive first. A hover waits while the server has progress open or reports `experimental/serverStatus` quiescent false. rust-analyzer sends its first status after the barrier reply, so a launch that reports status (`reporting_status()`) is busy until its first status arrives.
- Card from a server: definitions from `textDocument/definition` (at most 10), usages from `textDocument/references` without the declaration. Signature and doc come from the hover markdown (`hover.rs`); raw tags (`@param`, `_@`, `*@returns*`) and PHP's `<?php` line are cut. Intelephense's definition range starts at the doc comment, so `Places::at_name` moves it to the first line with the name. An imported TypeScript name hovers as the import, so the card shows the declaring line instead.
- Timeouts (`Timeouts` in `mod.rs`): start 30 s, an answer 30 s, indexing 180 s, idle 600 s. A server silent past the answer time is stopped and a later hover starts a new one; one past the indexing time falls back but is kept. The idle time counts from the last answered question, and a question in flight counts as activity. A crashed server is dropped and started again by a later hover. At quit: shutdown, exit, then kill after 2 s.
- Servers are keyed by (codebase, server name), so a No can find them.
- LSP error -32801 or -32802 (content modified, which rust-analyzer gives right after a checkout move) makes the adapter pause 250 ms, wait until the server is quiet and ask again, up to 5 times (`server_navigator.rs`).
- `window/showMessage` and the message of `window/showMessageRequest` (answered with no action) show as a toast "<server>: <message>", an error as a Danger toast; the same text from the same server once per run. `window/logMessage` is not shown. Decided 2026-11-24 (602efa8): the maintainer read Intelephense's licence (section 3: use with an LSP-compatible editor; section 5d: its notices may not be hidden) and kept Intelephense with its notices shown. Definition, references and hover are not premium features. Whether a review app counts as an LSP-compatible client was not checked with the author.
- The card says "The PR changes dependency files, so answers may be off" when a server answered and the review changes a composer, npm, yarn, pnpm, bun or Cargo manifest or lock (`is_dependency_file`, `src/navigation/dependency_file.rs`). By-name answers do not say it.
- Tests: `tests/it/language_server.rs` and `language_server_downloads.rs` against `tests/fixtures/fake_lsp.py`, which needs `python3` and says so when it is missing. Real-server tests run when the pinned server is in the cache folder, or download it when `GUIDED_REVIEW_DOWNLOAD_SMOKE` is set.

## Server checkout (2026-11-24)

Issue #187, commits 2753b10e, 3e2987d and b288601. See Server checkout in [GLOSSARY.md](../../GLOSSARY.md). Every server runs in `<state>/server-checkouts/<Codebase::state_folder()>` (`ServerCheckouts::moved_to`, `checkout.rs`), never in the reviewer's clone.

- Why not the clone: a server's index is keyed by its folder and its files. A fixed folder that only moves between commits keeps the index, and the reviewer's working tree, index and branch are never touched.
- The checkout is the app's own git repo, not a worktree of the clone, so the clone gets no worktree entry and no ref. It reads the clone's objects through `objects/info/alternates` and fetches a head it cannot read. When the alternates name another clone or a folder that is gone, or the checkout can no longer read its head (a `git gc` pruned it), it is made again with its own objects. An earlier build's worktree checkout is removed with its entry in the clone.
- No git hooks: every git command on it runs with `-c core.hooksPath=/dev/null`, and it is made with `git init --template=`. As a worktree it shared the clone's config, and a relative `core.hooksPath` resolved inside the PR's files, so a checkout ran a hook the PR wrote.
- Moving: `git diff --name-status --no-renames` from the head each running server last heard (`Running::heard`), then `git checkout --detach`. The server hears `textDocument/didClose` for changed files it had open and one `workspace/didChangeWatchedFiles`; it is not restarted. A server whose head can no longer be compared is restarted.
- Locking: a read-write lock per codebase over where the checkout was last moved. Questions at that head share it; a move takes it alone and waits for questions at the old head, so no server is asked about files that changed under it.
- `vendor/` and `node_modules/` link to the reviewed clone's folders when the head does not track that name. Rows under them keep their repo-relative label and open from disk.
- Server processes start with relative and empty `PATH` entries removed (`absolute_entries` in `rpc.rs`), since they resolve inside the checkout, the PR's files.

## Files in the PR that could start a program (2026-11-24, 2026-11-26)

Tried against each pinned real server with a hostile file that writes a marker, and a positive control that showed the marker can be written. What the app does, per server:

- rust-analyzer: a PR's `.cargo/config.toml` `build.rustc-wrapper` or `build.rustc`, a `rust-project.json` `sysroot`, and a `rust-toolchain.toml` `path` made cargo, rust-analyzer or rustup run a program from the repo. The checkout leaves out `PROGRAM_SETTINGS` (`checkout.rs`: `.cargo/config.toml`, `.cargo/config`, `rust-toolchain.toml`, `rust-toolchain`, `rust-project.json`, `rust-analyzer.toml`) at any depth with a sparse checkout, and servers hear no change to them (commit 0326d47). `rust-analyzer.toml` ran nothing, but its allowed keys change between releases. The client sends `cargo.buildScripts.enable`, `procMacro.enable` and `checkOnSave` false. The cost: rust-analyzer analyses with the default toolchain and without the repo's cargo config. The `rust-toolchain.toml` case cannot be tested from `cargo test`, which sets `RUSTUP_TOOLCHAIN`; it was run by hand.
- typescript-language-server: the client passes the pinned `tsserver.js` as `tsserver.path`, so the PR's `node_modules/typescript` never runs. Pass it only after the download put the file there: a path that is not a valid tsserver falls through to the workspace one. Typings acquisition is off, since it runs npm. A tsconfig plugin is not loaded, since typescript-language-server does not pass `--allowLocalPluginLoads`; a test guards it.
- Intelephense: composer scripts, autoloaded PHP, editor settings and `bin/` programs ran nothing. Nothing is kept out.
- Pyright: with no client setting it runs `python3` from PATH to read `sys.path`, and a config's venv Python when the venv has no `site-packages`. The client sets `pythonPath` to `<pyright folder>/no-python-interpreter`, where nothing is, so Pyright runs no Python and answers from its bundled stubs and the repo's files.
- gopls: with the inherited environment a PR's `go.mod` or `go.work` `toolchain`/`go` line made the go command download and run that Go, a file with `import "C"` fed the PR's C to the reviewer's compiler, and `go list` downloaded the modules the PR requires (a private module's path would go to proxy.golang.org). gopls starts with only the app's variables (`ServerEnvironment::Only`, `env_clear`): `PATH` = the pinned Go's `bin`, `GOTOOLCHAIN=local`, `GOENV=off`, `CGO_ENABLED=0`, `GOPROXY=off`, `GOPACKAGESDRIVER=off`, `GOFLAGS=-modcacherw`, and Go caches under `<state>/server-indexes/<Codebase::state_folder()>/gopls`; no `HOME`, so gopls keeps no telemetry. `GOPROXY=off` (2026-11-26): no module path leaves the machine and no git runs; the cost is that third-party imports are unresolved unless the head vendors them or a `replace` names a folder.

## Open

- A name defined by a macro, a re-export or a generated file is found by name only if a tags pattern names it. Usages by name include other classes' methods of the same name.
- Scope reading: TypeScript and Python block scope, shadowing inside one function, and Python `global`/`nonlocal` are not read. A PHP `$variable` at a file's top level is looked up by name across the repo.
- The card does not follow a page that scrolls under it: it closes.
- A hover that picked the adapter just before a No can start a server after the No stopped them.
- A folder with no origin whose path changes becomes a new codebase and asks again.
- Global git config still applies to the server checkout's commands (an LFS `filter.lfs.smudge` would run on checkout). A checkout without alternates copies the head's whole history once.
- `cargo metadata` still resolves the PR's `Cargo.toml` dependencies, which can fetch registries and git repos.
- The Rust standard library rows open only when rust-analyzer has the `rust-src` component.
- Pyright sees no installed packages, and without an interpreter assumes its newest Python version. Linking the clone's `.venv` was not tried.
- gopls: a `go.mod` that needs a newer Go than the pin fails to load and falls back by name, so the pin needs bumping as Go releases. A `replace` or `go.work` `use` naming an absolute folder shows that folder's files to the reviewer. The build has no time limit of its own: a stalled proxy keeps the card on "Downloading gopls…" and holds the download lock.
- The dependency note does not count `pyproject.toml`, `requirements*.txt`, `poetry.lock`, `uv.lock`, `go.mod`, `go.sum` or `go.work`.
- No server's speed is measured on a large repo.
- Code pane: a definition line that begins no declaration, or a declaration whose first token is on an earlier line than the name, gets no pane. The line numbers scroll away with the code sideways. The NSEvent-to-`ScrollEvent` step is not run by any scene; a real trackpad and a shift wheel were not tried by an agent.
- SQL is coloured but has no Definition card.
