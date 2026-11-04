# Rust quality audit, 2026-11

On 2026-11-04 the maintainer asked for a repo-wide review: idiomatic Rust, no raw `.unwrap()`, every error handled, and memory-lean types. Spec and tickets: [issue 83](https://github.com/ekvedaras/guided-review/issues/83). The audit ran in four phases (guardrails, user-facing bugs, render cost, domain types), all on `main` in small commits (see [delivery](delivery.md)). This page keeps the outcomes and their reasons; the findings lists and step logs were removed when the audit ended, and the commits hold them.

## Lasting outcomes

### Lints (2026-11-04, `0aafd08` to `3318339`)

- On and denied: `unwrap_used`, `expect_used`, `panic`, `unreachable`, `todo`, `unimplemented`, `dbg` (`48ad2c9`); lossy casts (`eeee318`); `string_slice` (`740fbeb`); `indexing_slicing`, which tests may use (`9b8a8be`). Also on: `unsafe_code`, `unused_qualifications`, `pedantic` and a reviewed set of picks for clones, needless copies, `allow_attributes_without_reason` and `use_self` (`3318339`). The exact list is in `Cargo.toml`.
- Skipped, with reasons (2026-11-04, `3318339`). Do not turn these on without reading the reason:
  - `cast_precision_loss`, `must_use_candidate`, `return_self_not_must_use`, `missing_errors_doc`, `missing_panics_doc`, `unreadable_literal` (hex colours), `too_many_lines`, `similar_names`, `struct_excessive_bools`, `fn_params_excessive_bools`: noise for a gpui app. `as_conversions`, `print_stdout`/`print_stderr`, `unused_results` and `missing_const_for_fn` were also skipped as noise, and `needless_pass_by_ref_mut` because it is a false positive on gpui's `cx: &mut Context`.
  - `struct_field_names`: the two hits are fine names.
  - `unused_self`: the hits are view helpers and `CommentDesk` rules that are methods on their owner on purpose.
  - `derive_partial_eq_without_eq`: fires inside gpui's `actions!` macro, which we cannot change.
  - `literal_string_with_formatting_args`: fires on the IDE command template's `{path}` and `{line}` placeholders.

### Reviewed `expect`s (2026-11-04, `48ad2c9`, `9b8a8be`, `3318339`)

Each carries a reason in the source; remove one only if its reason is gone.

- `expect_used` for compiled-in globs (`file_rules.rs`) and grammars and symbol queries (`symbols.rs`): fixed in the source, so a failure is a build bug.
- `indexing_slicing` in `SymbolChange::entry_for`: the index was just found or pushed.
- `cast_*` in `whole_pixels` and `channel_byte`: the value is clamped first.
- `unsafe_code` in `dock_icon::show`: an objc2 call that must run on the main thread.
- `needless_collect` in `DiagramMemory::strips_to_draw`: the collect ends a borrow of `self`.
- `too_many_arguments` on two constructors.

### One problems list for user-facing errors (2026-11-04, `0709c2a`, `4834628`, `3ff1748`)

A failure the reviewer must know about goes into `Review::problems()` (`Problem`), which the sidebar shows in one place, and stays until its cause clears (a save goes through, the re-pin ends, a thread read succeeds). Stderr is not a way to tell the reviewer. One exception on purpose: a diagram strip that fails to draw says why in its own place (`6c33500`), because the failure belongs to one block of the page. After a re-pin, comment outcomes still apply on purpose (`cb38ce9`): the draft is the same saved entity, and dropping a "sent" outcome would invite a second post.

### Render-cost rules (2026-11-04, `8b334bd` to `1d2ba19`, `ebf6068`)

- Highlight queries compile once per language and markdown parses once per text; a thread and the PR description are shared by `Rc`, never deep-copied per frame. Measured: a PHP code fence went from 14.6 ms to 17 µs per highlight (`8b334bd`). The measurements were throwaway release-mode timing tests, not `GUIDED_REVIEW_FRAME_STATS` on a real PR.
- Lookups replace per-line splits (`hang_under`, cmd+k, `percent_encode_path`), and a cached blob is an `Arc<[u8]>` keyed so a lookup allocates nothing (`957f975`).
- Bulk data is stored lean: `Span` offsets are `u32`, a file's spans are one list, `FilesAtHead` is one text per file, and big row payloads sit behind an `Rc`. A page row is exactly 56 bytes and a diff line exactly 160, guarded by size tests that fail on a pad-up as well as a shrink (`1d2ba19`, `ebf6068`).
- Left on purpose: getting a page row to 48 bytes would cost another `Rc` for 8 bytes a row; caching all cmd+k targets is not worth it because they change with a re-pin and ranking a few hundred labels is cheap.

### Domain types (2026-11)

Introduced so a wrong mix-up does not compile and rewording a label cannot change behaviour: `StepId` (`7c69aa1`), `FilePath` (`e8ccd47`, `9c35c97`, `1473436`), `PieceId` (`0d0f227`, `2945876`), `LineNumber` as `NonZeroU32` (`03fd0ac`), `CommentPlace` (`b7489d6`, `70f5683`), `JumpPlace` (`d8aff63`), `CommentProblem` (`a2de777`), `ImageMark` (`e7c402b`), `GitHubError` (`74a433e`, `eae8fac`), `ThreadId` and `CommentId` (`afbdb40`), thread-action refusals as plain values (`c1a7807`), and enums in place of `bool` parameters (`5df0033`, `7d85e03`, `c30df13`, `961d4a6`). `TextDraft` keeps an anchor and a head instead of a reversed flag (`26054cf`).

### Decisions with reasons

- **`#` in a piece id (2026-11-04, `afaead9`, `f3faf52`).** A state file holds a piece id as text `path#symbol`. A whole file whose name holds a `#`, such as `docs/C#.md`, would read as the symbol `.md` of `docs/C`, so such a whole file is saved with a trailing `#` (`docs/C#.md#`). Text saved before the mark reads as a whole file when no symbol fits, and an id no build wrote is dropped instead of failing the state file. The `#` was kept as the separator, not changed, so existing state files keep loading and ids still order by their text.
- **`FilePath` accepts `\` (2026-11-04, `1473436`).** A backslash is a legal file name character on Linux and macOS, so refusing it would drop a real file from the review. A diff path that is not a valid repo path is now an error, where before the file was silently dropped.
- **Check loop (2026-11-04, `87ca7b7`, `9ac8cb9`, `5751818`).** The gate took 160 to 290 s, hit the 180 s tool limit, and every agent wrote its own script. The cause was not the build but running 38 separate test executables one after another, so the tests became one crate (`tests/it`) and `scripts/check` is the single gate: 178 s to 41 s on a warm cache, same 430 tests. Usage is in [AGENTS.md](../../AGENTS.md). Not profiled: the 24.7 s inside the `it` crate. Not tried: `[lib] doctest = false`, `[profile.dev.package."*"] opt-level = 1`.

## Known open (2026-11-04, verified at `f3faf52`)

- Superseded 2026-11-04 (`508f16c`, `810487b`, `f0a0b38`, `48be52f`, `e222f47`, `1263919`): each failed read below is now a `Problem` in the sidebar, cleared by the next good read (the IDE one by the next IDE that opens, the GitHub fallback by the next review opened on the clone). Left on stderr on purpose: the sync error in `apply_sync` (now also a sidebar problem, `ce04955`), a queued comment that cannot be sent (a send that fails stays queued; the only error left is a failed save, already a sidebar problem), window and snapshot errors, frame stats, and a broken highlight query (a developer error). Was: failed reads that still go only to stderr: the PR head check (the new-commits banner never shows), the PR description, the pending-review lookup and the authorship lookup (the finish dialog may offer the wrong choices), an IDE command that cannot start (`ide.rs`, the code link does nothing), and the fallback to GitHub when the local clone cannot fetch (`pick_code_source.rs`, code opens on GitHub without saying why). Each needs its own `Problem` variant and clearing rule.
- Measured 2026-11-04 (release timing on a large real PR, about 3,400 rows in one step; #102): a rows rebuild takes 2.5 ms, a drag selection end about 7 ms (selected lines 1.6, place of lines 1.2, finish 1.8, rows 2.5), a cmd+k keystroke 0.45 ms. Under the 3 ms and 8 ms gates, so shared diff lines (#104) and one page per change (#105) were dropped as not planned; the frame stats now log `selection ended in X ms` to check a live drag. Comment bodies are prepared once per text (#106), so drawing borrows prepared text.
- Render costs left because the row model needs reshaping: `FileBlock` holds rows twice, `block_rows` clones each line per row, and selection and image queries (`selected_lines`, `place_of_lines`, `image_preview_requests`) rebuild the whole page.
- Superseded 2026-11-04 (`5867c84`): a broken state file is moved aside as `<pr>.broken-<timestamp>.json` and the app opens fresh with a sidebar problem naming the backup; it refuses to open only when the move itself fails. Was: the app would not open until the reviewer fixed the file by hand.
