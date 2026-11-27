# Rust quality audit, 2026-11

On 2026-11-04 the maintainer asked for a repo-wide review: idiomatic Rust, no raw `.unwrap()`, every error handled, and memory-lean types. Spec and tickets: issue 83 in the private archive. The audit ran in four phases (guardrails, user-facing bugs, render cost, domain types), all on `main` in small commits (see [delivery](delivery.md)). This page keeps the outcomes and their reasons; the commits hold the findings lists and step logs.

## Lints (2026-11-04, `83b8736` to `5d007aa`)

- On and denied: `unwrap_used`, `expect_used`, `panic`, `unreachable`, `todo`, `unimplemented`, `dbg_macro`; lossy casts; `string_slice`; `indexing_slicing`, which tests may use. Also on: `unsafe_code`, `unused_qualifications`, `pedantic` and a reviewed set of picks for clones, needless copies, `allow_attributes_without_reason` and `use_self`. The exact list is in `Cargo.toml`, and the policy is in [panic policy](panic-policy.md).
- Pedantic lints left off, with reasons in `Cargo.toml`. Do not turn these on without reading the reason:
  - `cast_precision_loss`, `must_use_candidate`, `return_self_not_must_use`, `missing_errors_doc`, `missing_panics_doc`, `unreadable_literal` (hex colours), `too_many_lines`, `similar_names`, `struct_excessive_bools`, `fn_params_excessive_bools`: noise for a gpui app.
  - `struct_field_names`: the two hits are fine names.
  - `unused_self`: the hits left are view helpers and test fixtures that are methods on their owner on purpose. The comment desk's stateless rules are associated functions.

## Reviewed exceptions

Each carries a reason in the source (`#[expect(..., reason = "...")]`); remove one only if its reason is gone. They are: `expect_used` for compiled-in globs (`file_rules.rs`), grammars and symbol queries (`symbols.rs`, `language.rs`), which are fixed in the source, so a failure is a build bug; `indexing_slicing` in `SymbolChange::entry_for`, where the index was just found or pushed; `cast_*` in `image_facts::whole_pixels`, where the value is clamped first; `unsafe_code` in the macOS `dock_icon` and `word_keys`, objc2 calls that run on the main thread; `needless_collect` where the collect ends a borrow of `self`.

## One problems list for user-facing errors (2026-11-04)

A failure the reviewer must know about goes into `Review::problems()` (`Problem`, `src/viewer/problems.rs`), which the sidebar shows in one place, and stays until its cause clears (a save goes through, the re-pin ends, a read succeeds). Stderr is not a way to tell the reviewer. Each failed read has its own `Problem` variant and clearing rule, including the IDE command that cannot start, the fallback to GitHub when the clone cannot fetch, the PR head check, the description, the pending-review and authorship lookups and the viewed sync (`bc70e4f`, `f1f4fb2`, `c97efc0`, `b01b9fe`, `bb7ca4e`, `f43be7c`, `0c720b4`).

- Left on stderr on purpose: window and snapshot errors, frame stats, and a broken highlight query (a developer error).
- A diagram strip that fails to draw says why in its own place (`c5dfd74`), because the failure belongs to one block of the page.
- After a re-pin, comment outcomes still apply (`f48b4fa`): the draft is the same saved entity, and dropping a "sent" outcome would invite a second post.
- A state file that cannot be read is moved aside as `<pr>.broken-<timestamp>.json`, and the app opens fresh with a sidebar problem naming the backup (`877752c`). It refuses to open only when the move itself fails. Before, the app would not open until the reviewer fixed the file by hand.

## Render-cost rules (2026-11-04, `40c80fa` to `3ad9eb7`, `f874177`)

- Highlight queries compile once per language and markdown parses once per text. A thread and the PR description are shared by `Rc`, never deep-copied per frame. A PHP code fence went from 14.6 ms to 17 µs per highlight (`40c80fa`), measured with throwaway release-mode timing tests.
- Lookups replace per-line splits (`hang_under`, cmd+k, `percent_encode_path`), and a cached blob is an `Arc<[u8]>` keyed so a lookup allocates nothing (`1b6f1e7`).
- Bulk data is stored lean: `Span` offsets are `u32`, a file's spans are one list, `FilesAtHead` is one text per file, and big row payloads sit behind an `Rc`. Size tests fail on a pad-up as well as a shrink: a page row is 48 bytes (`tests/it/page.rs`) and a diff line 184 (`tests/it/line_number.rs`).
- Left on purpose: getting a page row below 48 bytes would cost another `Rc` for 8 bytes a row. Caching all cmd+k targets is not worth it, because they change with a re-pin and ranking a few hundred labels is cheap.
- Measured on 2026-11-04 (release timing on a large real PR, about 3,400 rows in one step; #102): a rows rebuild takes 2.5 ms, a drag selection end about 7 ms, a cmd+k keystroke 0.45 ms. These are under the 3 ms and 8 ms gates, so shared diff lines (#104) and one page per change (#105) were dropped as not planned. The frame stats log `selection ended in X ms` (`GUIDED_REVIEW_FRAME_STATS`) to check a live drag. Comment bodies are prepared once per text (#106), so drawing borrows prepared text.
- The viewer keeps one page per change, which later removed the rebuilds the selection and image queries did; see [deepening pass](deepening-pass.md).

## Domain types (2026-11)

Introduced so a wrong mix-up does not compile and rewording a label cannot change behaviour: `StepId`, `FilePath`, `PieceId`, `LineNumber` as `NonZeroU32`, `CommentPlace`, `JumpPlace`, `CommentProblem`, `ImageMark`, `HostError` (was `GitHubError`), `ThreadId` and `CommentId`, thread-action refusals as plain values, and enums in place of `bool` parameters. `TextDraft` keeps an anchor and a head instead of a reversed flag.

## Decisions with reasons

- **`#` in a piece id (2026-11-04, `f9fad2f`, `0676f69`).** A state file holds a piece id as text `path#symbol`. A whole file whose name holds a `#`, such as `docs/C#.md`, would read as the symbol `.md` of `docs/C`, so such a whole file is saved with a trailing `#` (`docs/C#.md#`). Text saved before the mark reads as a whole file when no symbol fits, and an id no build wrote is dropped instead of failing the state file. The `#` stays the separator, so existing state files keep loading and ids still order by their text.
- **`FilePath` accepts `\` (2026-11-04, `4cc79ef`).** A backslash is a legal file name character on Linux and macOS, so refusing it would drop a real file from the review. A diff path that is not a valid repo path is an error, where before the file was silently dropped.
- **Check loop (2026-11-04, `e4f7b84`, `20be6d9`, `94d8f6c`).** The gate took 160 to 290 s, hit the 180 s tool limit, and every agent wrote its own script. The cause was running 38 separate test executables one after another, so the tests became one crate (`tests/it`) and `scripts/check` is the single gate: 178 s to 41 s on a warm cache with the same 430 tests. Usage is in [AGENTS.md](../../AGENTS.md). Not profiled: the 24.7 s inside the `it` crate. Not tried: `[lib] doctest = false`, `[profile.dev.package."*"] opt-level = 1`.
