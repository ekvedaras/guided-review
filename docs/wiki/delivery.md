# Where work is tracked

On 2026-11-04 the maintainer decided that plans and tickets live in GitHub issues, not in the repo (the Rust audit's spec is issue 83 in the private archive). The wiki keeps decisions and their reasons. It does not keep step lists, findings lists or task tracking. When a piece of work ends, its page drops what is no longer relevant and keeps the rules and the reasons behind them, so nobody re-enables a skipped lint or removes a reviewed `expect` without knowing why.

The same day the maintainer chose small commits straight to `main` for the audit, not a PR stack. The gate each commit passes is in [AGENTS.md](../../AGENTS.md).

On 2026-11-06 the maintainer chose the same for the agent-loop spec (issue 107 in the private archive, tickets 108 to 116): small commits straight to `main`, each through the gate, not a PR stack. Implementers work in worktrees and their branches are fast-forwarded onto `main`.

On 2026-11-06 the maintainer decided to publish the project. This repo stays private (to be renamed `guided-review-private`), because GitHub keeps its pull request refs and issues, and some of those quote private work repos. The public `ekvedaras/guided-review` gets main's history cleaned with `git filter-repo`: private content removed from every commit, the author email changed to the maintainer's personal one, and the commit hashes the wiki cites remapped. Audit: `.claude/notes/2026-11-06-going-public.md` (scratch, not in git).

On 2026-11-06 the project went public as `ekvedaras/guided-review` with the cleaned history; the old repo is `ekvedaras/guided-review-private`, an archive. New work lands here. The old issues and pull requests stay in the private repo; commits cited before this date resolve here, issue links into 1-120 point at the private repo.

On 2026-11-14 the maintainer chose one pull request for the whole platform spec (#67, tickets #68 to #77): implementers worked in worktrees, their commits were cherry-picked onto one branch, `platform/linux`, and that branch is #78. Lessons from that run: worktrees that share one `CARGO_TARGET_DIR` replace each other's test binary mid-run, so each checkout needs its own; and adapter facts that every ticket appends to one file (`src/platform.rs` before the `Adapter` struct) conflict on every landing.

On 2026-11-14 commits from the maintainer's Linux machine are signed with the SSH key `~/.ssh/id_ed25519` (repo-local `gpg.format ssh`), because the GPG key the Mac signs with is not on it. `main`'s ruleset requires signed commits.
