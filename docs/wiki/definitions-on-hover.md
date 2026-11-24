# Definitions on hover

2026-11-24 (issue #139, commits 6fc1ea9 core, e5c7a63 viewer, then the card in the app). Hover a name in a diff line of a Rust, PHP or TypeScript file and a Definition card lists where the name is defined and where it is used. Check the cited code before trusting a number here.

## Why tree-sitter and not language servers

The app already parses these languages with tree-sitter to cut files into pieces. A language server needs the project installed and built (`composer install`, `node_modules`, a compiling Rust workspace), one server per language and a running process per repo, and it answers for the checkout on disk, not for the PR's head or base commit. A tags query over the git objects at the commit needs none of that and works for any sha. The cost: matching is by name only, so `total` in two classes is one name. The card never hides that: it counts the definitions ("2 definitions"), ranks them and shows each file, so the reviewer sees the guess.

## What a lookup does

- Commit: the review's head for added and context lines, the base for removed lines (`NameKey` in `src/viewer/name_desk.rs`).
- Files: with a local clone, `git grep -w -l <word> <sha>` over the three languages' globs (`src/navigation/git_grep.rs`); at most 300 files are parsed, and the card says when more hold the name. Without a clone (code read from GitHub) only the review's changed files are searched, and the card says "Usages from the PR's files only".
- Definitions and usages come from `queries/<language>/tags.scm` (`@definition.<kind>`, `@reference.<kind>`). Definitions rank: the hovered file first, then files it imports (`Language::imported_names`), then the rest, ties by path.
- One lookup per (sha, word) is kept for the review's life: a commit never changes. It runs on a background thread (`NameWork::run`) and its card reads "Looking up" until it lands. A failed lookup is asked again on the next hover.
- The card lists up to 20 usages, those in the PR's files first, and counts the rest.
- A click on a row jumps there in the tour when the line is in a changed file the tour shows (`NameTarget::Tour`), else opens the code through the `CodeOpener` (`NameTarget::Code`).

## Rust is parsed, not cut into pieces

`Language` gained `Rust` for the tags query. Rust files still stay one piece each and the outline is unchanged: `Language::cuts_into_pieces` is false for Rust and `Language::for_pieces` filters on it, so every piece, outline and symbol path goes through it. A test pins the Rust outline (`tests/it/outline_symbols.rs`). Teaching the outline Rust's symbols is a separate change.

## The card in the app

- Which name: `HoveredName::under_pointer` (`src/viewer/name_desk.rs`) takes the run of letters, digits and `_` at the pointer, so `$order` is `order` and `Cart::total` is the part under the pointer. A pointer just past the end of a name still counts for it. Files of no known language give none.
- When: `NameHover` (`src/viewer/name_hover.rs`) opens the card after the pointer rests 500 ms on a name, as a tooltip waits. It stays while the pointer is on the name or the card, and closes 150 ms after it leaves both (long enough to cross to the card). Escape, a press on the code (a text selection starts) or the page scrolling or changing screen close it; it stays closed until the pointer goes to another name. A move with a button held never opens it.
- Where: under the start of the name, through `overlay::popover_at` (`popover` with a position), so it occludes like every floating panel ([Floating panels](floating-panels.md)). Rows use `picker_row_at`, and the lists scroll above 380 px.
- Snapshot scene `96-name-card` stages the card through `Review::stage_looked_up_name` with a made-up lookup.

## Open

- A name defined by a macro, a re-export or a generated file is found only if a tags pattern names it.
- Usages by name include other classes' methods of the same name.
- The card does not follow a page that scrolls under it: it closes.
