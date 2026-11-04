# Rust quality audit, 2026-11

On 2026-11-04 the maintainer asked for a repo-wide review: idiomatic Rust, no raw `.unwrap()`, every error handled, and memory-lean types (the smallest and fastest type that fits).

## Plan (2026-11-04)

1. Turn on lints first, so the rules hold after the audit ends. See [panic policy](panic-policy.md).
2. Audit each area read-only: `src/*.rs`, `src/viewer/`, `src/app/` core and layouts, `src/app/components/`.
3. Fix in small commits on main. See [delivery](delivery.md).

## Findings

As of `guided-review@415894b`, the `src/*.rs` audit reported:

- `blob_at` in `cached_code_source.rs` clones the whole cached blob on every read.
- `piece.id.to_string()` allocates per comparison in `repin.rs` and `coverage.rs`.
- `anyhow` errors are downcast where a typed error enum would let callers branch.
- Panic risks: byte slicing `new_id[file.len()+1..]` in `repin.rs` (renames), two `expect`s in `parse_unified_diff` in `code_source.rs`, and `file_at(...).unwrap_or_default()` in `pieces.rs` hiding real failures.

As of `guided-review@415894b`, the `src/app/*.rs` and `src/app/layouts/` audit reported:

- A bug: a background task still running after a re-pin can apply the old review's highlights, context lines or sync results to the new review (`review_session.rs`, around lines 521-541).
- Failures to save ticks, drafts and replies go only to stderr. The user never sees them.
- No panic there needs a fix. The existing `expect`s and indexing guard true invariants.
- Modelling: file paths are bare `String` (they want a path value object), jump targets are matched by their display label, comment problems are stored as strings, and three functions take `bool` params.
- Per-frame cost: markdown is parsed again on every frame (`one_step.rs` and every `InlineMarkdown::new`), the PR description and thread views are deep-cloned per frame, and `hang_under` in `page_rows.rs` does O(lines × comments) work with two allocations per line on each rebuild.

## Lint probe (2026-11-04, at `guided-review@415894b`)

A clippy run with pedantic, nursery and chosen restriction lints found 2,449 hits in 67 lints: 1,271 in `src/`, 1,178 in `tests/`.

- Panic lints in `src/`: `unwrap_used` 4, `expect_used` 20, `panic` 1, `unreachable` 2, `indexing_slicing` 54, `string_slice` 36 (mostly `text_input.rs`, which slices user text), and 25 casts that can truncate or lose the sign. The probe recommended deny for all of these.
- The clippy.toml test allowances (`allow-unwrap-in-tests` and the related ones) cut test hits from 1,178 to 527. The 217 unwraps left are in test helpers that are not `#[test]` functions (for example `tests/support`), so those files need their own allow.
- Skipped as noise for a gpui app: `as_conversions`, `unused_results`, `print_stdout`/`print_stderr`, `must_use_candidate` (186), `missing_const_for_fn` (90), `needless_pass_by_ref_mut` (35, false positives on gpui `cx: &mut Context`), `unreadable_literal` (77, hex colours).
- `string_to_string` no longer exists. `implicit_clone` covers it.
- `cargo clippy --fix` can fix about 400 hits by itself, mostly `str_to_string` and `clone_on_ref_ptr`.

## Findings in `src/app/components/` (2026-11-04, at `guided-review@415894b`)

- Bug: in `ask_panel.rs` (around lines 348-356), a failed retry leaves an Ask stuck on "Waiting for the answer..." and shows no error.
- Bug: in `thread_view.rs` (around lines 241-272), cmd-enter can send a reply without its last keystrokes.
- Panic: a reversed range from the input method panics in `text_input.rs` (lines 94 and 137).
- Modelling: the image mark is chosen by matching on the string `"added"`, two separate parsers read the piece id string, `CommentComposer` takes 4 bools, and the Ask target holds a dummy `AgentLaunch`.
- Per-frame cost: the tree-sitter config and the markdown are parsed again for every comment on every frame (`markdown_view.rs:264`, `thread_view.rs:415`), and diagram strips do O(strips²) allocations per frame (`diagram/view.rs:155`).

## Findings in `src/viewer/` (2026-11-04, at `guided-review@415894b`)

- Panic: valid but odd markdown, such as a `</details>` inside a list item, can crash the app (`markdown.rs:288`).
- Bug: "Suggest change" can post an empty suggestion, which wipes the lines (`review.rs:904`).
- Bug: a result line with no session id makes the next Ask resume with an empty id (`agent_event.rs:67`).
- Modelling: piece ids become plain strings almost everywhere, a tour step's id is a `String` and not `StepId`, and errors are told apart by `anyhow` downcasts.
- Cost: each Ask code-fence highlight compiles the tree-sitter queries again on every render (`highlight.rs:396`), each diff row makes several small allocations, and cmd+k builds the whole safety-net page on every keystroke.

