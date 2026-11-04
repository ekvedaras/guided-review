# Commits go to main

On 2026-11-04 the maintainer chose small commits straight to `main` for the Rust quality audit, not a PR stack. Each commit must pass `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --all-features` before it lands.

Related: [audit log](rust-audit-2026-11.md).
