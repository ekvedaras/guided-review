# Where work is tracked

On 2026-11-04 the maintainer decided that plans and tickets live in GitHub issues, not in the repo (the Rust audit's spec is [issue 83](https://github.com/ekvedaras/guided-review/issues/83)). The wiki keeps decisions and their reasons. It does not keep step lists, findings lists or task tracking. When a piece of work ends, its page drops what is no longer relevant and keeps the rules and the reasons behind them, so nobody re-enables a skipped lint or removes a reviewed `expect` without knowing why.

The same day the maintainer chose small commits straight to `main` for the audit, not a PR stack. The gate each commit passes is in [AGENTS.md](../../AGENTS.md).