## Fix order (decided 2026-11-04)

Phases run one after another on `main`, because they touch the same files.

1. **Guardrails.** Add `CODING_STANDARDS.md` and the lint block, one lint family per commit, and fix its hits in the same commit: panics (`unwrap_used`, `expect_used`, `panic`, `unreachable`), lossy casts, `string_slice`, `indexing_slicing`, then the `warn` picks and what `cargo clippy --fix` repairs.
2. **User-facing bugs** from the findings above, each with a test that fails first.
3. **Render cost**: highlight configs cached per language, markdown parsed once, no deep clones per frame, the diagram strip and `hang_under` loops.
4. **Domain types**: path and piece id value objects, `StepId`, typed errors in place of downcasts, enums in place of `bool` params. This changes design, so it gets a spec before code.

## Phase 1 result (2026-11-04)

Commits on `main`, each green on `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --all-features`:

- `0aafd08` `CODING_STANDARDS.md` and a README pointer to it.
- `48ad2c9` unwrap, expect, panic, unreachable, todo, unimplemented, dbg denied; `clippy.toml` test allowances; file-level `allow` with a reason in test files that have non-`#[test]` helpers.
- `eeee318` cast_possible_truncation, cast_sign_loss, cast_possible_wrap, cast_lossless denied; `image_facts::whole_pixels`, integer percentiles, `cast_signed`.
- `740fbeb` string_slice denied; `get`, `split_once`, `split_at_checked`, `TextDraft::text_in`.
- `9b8a8be` indexing_slicing denied (tests may index); `get`, iterators, `json::at` for JSON reads, typed GraphQL mutation inputs, `SelectedLines` holds its first and last location.
- `3318339` pedantic at warn plus the picks below, with `cargo clippy --fix` and hand fixes; avoid-breaking-exported-api off.

Lints turned on: the four families above, plus `unsafe_code`, `unused_qualifications`, `pedantic` (group), `clone_on_ref_ptr`, `rc_buffer`, `redundant_clone`, `needless_pass_by_value`, `large_types_passed_by_value`, `trivially_copy_pass_by_ref`, `needless_collect`, `format_collect`, `format_push_string`, `assigning_clones`, `implicit_clone`, `or_fun_call`, `let_underscore_must_use`, `map_err_ignore`, `str_to_string`, `allow_attributes_without_reason`, `use_self`, `useless_let_if_seq`.

Lints skipped:

- `cast_precision_loss`, `must_use_candidate`, `return_self_not_must_use`, `missing_errors_doc`, `missing_panics_doc`, `unreadable_literal`, `too_many_lines`, `similar_names`, `struct_excessive_bools`, `fn_params_excessive_bools`: noise for this app, as the lint probe said.
- `struct_field_names`: 2 hits, both fine names (`on_*` callbacks, `Symbols::symbols`).
- `unused_self`: 12 hits in src, view helpers and `CommentDesk` rules that are methods on their owner on purpose. Revisit when the domain types phase decides what the desk owns.
- `derive_partial_eq_without_eq`: fires inside gpui's `actions!` macro, which we cannot change.
- `literal_string_with_formatting_args`: fires on the IDE template's `{path}` and `{line}` placeholders.

`#[expect]` kept, each with a reason: `expect_used` for the compiled-in globs (`file_rules.rs`), the compiled-in grammars and symbol queries (`symbols.rs`); `indexing_slicing` in `SymbolChange::entry_for` (the index was just found or pushed); `cast_*` in `whole_pixels` and `channel_byte` (clamped first); `unsafe_code` in `dock_icon::show` (objc2 call on the main thread); `needless_collect` in `DiagramMemory::strips_to_draw` (the collect ends a borrow of `self`); `too_many_arguments` on two constructors. In tests: `needless_pass_by_value` is allowed per file for helpers, and `unnecessary_wraps` on one helper.

Behaviour that changed because a lint forced it, beyond the audit's phase 2 list:

- A judge answer with `}` before `{` no longer panics; it fails with the usual "did not answer with JSON".
- A missing built-in highlight query leaves the file plain and prints the language on stderr, instead of panicking.
- A failed window open prints the error and quits, instead of panicking.
- A diagram font that fails to load shows the error where the diagram would be, instead of panicking at the first draw.
- `PieceDiff::of` and `symbol_piece` drop a piece with no file or no symbol on either side, where they used to panic.

Could not fix: nothing is left open. `needless_pass_by_value` and `large_types_passed_by_value` changed many signatures (`&Colors`, `&SvgJob`, `&StripJob`), which phase 3 should know when it touches the render path.
