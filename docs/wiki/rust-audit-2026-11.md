# Rust quality audit, 2026-11

On 2026-11-04 the maintainer asked for a repo-wide review: idiomatic Rust, no raw `.unwrap()`, every error handled, and memory-lean types. Spec and tickets: issue 83 in the private archive. The audit ran in four phases (guardrails, user-facing bugs, render cost, domain types), all on `main` in small commits (see [delivery](delivery.md)). This page keeps the outcomes and their reasons; the findings lists and step logs were removed when the audit ended, and the commits hold them.

## Lasting outcomes

### Lints (2026-11-04, `83b8736` to `5d007aa`)

- On and denied: `unwrap_used`, `expect_used`, `panic`, `unreachable`, `todo`, `unimplemented`, `dbg` (`1c2544b`); lossy casts (`edafbda`); `string_slice` (`6fd5574`); `indexing_slicing`, which tests may use (`17f4f1d`). Also on: `unsafe_code`, `unused_qualifications`, `pedantic` and a reviewed set of picks for clones, needless copies, `allow_attributes_without_reason` and `use_self` (`5d007aa`). The exact list is in `Cargo.toml`.
- Skipped, with reasons (2026-11-04, `5d007aa`). Do not turn these on without reading the reason:
  - `cast_precision_loss`, `must_use_candidate`, `return_self_not_must_use`, `missing_errors_doc`, `missing_panics_doc`, `unreadable_literal` (hex colours), `too_many_lines`, `similar_names`, `struct_excessive_bools`, `fn_params_excessive_bools`: noise for a gpui app. `as_conversions`, `print_stdout`/`print_stderr`, `unused_results` and `missing_const_for_fn` were also skipped as noise, and `needless_pass_by_ref_mut` because it is a false positive on gpui's `cx: &mut Context`.
  - `struct_field_names`: the two hits are fine names.
  - `unused_self`: the hits are view helpers and `CommentDesk` rules that are methods on their owner on purpose.
  - `derive_partial_eq_without_eq`: fires inside gpui's `actions!` macro, which we cannot change.
  - `literal_string_with_formatting_args`: fires on the IDE command template's `{path}` and `{line}` placeholders.

### Reviewed `expect`s (2026-11-04, `1c2544b`, `17f4f1d`, `5d007aa`)

Each carries a reason in the source; remove one only if its reason is gone.

- `expect_used` for compiled-in globs (`file_rules.rs`) and grammars and symbol queries (`symbols.rs`): fixed in the source, so a failure is a build bug.
- `indexing_slicing` in `SymbolChange::entry_for`: the index was just found or pushed.
- `cast_*` in `whole_pixels` and `channel_byte`: the value is clamped first.
- `unsafe_code` in `dock_icon::show`: an objc2 call that must run on the main thread.
- `needless_collect` in `DiagramMemory::strips_to_draw`: the collect ends a borrow of `self`.
- `too_many_arguments` on two constructors.

### One problems list for user-facing errors (2026-11-04, `a27588d`, `23c49af`, `7f39405`)

A failure the reviewer must know about goes into `Review::problems()` (`Problem`), which the sidebar shows in one place, and stays until its cause clears (a save goes through, the re-pin ends, a thread read succeeds). Stderr is not a way to tell the reviewer. One exception on purpose: a diagram strip that fails to draw says why in its own place (`c5dfd74`), because the failure belongs to one block of the page. After a re-pin, comment outcomes still apply on purpose (`f48b4fa`): the draft is the same saved entity, and dropping a "sent" outcome would invite a second post.

### Render-cost rules (2026-11-04, `40c80fa` to `3ad9eb7`, `f874177`)

- Highlight queries compile once per language and markdown parses once per text; a thread and the PR description are shared by `Rc`, never deep-copied per frame. Measured: a PHP code fence went from 14.6 ms to 17 µs per highlight (`40c80fa`). The measurements were throwaway release-mode timing tests, not `GUIDED_REVIEW_FRAME_STATS` on a real PR.
- Lookups replace per-line splits (`hang_under`, cmd+k, `percent_encode_path`), and a cached blob is an `Arc<[u8]>` keyed so a lookup allocates nothing (`1b6f1e7`).
- Bulk data is stored lean: `Span` offsets are `u32`, a file's spans are one list, `FilesAtHead` is one text per file, and big row payloads sit behind an `Rc`. A page row is exactly 56 bytes and a diff line exactly 160, guarded by size tests that fail on a pad-up as well as a shrink (`3ad9eb7`, `f874177`). Since then a page row is 48 bytes and a diff line 184; the guards are in `tests/it/page.rs` and `tests/it/line_number.rs`.
- Left on purpose: getting a page row to 48 bytes would cost another `Rc` for 8 bytes a row; caching all cmd+k targets is not worth it because they change with a re-pin and ranking a few hundred labels is cheap.

### Domain types (2026-11)

