# Commits go to main

On 2026-11-04 the maintainer chose small commits straight to `main` for the Rust quality audit, not a PR stack. Each commit must pass `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --all-features` before it lands.

Related: [audit log](rust-audit-2026-11.md).

On 2026-11-04 the maintainer decided that plans and tickets live in GitHub issues, not in the repo. The wiki keeps decisions and their reasons. It does not keep step lists or task tracking. When the audit ends, the wiki drops what is no longer relevant (fixed findings, finished plans) and keeps the rules and the reasons behind them.
