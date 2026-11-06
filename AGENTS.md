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

## Install

The maintainer tries changes by hand with the installed binary (`~/.cargo/bin/guided-review`). Once the gate is green, run `cargo install --path . --locked` and check that `guided-review --version` shows HEAD's short sha with no `-dirty`. Commit first, since the sha names the commit.

## Seeing the app

`scripts/snapshot <pr-url>` writes a PNG of the top and end of every screen of that PR's tour to `/tmp/guided-review-snapshots`, downsized for reading (it builds the `snapshots` feature and runs on a throwaway HOME, so the maintainer's app stays untouched). The PR needs a tour in `~/.guided-review/tours`. `screencapture` is blocked in auto mode.

## Releasing

Bump the version in `Cargo.toml` and `Cargo.lock`, then push the tag `v<version>`. The release workflow builds the archives and creates a draft release whose notes are the commit subjects since the previous `v*` tag, grouped by prefix. Review the draft on GitHub and publish it. Preview the notes locally with `scripts/release-notes <from-tag-or-empty> <to-ref>`, e.g. `scripts/release-notes "" main`.

## herdr

- herdr-core (`hc`) and herdr-app run the maintainer's live terminals. `.claude/settings.json` denies driving them directly. To try herdr, use `scripts/herdr-probe`: `start` gives a throwaway terminal, every other verb works only on those, and `clean` removes them when done.
- `herdr-app commands` gives each command's arguments; `herdr-app do <cmd> --help` does not. macOS has no `timeout`; use `perl -e 'alarm 10; exec @ARGV' -- <cmd>`.

## Read before changing code

- `CODING_STANDARDS.md`: the rules and lints.
- `GLOSSARY.md`: the domain words.
- `docs/wiki/index.md`: dated decisions, their reasons and what is known open. Check the cited commit before trusting a claim.
