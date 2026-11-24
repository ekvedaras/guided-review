# Definitions on hover

2026-11-24 (issue #139, commits 6fc1ea9 core, e5c7a63 viewer, then the card in the app). Hover a name in a diff line of a Rust, PHP or TypeScript file and a Definition card lists where the name is defined and where it is used. Check the cited code before trusting a number here.

## Why tree-sitter and not language servers

_Superseded on 2026-11-24 by [Language servers first](#language-servers-first-2026-11-24) below; kept as the record of the first choice._

The app already parses these languages with tree-sitter to cut files into pieces. A language server needs the project installed and built (`composer install`, `node_modules`, a compiling Rust workspace), one server per language and a running process per repo, and it answers for the checkout on disk, not for the PR's head or base commit. A tags query over the git objects at the commit needs none of that and works for any sha. The cost: matching is by name only, so `total` in two classes is one name. The card never hides that: it counts the definitions ("2 definitions"), ranks them and shows each file, so the reviewer sees the guess.

## What a lookup does

- Commit: the review's head for added and context lines, the base for removed lines (`NameKey` in `src/viewer/name_desk.rs`).
- Files: with a local clone, `git grep -w -l <word> <sha>` over the three languages' globs (`src/navigation/git_grep.rs`); at most 300 files are parsed, and the card says when more hold the name. Without a clone (code read from GitHub) only the review's changed files are searched, and the card says "Usages from the PR's files only".
- Definitions and usages come from `queries/<language>/tags.scm` (`@definition.<kind>`, `@reference.<kind>`). Definitions rank: the hovered file first, then those in the module the hovered file imports the name itself from (`Import::leads_to` in `src/navigation/import.rs`: a Rust `use` path resolved through `crate`/`super`, `mod.rs` and the file stem; a PHP namespace's tail against the folders; a TypeScript module path resolved against the hovered file), then the rest, ties by path. A file that cannot be read is skipped, and an unreadable hovered file only loses the import ranking.
- A local is looked up in its function only (2026-11-24, issue #183, commit 8bd96a8): see [Locals stay in their function](#locals-stay-in-their-function-2026-11-24).
- One lookup per (sha, word) is kept for the review's life: a commit never changes. It runs on a background thread (`NameWork::run`) and its card reads "Looking up" until it lands. A failed lookup is asked again on the next hover.
- The card lists up to 20 usages, those in the PR's files first, and counts the rest.
- A click on a row jumps to the line's own row (`JumpPlace::Line`) on the first screen that shows it, base or head side, removed lines included; any other line, also one in a changed file outside every shown hunk, opens through the `CodeOpener` (`NameTarget::Code`). A context line is shown only once the files at head are read, so before that it opens as code.
- A word is a name unless the line's highlights call it a keyword, literal, comment or built-in; before the highlights are ready, unless the grammar spells it as one of its own tokens (`Language::is_keyword`).

## Rust is parsed, not cut into pieces

`Language` gained `Rust` for the tags query. Rust files still stay one piece each and the outline is unchanged: `Language::cuts_into_pieces` is false for Rust and `Language::for_pieces` filters on it, so every piece, outline and symbol path goes through it. A test pins the Rust outline (`tests/it/outline_symbols.rs`). Teaching the outline Rust's symbols is a separate change.

## The card in the app

- Which name: `HoveredName::under_pointer` (`src/viewer/name_desk.rs`) takes the run of letters, digits and `_` at the pointer, so `$order` is `order` and `Cart::total` is the part under the pointer. A pointer just past the end of a name still counts for it. Files of no known language give none.
- When: `NameHover` (`src/viewer/name_hover.rs`) opens the card after the pointer rests 500 ms on a name, as a tooltip waits. It stays while the pointer is on the name or the card, and closes 150 ms after it leaves both (long enough to cross to the card). Escape, a press on the code (a text selection starts) or the page scrolling or changing screen close it; it stays closed until the pointer goes to another name. A move with a button held never opens it.
- Where: under the start of the name, through `overlay::popover_at` (`popover` with a position), so it occludes like every floating panel ([Floating panels](floating-panels.md)). Rows use `picker_row_at`, and the lists scroll above 380 px.
- Snapshot scene `96-name-card` stages the card through `Review::stage_looked_up_name` with a made-up lookup.

## Open

- A name defined by a macro, a re-export or a generated file is found only if a tags pattern names it.
- Usages by name include other classes' methods of the same name (locals excepted since 8bd96a8).
- The card does not follow a page that scrolls under it: it closes.

## Locals stay in their function (2026-11-24)

Issue #183, commit 8bd96a8. The by-name lookup searched the repo for a function-local `$deliveryDate` (1558 usages in 274 files, see below). Now `Lookups::look_up_at` (`src/navigation/lookup.rs`) first asks `Local::at` (`src/navigation/local.rs`) whether the name at the hovered line and column is a local; if so it parses only the hovered file and runs no file search (`Reach::EnclosingFunction`).

- Which names: a PHP `$variable` other than `$this`, not a property declaration or `self::$x`; a Rust identifier that does not start with a capital (a capital in a pattern is a variant or constant); a TypeScript identifier or shorthand property. `$this->x`, `self.x`, `obj.x`, methods, functions, classes and module-level names go to the repo search as before.
- Which function: walking up from the name, the first function-like node that binds the name (a parameter, a `let`/assignment/`foreach`/`for`/`catch`/pattern binding). A PHP function, method or closure (`function () use ($x)`) is the scope even when nothing in it binds the name, since it sees nothing around it; a PHP arrow function, a Rust closure and every TypeScript function see their parent's locals, so the walk goes on past them. A Rust `fn` that does not bind the name stops the walk: the name is a constant, function or unit struct, looked up by name.
- A function inside the scope that binds the name itself, or is a PHP closure or Rust `fn`, has its own local and is left out of the usages.
- The definition is the first binding in source order, kind `variable` or `parameter`, its signature the whole line. A local nothing binds (a PHP superglobal) has usages only.
- The hovered name now carries its column (`HoveredName::column`, `NameKey::column`, a byte offset in the line, `navigation::Column`), so `$total` and `->total` on one line are two cards. Lookups by name are still kept per (sha, word); a local is parsed again on each new card, one file.

Open: TypeScript block scope is not read (a `let` in two sibling blocks of one function is one local); shadowing inside one function is not read; a PHP `$variable` at a file's top level, outside any function, is looked up by name across the repo.

## Language servers first (2026-11-24)

On 2026-11-24 the maintainer tried the card (as of `ekvedaras/guided-review@66682ce`) on a Laravel PHP repo and rejected the by-name results: a function-local `$deliveryDate` was grepped across the repo (1558 usages in 274 files, with unrelated `deliveryDate()` methods as definitions), and `$this->schedule()` listed 9 `schedule` definitions from other classes. Scope rules on top of tree-sitter were weighed and turned down as a weak copy of a type checker.

Decided by the maintainer the same day:

- Language servers answer first (rust-analyzer, Intelephense, typescript-language-server); the tree-sitter lookup stays as the fallback and the card says when it answered by name only.
- The app manages the servers: it downloads, pins and updates them, and the Node runtime that two of them need, so the reviewer installs nothing. On the maintainer's Mac on that day only rust-analyzer was installed; there was no node, php or composer.
- One port for definitions and usages with a tree-sitter and an LSP adapter; the LSP adapter is tested against a scripted fake server over stdio, with a smoke test per real server only where it is installed.
- Same branch (`139-symbol-hover`) and one PR for all of it.

The spec and tickets are GitHub issues linked from issue #139.

## Language servers are opt-in per repo (2026-11-24)

Issue #190, commit b50cc4e. `NameDesk` (`src/viewer/name_desk.rs`) holds the by-name navigator and, once `Review::use_language_servers(adapter, repo, answer)` is called, a `ServerOptIn` with the adapter, the repo and the reviewer's answer. A name is asked of the adapter only when the answer is yes and the line is a head line; otherwise the by-name navigator answers, so before a yes and after a no the adapter is never asked (tests/it/hover_cards.rs, through `FakeNavigator`).

- The question is `FoundNames::question`, set on a found card of a head line in a language with a server (any file `Language::for_path` knows) while the repo has no answer. Base-side names are by name always, so they ask nothing. The by-name answer shows under it, so a hover is not blocked on the question.
- `Review::answer_language_servers` returns the repo. No strips the question from the kept cards; yes forgets the cards and `ReviewSession::answer_language_servers` asks the hovered name again, now of the adapter. The app keeps the answer with `Settings::remember_language_servers`, which loads, sets and saves through Settings' rules (only changed keys, comments kept); a failed write is a toast.
- `config.toml` holds `[language_servers]` with `"owner/repo" = true|false` (`Config::language_servers`; a value that is not a bool reads as not asked). Settings' Code page lists the rows and changes each (`Settings::change_language_servers`); there is no way to remove a row, so no repo can be asked again from the app.
- Nothing in the app calls `use_language_servers` yet, since no language server adapter exists: no card asks the question until the adapter ticket wires it with `Config::language_servers(repo)` as the answer. The question names rust-analyzer whatever the repo's languages are.

## Code navigator port (2026-11-24)

Issue #184. `CodeNavigator` (`src/navigation/navigator.rs`) is the one thing the viewer asks: `navigate(code, &NavigationRequest { sha, at, column, word }, waiting)` gives an `Answer { lookup, answered_by }`. `Lookups` implements it as the by-name adapter. `NameDesk` holds an `Arc<dyn CodeNavigator>`, and `NameWork::run_reporting` hands what the navigator waits on (`Waiting::Downloading`, `Starting`, `Indexing`) to `Review::name_waiting`, which turns the card into `DefinitionCard::Waiting`; the app sends these from the background thread over a channel (`ReviewSession::look_up_name`). `AnsweredBy::ByName { because: Some(..) }` adds `CardNote::AnsweredByName` ("Answered by name: ..."); with no reason, or from a language server, there is no note. Tests swap the adapter with `Review::use_navigator` and `FakeNavigator` (`tests/it/support/fake_ports.rs`). Nothing visible changes until the language server adapter lands. Cards are kept per `NameKey`, which carries the column (commit 8bd96a8); `Lookups` still keeps its by-name results per (sha, word).

## Rust names answered by rust-analyzer (2026-11-24)

Issue #185, in the branch's commits after b50cc4e. `LanguageServers` (`src/navigation/language_server/mod.rs`) is the language server adapter; `LanguageServers::navigator(by_name)` gives each review a `CodeNavigator` that falls back to its own by-name lookup. The app builds one pool for every review (`src/app/language_servers.rs`, wired in `ReviewSession::new`, off under the `snapshots` feature so scene 96 shows no question) and stops it on quit.

- When a server answers: the line is a head line, the answer for the repo is yes, the code has a local clone. Otherwise the by-name lookup answers with a note: "needs a local clone, and the code is read from GitHub". (Until #187 the clone also had to be clean at head; that rule is gone.) Since #187 the server runs in the server checkout, not in the clone (see below).
- Which server: `ServerCatalog::launch_for(language)`. For now only rust-analyzer, found on PATH or in `~/.cargo/bin` by running `--version` (a rustup proxy without the component fails that and counts as not installed); PHP and TypeScript fall back with "<server> is not downloaded by the app yet". The managed download of #186 replaced `ServersOnThisMachine` (see "rust-analyzer is downloaded by the app" below).
- Settings sent as `initializationOptions` and as the answer to `workspace/configuration`: `cargo.buildScripts.enable`, `procMacro.enable` and `checkOnSave` all false (`ServerLaunch::rust_analyzer`).
- Protocol (`rpc.rs`): Content-Length framed JSON-RPC, a reader thread per server. The client offers UTF-8 and UTF-16 positions; the server's `capabilities.positionEncoding` decides how the byte column is counted. After `initialized` one round trip (`$/guidedReviewBarrier`, any answer or none within 2 s) lets progress that began on `initialized` arrive before the first busy check. A hover waits while the server has progress tokens open or reports `experimental/serverStatus` quiescent false, and reports `Indexing` once; `Timeouts` says how long (start 30 s, a request 30 s, indexing 180 s, idle 600 s).
- Lifecycle: one server per (clone folder, server name), started by the first hover after the yes, kept across reviews of that repo, stopped by a thread that checks every quarter of the idle time, and by `stop_all` at quit (shutdown, exit, then kill after 2 s). A crashed server is dropped and the next hover starts it again.
- The card: definitions come from `textDocument/definition` (at most 10), usages from `textDocument/references` with `includeDeclaration: false`. The first definition's signature is the last code block before the first `---` of the hover markdown and its doc is the first paragraph after it, cut at a line that starts with `@`, `#` or a code fence, so raw tags never show. Other definitions show their line as the signature. The kind is read from the signature's first words (`Hovered`'s `kind_of`).
- A location outside the repo or in a file git does not track at head is kept in `Lookup::on_disk_files` and its row is `NameTarget::OnDisk`, opened from disk through the IDE template (`CodeOpener::destination_of_file`); its label is the absolute path without the leading slash.
- The question names the hovered file's server (`ServerName::of(language)`).
- Tests: `tests/it/language_server.rs` against `tests/fixtures/fake_lsp.py` (needs `python3`), a scripted server that logs what it is sent. The smoke test against the real rust-analyzer is described under #186 below.

Open: the Rust standard library rows open only when rust-analyzer has the `rust-src` component; whether the real server answers within the timeouts on a large workspace with the three settings off is not measured; the barrier request is a guess at how servers treat an unknown `$/` request.

## rust-analyzer is downloaded by the app (2026-11-24)

Issue #186, commit 658dc24. `ManagedServers` (`src/navigation/language_server/managed.rs`) is the `ServerCatalog`; `ServersOnThisMachine` and the PATH search are gone, so a rust-analyzer the reviewer installed is never used.

- Pins are in `ServerPins::pinned()`: tag `2026-11-23` and a sha256 for each of macOS and Linux on x86_64 and aarch64 (the digests GitHub shows for the release assets `rust-analyzer-<triple>.gz`; the macOS arm64 file was downloaded and ran, `--version` 0.3.3130). A bump is a new tag and four digests; a test checks each target has one.
- The first Rust hover after a yes calls `waiting(Downloading)`, downloads `https://github.com/rust-lang/rust-analyzer/releases/download/<tag>/rust-analyzer-<triple>.gz` through `ReleasesPort::download` (the update code's port), checks it with `update::check_digest` (the update's checksum check, now shared) and only then gunzips it into `<cache>/language-servers/rust-analyzer-<tag>/rust-analyzer`, unpacked beside its place and renamed, so the folder holds a whole binary or does not exist. `launch_for` gained a `waiting` argument for this.
- A wrong digest or a failed download is not unpacked or run; the by-name lookup answers with "rust-analyzer could not be downloaded: ..." and nothing is kept, so the next hover tries again. One lock around the download makes two hovers download once; the second also shows Downloading while it waits.
- `ManagedServers::remove_unpinned` runs when the app builds its pool and deletes everything in the folder that is not the pinned version's folder, a stopped unpacking included.
- Found with the real server: rust-analyzer sends `experimental/serverStatus` (quiescent false) after the reply to the barrier request, so the first busy check saw a quiet server and the hover got an empty answer. A launch that says `reporting_status()` (rust-analyzer's) is busy until its first status arrives. Its fake in `tests/fixtures/fake_lsp.py` sends status after the barrier reply with `busy_ms`.
- Smoke test (`the_real_rust_analyzer_finds_a_definition`): passed on 2026-11-24 against the downloaded server (tiny crate, definition at the right line). It runs when the pinned binary is in the app's cache folder, or downloads it when `GUIDED_REVIEW_DOWNLOAD_SMOKE` is set. A lookup of `check_digest` in a clone of this repo took 9 s including start and indexing and gave the definition, its doc and four usages (a throwaway test, not kept).
- Not done: Intelephense and typescript-language-server still fall back with "is not downloaded by the app yet"; the Rust standard library rows need `rust-src` as before.

## Servers run in a server checkout, not in the clone (2026-11-24)

Issue #187. `ServerCheckouts::moved_to(clone, repo, head)` (`src/navigation/language_server/checkout.rs`) makes, on the first server start of a repo, a git worktree of the reviewer's clone at `<state>/server-checkouts/<owner>/<repo>` (`AppFolders::server_checkouts`, `git worktree add --detach`), and checks it out detached at each reviewed head. See Server checkout in GLOSSARY.md.

- Why not the clone: a server's index is keyed by its folder and its files. Running in the clone when it is clean at head and elsewhere when it is not would change the folder and throw the index away; the clone's own branch and edits would also decide whether a card gets a server. A fixed folder that only moves between commits keeps the index, and the reviewer's working tree, index and branch are never touched (tested on a fixture clone with a dirty tree). A worktree adds an entry under the clone's `.git/worktrees`; that is the only change in the clone.
- Moving: `git diff --name-status --no-renames -z old new`, then `git checkout --detach new`. The running server hears `textDocument/didClose` for each changed file it had open (the next question opens the new text) and one `workspace/didChangeWatchedFiles` (created 1, changed 2, deleted 3). It is not restarted. A server that is not running starts fresh in the moved checkout.
- `vendor/` and `node_modules/` are symlinks to the clone's, made when the clone has the folder and the head does not track that name; a move to a head that tracks it removes the link first, or git refuses the checkout. A link is made again on the next move once the head stops tracking it.
- Found with the real server: right after a move rust-analyzer answers the first question with LSP error -32801 (ContentModified). `Failure::Modified` (-32801 and -32802) makes the adapter pause 250 ms, wait until the server is quiet and ask again, up to 5 times, instead of falling back. Timing on 2026-11-24 in this repo (debug build, first lookup of `look_up_at` at HEAD~3 then at HEAD~5 in the same checkout): 5.7 s with start and indexing, 0.9 s after the move.
- One lock per repo makes reviews of the same repo take turns for move plus question, since they share the folder. Hovers of other repos are not held up.
- Locations under the checkout are repo paths when git tracks them at the reviewed head; others (the linked `vendor/`) keep the repo-relative label and open from disk through the checkout's link. A location the server reports under another folder (for instance the clone's real `vendor/` path) is labelled by its absolute path.
- Dependency note: a card answered by a language server says "The PR changes dependency files, so answers may be off" when the review changes a file named `composer.json`, `composer.lock`, `package.json` (any folder), `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock`, `bun.lockb`, `Cargo.toml` or `Cargo.lock` (`is_dependency_file`). By-name answers do not say it, since they read no installed packages. The note is about packages the PR replaces; the checkout borrows the clone's installed ones.
- Open: a checkout whose clone was deleted or replaced is not recovered; a failed move falls back by name with "could not get the reviewed head checked out". Real-server timings of a second head are under #187's report, not here.
