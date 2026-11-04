# Panic and error policy

On 2026-11-04 the maintainer decided:

- Production code (`src/`) has no raw `.unwrap()`. Errors propagate with `?` and context, or are shown to the user.
- `.expect()` and `panic!` are banned by default in `src/`. A true invariant (for example a glob or query fixed in the source) may keep its `expect`, but only under `#[expect(clippy::expect_used, reason = "...")]`, so each exception is visible and reviewed.
- Tests may use `.unwrap()` and `.expect()`. A panic in a test is the test failing. Clippy's `allow-unwrap-in-tests` and `allow-expect-in-tests` carry this.
- The rule is enforced by lints in `Cargo.toml` and `clippy.toml`, and CI runs clippy with `-D warnings`. See [the audit log](rust-audit-2026-11.md) for the commit that turned the lints on.

As of `guided-review@415894b`, before the audit: `src/` had 4 `.unwrap()` and 20 `.expect()`; `tests/` had 671 `.unwrap()`.

Related: [audit log](rust-audit-2026-11.md).
