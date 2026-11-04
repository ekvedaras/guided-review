# Panic and error policy

On 2026-11-04 the maintainer decided (the rules are listed in [CODING_STANDARDS.md](../../CODING_STANDARDS.md)):

- A panic in production code is a bug the reviewer meets as a crash, so errors propagate with `?` and context or are shown to the user. A true invariant (a glob or query fixed in the source) may keep an `expect`, but only under `#[expect(clippy::expect_used, reason = "...")]`, so each exception is visible and reviewed.
- Tests may unwrap and expect: a panic in a test is the test failing. Clippy's `allow-unwrap-in-tests` and `allow-expect-in-tests` carry this; test files with helpers that are not `#[test]` functions carry a file-level `allow` with a reason.
- Lints in `Cargo.toml` and `clippy.toml` enforce it, and CI runs clippy with `-D warnings`. What is on and off, and why, is in the [audit record](rust-audit-2026-11.md).
