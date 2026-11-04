# Check loop, 2026-11-04

Why: the retro of audit phases 1 to 2b found the gate (fmt, clippy, test) took 160 to 290 s, hit the 180 s tool limit, and every agent wrote its own gate script.

## Before and after

Warm cache, one blank line appended to `src/lib.rs`, then `cargo test --all-features`, same machine.

| | Before | After |
|---|---|---|
| Wall time | 178 s | 41 s |
| Test build and link (`--timings`) | 18.9 s | 5.3 s |
| Test executables run | 39 | 4 (lib, bin, `it`, doc) |
| Tests | 430 | 430 |
| Time inside tests | 33.6 s summed | 24.7 s for the `it` crate |

The build was not the main cost: about 140 s went to starting 38 separate executables one after another, which is the saving the merge buys.

## What changed

- `87ca7b7`: `tests/*.rs` became modules of one crate, `tests/it/main.rs`. Support moved to `tests/it/support/`, fixtures stay in `tests/fixtures/`. The lint allows live once in `main.rs`; the per-binary `dead_code` allows are gone and clippy stays clean.
- 9ac8cb9: `scripts/check` runs fmt, clippy, tests, stops at the first failure, ends on `ALL GREEN`, takes an optional test filter, and falls back to the rustup toolchain path when `cargo` is not on PATH. CI runs it.
- 5751818: `AGENTS.md` (with `CLAUDE.md` pointing to it): run the gate before each commit, edit nothing while it runs, where tests and fakes live.

## Left

- One test takes most of the 24.7 s; the next speedup is there.
- Not tried: `[lib] doctest = false`, `[profile.dev.package."*"] opt-level = 1`.
