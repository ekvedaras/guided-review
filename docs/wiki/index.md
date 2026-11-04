# Wiki

Notes for agents and people who work on this repo. Each page is a dated record, not a description of the code today. Check the commit a claim cites before you trust it.

## Decisions

- [Panic and error policy](panic-policy.md): where `unwrap`, `expect` and `panic!` may appear, and how the lints enforce it.
- [Commits go to main](delivery.md): how audit fixes ship.

## Work logs

- [Check loop, 2026-11](check-loop.md): the gate went from 178 s to 41 s: one test crate, `scripts/check`, `AGENTS.md`.
- [Rust quality audit, 2026-11](rust-audit-2026-11.md): the repo-wide review for idiomatic, panic-free, lean Rust, its findings and the fix order.
