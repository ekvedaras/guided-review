# deslop-please

## 2026-11-25: the linter joins the gate

- [deslop-please](https://github.com/deterministic-machines/deslop-please) 0.1.4 lints module structure: module cycles, tangles (items that know each other in a circle), fields used outside their type, modules that show most of their functions, directories over 15 entries, files over 2000 lines. It reads the index `rust-analyzer scip` writes, so `rust-toolchain.toml` now installs `rust-analyzer` and `rust-src`, and the index matches the pinned Rust.
- `scripts/deslop` pins the version and installs it when missing. `scripts/check` runs it after clippy, in CI too.
- On its first run it found 160 errors on macOS (13 module cycles, 13 tangles, 121 field leaks, 4 shallow modules, 5 wide directories, 4 long files). They went into allow lists unfixed. The gate fails only on new ones.
- One run checks one configuration. `--all-features` is the one the tests build. The default features gave 162 errors, mostly the same: their circles are smaller (the `snapshots` code is gone), and 5 field leaks show only there. Running both would double the time for those 5.
- One allow list per target triple: Linux adds `shallow-module guided_review::platform`, since `src/platform` has other code there. A single list would tell a Mac run that the Linux entry is fixed, and following its tip to update the list would then break Linux CI. `scripts/deslop --update-allow` rewrites every list from a Mac with `--target`.

Open as of 2026-11-25: the 160 errors.
