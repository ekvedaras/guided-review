# Guided review

Rust 2024 gpui app.

## The gate

`scripts/check` is the gate before every commit: fmt, clippy, tests, stopping at the first failure, ending on `ALL GREEN`. CI runs the same script. It finds `cargo` itself.

- Start it with `run_in_background` and read only until it ends. A commit stages what is on disk, so edit nothing while it runs, and stage first or check `git diff` against what you commit.
- Inner loop: `scripts/check <test-name-filter>` runs only matching tests. Clippy denies warnings, so while code is half-written, see a red test with `cargo test <filter>`.
- Change files with the Edit and Write tools, not shell edits. In an agent worktree the sandbox has refused heredocs, `sed -i` without a backup suffix, and chained commands (2026-11-06).

## Tests

- All integration tests are one crate, `tests/it/main.rs`, one module per topic. A new file needs a `mod` line there.
- `tests/it/support/` holds the fixture repo, CLI runner and fake ports (`fake_ports.rs`). `tests/fixtures/` holds recorded data.
- `src/viewer/` is the window-free core and is tested through those fakes. `src/app/` is gpui with no window tests, so put logic behind a viewer method.
- New behaviour is test-first, one slice at a time: write the test, stub the new code with `todo!()`, see `cargo test <filter>` fail on that test, then implement. A test that was never red proves nothing.

## herdr

- herdr-core (`hc`) and herdr-app run the maintainer's live terminals. Focus, label, send to, kill or remove only terminals you started yourself for the task, and remove them when done.
- `herdr-app commands` gives each command's arguments; `herdr-app do <cmd> --help` does not. macOS has no `timeout`; use `perl -e 'alarm 10; exec @ARGV' -- <cmd>`.

## Read before changing code

- `CODING_STANDARDS.md`: the rules and lints.
- `GLOSSARY.md`: the domain words.
- `docs/wiki/index.md`: dated decisions, their reasons and what is known open. Check the cited commit before trusting a claim.
