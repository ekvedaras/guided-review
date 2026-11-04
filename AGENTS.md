# Guided review

Rust 2024 gpui app.

## The gate

`scripts/check` is the gate before every commit: fmt, clippy, tests, stopping at the first failure, ending on `ALL GREEN`. CI runs the same script. It finds `cargo` itself.

- Start it with `run_in_background` and read only until it ends. A commit stages what is on disk, so edit nothing while it runs, and stage first or check `git diff` against what you commit.
- Inner loop: `scripts/check <test-name-filter>` runs only matching tests.

## Tests

- All integration tests are one crate, `tests/it/main.rs`, one module per topic. A new file needs a `mod` line there.
- `tests/it/support/` holds the fixture repo, CLI runner and fake ports (`fake_ports.rs`). `tests/fixtures/` holds recorded data.
- `src/viewer/` is the window-free core and is tested through those fakes. `src/app/` is gpui with no window tests, so put logic behind a viewer method.

## Read before changing code

- `CODING_STANDARDS.md`: the rules and lints.
- `GLOSSARY.md`: the domain words.
- `docs/wiki/index.md`: dated decisions and work logs. Check the cited commit before trusting a claim.
