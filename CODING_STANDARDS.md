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
- **Comments say why, not what.** Prefer expressive names and types.
