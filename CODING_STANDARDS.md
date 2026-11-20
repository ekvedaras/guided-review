# Coding standards

A reviewer can check each rule below by reading the diff. Lints in `Cargo.toml` and `clippy.toml` enforce most of them, and CI runs clippy with `-D warnings`.

- **Panics.** Follow the [panic policy](docs/wiki/panic-policy.md). No `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!` or `dbg!` in `src/`. A true invariant may keep an `expect` only under `#[expect(clippy::expect_used, reason = "...")]`. Tests may unwrap.
- **Errors.** An error propagates with `?` and context (`anyhow::Context`), or it reaches the user. Never drop a `Result` with `let _ =` or `map_err(|_| ..)` without saying why.
- **No lossy `as` casts.** Use `From`, `TryFrom` or a named helper. A cast that truly cannot lose data needs an `#[expect]` with a reason.
- **Text is cut on char boundaries.** No byte-index slicing of `&str`. Use `char_indices`, `split_at_checked`, `get(..)` or grapheme iteration.
- **No unchecked indexing.** Use `get()`, iterators or `split_at_checked` instead of `v[i]` and `v[a..b]` in `src/`.
- **Smallest type that fits for data stored in bulk.** Per-line, per-piece or per-pixel data uses `u8`/`u16`/`u32` where the range allows, `Arc<str>` over `String`, and `Arc<[T]>` over `Arc<Vec<T>>`.
- **No per-frame allocation in render paths.** `render` and the code it calls do not parse, highlight, clone deep structures or build `String`s per frame. Compute once, cache, and borrow.
- **Value objects over bare strings and bools.** A path, sha, id or mode is a type, and a `bool` parameter that changes behaviour is an enum.
- **Opt-outs say why.** Every `#[expect]` and `#[allow]` carries a `reason`. Prefer `#[expect]` so a stale one fails the build.
- **What floats goes through the overlay module.** A panel over the page beside what opens it (a list, card or picker) is built with `overlay::popover`, a covering layer with `modal_layer` or `input_blocker`, so it takes the mouse and nothing under it lights up or scrolls. Clippy refuses a bare `gpui::deferred` elsewhere; see [floating panels](docs/wiki/floating-panels.md).
- **Rows that touch square the corners they share.** A list of adjacent rows (picker, sheet, sidebar) builds each row with `picker_row_at` and `touching_places` (`overlay.rs`), never a bare rounded row: the selected row and a hovered neighbour are square where they meet, and a row with a body open or a heading between has no neighbour there. See [visual design](docs/wiki/visual-design.md).
- **A count is shown only above 0.** "Fix (0)" says nothing; leave the number off (`with_count` in the findings sheet).
- **Comments say why, not what.** Prefer expressive names and types.
