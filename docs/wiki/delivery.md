# Where work is tracked

On 2026-11-04 the maintainer decided that plans and tickets live in GitHub issues, not in the repo. The wiki keeps decisions and their reasons. It does not keep step lists, findings lists or task tracking. When a piece of work ends, its page drops what is no longer relevant and keeps the rules and the reasons behind them, so nobody re-enables a skipped lint or removes a reviewed `expect` without knowing why. The gate each commit passes is in [AGENTS.md](../../AGENTS.md).

## Repository

- On 2026-11-06 the project went public as `ekvedaras/guided-review`. Its history was cleaned with `git filter-repo`: private content removed from every commit, the author email changed to the maintainer's personal one, and the commit hashes the wiki cites remapped. Private work repos are quoted in the old repo's pull request refs and issues, so that repo stays private as the archive `ekvedaras/guided-review-private`. New work lands in the public repo. Old issues and pull requests stay in the archive: issue links into 1-120 point at it, and commits cited before this date resolve in the public repo.
- `main`'s ruleset requires signed commits. Commits from the maintainer's Linux machine are signed with the SSH key `~/.ssh/id_ed25519` (repo-local `gpg.format ssh`), because the GPG key the Mac signs with is not on it (2026-11-14).

## How a spec is delivered

The maintainer chooses per spec.

- The Rust audit (2026-11-04) and the agent-loop spec (2026-11-06) landed as small commits straight to `main`, each through the gate, not a PR stack. Implementers worked in worktrees and their branches were fast-forwarded onto `main`.
- The platform spec (2026-11-14, #67) went in as one pull request, #78, from the branch `platform/linux`: implementers worked in worktrees and their commits were cherry-picked onto it. Adapter facts that every ticket appended to one file conflicted on every landing, so give each ticket its own place for such facts. The first lesson of that run, that each worktree needs its own `CARGO_TARGET_DIR`, was reversed on 2026-11-16: worktrees share one `target` because a `target/` per worktree filled the disk (see [agent-builds.md](agent-builds.md)).