Introduced so a wrong mix-up does not compile and rewording a label cannot change behaviour: `StepId` (`f3ede3b`), `FilePath` (`0f3c548`, `e6185a7`, `4cc79ef`), `PieceId` (`b036dce`, `b38a1e1`), `LineNumber` as `NonZeroU32` (`9efcdb2`), `CommentPlace` (`65f42c1`, `8fdcca5`), `JumpPlace` (`756757d`), `CommentProblem` (`d0b7859`), `ImageMark` (`027d7eb`), `GitHubError` (`ae36a27`, `173b93f`), `ThreadId` and `CommentId` (`059a454`), thread-action refusals as plain values (`d08398d`), and enums in place of `bool` parameters (`ab9171d`, `4249f90`, `c0d5b28`, `2a49590`). `TextDraft` keeps an anchor and a head instead of a reversed flag (`cce762e`).

### Decisions with reasons

- **`#` in a piece id (2026-11-04, `f9fad2f`, `0676f69`).** A state file holds a piece id as text `path#symbol`. A whole file whose name holds a `#`, such as `docs/C#.md`, would read as the symbol `.md` of `docs/C`, so such a whole file is saved with a trailing `#` (`docs/C#.md#`). Text saved before the mark reads as a whole file when no symbol fits, and an id no build wrote is dropped instead of failing the state file. The `#` was kept as the separator, not changed, so existing state files keep loading and ids still order by their text.
- **`FilePath` accepts `\` (2026-11-04, `4cc79ef`).** A backslash is a legal file name character on Linux and macOS, so refusing it would drop a real file from the review. A diff path that is not a valid repo path is now an error, where before the file was silently dropped.
- **Check loop (2026-11-04, `e4f7b84`, `20be6d9`, `94d8f6c`).** The gate took 160 to 290 s, hit the 180 s tool limit, and every agent wrote its own script. The cause was not the build but running 38 separate test executables one after another, so the tests became one crate (`tests/it`) and `scripts/check` is the single gate: 178 s to 41 s on a warm cache, same 430 tests. Usage is in [AGENTS.md](../../AGENTS.md). Not profiled: the 24.7 s inside the `it` crate. Not tried: `[lib] doctest = false`, `[profile.dev.package."*"] opt-level = 1`.

## Known open (2026-11-04, verified at `0676f69`)

- Superseded 2026-11-04 (`bc70e4f`, `f1f4fb2`, `c97efc0`, `b01b9fe`, `bb7ca4e`, `f43be7c`): each failed read below is now a `Problem` in the sidebar, cleared by the next good read (the IDE one by the next IDE that opens, the GitHub fallback by the next review opened on the clone). Left on stderr on purpose: the sync error in `apply_sync` (now also a sidebar problem, `0c720b4`), a queued comment that cannot be sent (a send that fails stays queued; the only error left is a failed save, already a sidebar problem), window and snapshot errors, frame stats, and a broken highlight query (a developer error). Was: failed reads that still go only to stderr: the PR head check (the new-commits banner never shows), the PR description, the pending-review lookup and the authorship lookup (the finish dialog may offer the wrong choices), an IDE command that cannot start (`ide.rs`, the code link does nothing), and the fallback to GitHub when the local clone cannot fetch (`pick_code_source.rs`, code opens on GitHub without saying why). Each needs its own `Problem` variant and clearing rule.
- Measured 2026-11-04 (release timing on a large real PR, about 3,400 rows in one step; #102): a rows rebuild takes 2.5 ms, a drag selection end about 7 ms (selected lines 1.6, place of lines 1.2, finish 1.8, rows 2.5), a cmd+k keystroke 0.45 ms. Under the 3 ms and 8 ms gates, so shared diff lines (#104) and one page per change (#105) were dropped as not planned; the frame stats now log `selection ended in X ms` to check a live drag. Comment bodies are prepared once per text (#106), so drawing borrows prepared text.
- Superseded 2026-11-12 (`351c80d`, `1da820a`): the viewer's `Page` (`src/viewer/page.rs`) lays out the rows and `Review` keeps it until the review changes, so selection, Ask place, suggestion and picture queries read one page; a one-piece `FileBlock` shows its piece's own hunks; a line row is a `ShownLine` cut once per page from the `DiffLine`, not a copy of it. Still copied: a block of several pieces joins copies of their rows. Was: render costs left because the row model needs reshaping: `FileBlock` holds rows twice, `block_rows` clones each line per row, and selection and image queries (`selected_lines`, `place_of_lines`, `image_preview_requests`) rebuild the whole page.
- The kept page's rule (2026-11-12, `351c80d`): `Review` is `ReviewCore` plus the page built from it. Reads go through `Deref`; the only `&mut ReviewCore` is `DerefMut`, which drops the page first, and the two fields are private to `src/viewer/review/kept.rs`, so no change can leave a stale page. Any `&mut` drops it, even one the page does not read (a drag selection, typing); the app's list holds the `Arc<Page>` it took at its last rebuild, after every `ReviewSession::change`, as it held its rows before, so those changes cost no rebuild. Inputs from outside the review go into it (`Review::set_asked_lines` from the Ask panel), so they drop the page too.
- Superseded 2026-11-04 (`877752c`): a broken state file is moved aside as `<pr>.broken-<timestamp>.json` and the app opens fresh with a sidebar problem naming the backup; it refuses to open only when the move itself fails. Was: the app would not open until the reviewer fixed the file by hand.
