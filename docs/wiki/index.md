# Wiki

Notes for agents and people who work on this repo: decisions, the reasons behind them, and what is known to be open. Plans and tickets live in GitHub issues, not here. Each page is dated, so check the commit a claim cites before you trust it. The rules themselves are in [CODING_STANDARDS.md](../../CODING_STANDARDS.md) and [AGENTS.md](../../AGENTS.md); the wiki cites them and does not repeat them.

- [Panic and error policy](panic-policy.md): why `unwrap`, `expect` and `panic!` are limited the way they are.
- [Where work is tracked](delivery.md): plans and tickets in GitHub issues, the wiki keeps decisions.
- [Agent runs the app starts](agent-runs.md): the silent refresh failure and its lessons, and the 2026-11-06 decisions on the tour agent, the PR switcher and one window; why the first app-started agents could not find `guided-review`.
- [Rust quality audit, 2026-11](rust-audit-2026-11.md): what the audit set out to do, the lasting outcomes and why, and what is still open.
