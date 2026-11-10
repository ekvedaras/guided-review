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

`scripts/snapshot <pr-url> [out-dir]` writes PNGs of the app to `<out>/light` and `<out>/dark` (default `/tmp/guided-review-snapshots`): `NN-top`/`NN-end` of every tour screen, then `20-settings` to `27-loading-screen`, staged in `src/app/layouts/one_step/scenes.rs`. It builds the `snapshots` feature, runs on a throwaway HOME with only that PR's tour, and draws on a timer, so it works with the screen locked. A scene is taken once its picture is unchanged for 1s (20s cap), so finite animations end first. The PR needs a tour in `~/.guided-review/tours`; use public PRs only (ekvedaras/guided-review#12 has one) for pictures that leave the machine. `screencapture` is blocked in auto mode.

`scripts/snapshot-compare <before-dir> <after-dir> <out-dir> [markers.json]` puts each scene side by side, before left. Markers number spots on the after picture, in its pixels: `{ "21-jump-palette": [{ "x": 640, "y": 260, "n": 1 }] }`.

## Releasing

Bump the version in `Cargo.toml` and `Cargo.lock`, then push the tag `v<version>`. The release workflow builds the archives and creates a draft release whose notes are the commit subjects since the previous `v*` tag, grouped by prefix. Review the draft on GitHub and publish it. Preview the notes locally with `scripts/release-notes <from-tag-or-empty> <to-ref>`, e.g. `scripts/release-notes "" main`.

## herdr

- herdr-core (`hc`) and herdr-app run the maintainer's live terminals. `.claude/settings.json` denies driving them directly. To try herdr, use `scripts/herdr-probe`: `start` gives a throwaway terminal, every other verb works only on those, and `clean` removes them when done.
- `herdr-app commands` gives each command's arguments; `herdr-app do <cmd> --help` does not. macOS has no `timeout`; use `perl -e 'alarm 10; exec @ARGV' -- <cmd>`.

## Read before changing code

- `CODING_STANDARDS.md`: the rules and lints.
- `GLOSSARY.md`: the domain words.
- `docs/wiki/index.md`: dated decisions, their reasons and what is known open. Check the cited commit before trusting a claim.
