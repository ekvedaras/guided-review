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

## Code navigator port (2026-11-24)

Issue #184. `CodeNavigator` (`src/navigation/navigator.rs`) is the one thing the viewer asks: `navigate(code, &NavigationRequest { sha, at, column, word }, waiting)` gives an `Answer { lookup, answered_by }`. `Lookups` implements it as the by-name adapter. `NameDesk` holds an `Arc<dyn CodeNavigator>`, and `NameWork::run_reporting` hands what the navigator waits on (`Waiting::Downloading`, `Starting`, `Indexing`) to `Review::name_waiting`, which turns the card into `DefinitionCard::Waiting`; the app sends these from the background thread over a channel (`ReviewSession::look_up_name`). `AnsweredBy::ByName { because: Some(..) }` adds `CardNote::AnsweredByName` ("Answered by name: ..."); with no reason, or from a language server, there is no note. Tests swap the adapter with `Review::use_navigator` and `FakeNavigator` (`tests/it/support/fake_ports.rs`). Nothing visible changes until the language server adapter lands. Cards are kept per `NameKey`, which carries the column (commit 8bd96a8); `Lookups` still keeps its by-name results per (sha, word).
