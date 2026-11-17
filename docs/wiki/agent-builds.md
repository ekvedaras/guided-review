# Agent builds and the maintainer's machine

## 2026-11-17: two agents' builds overloaded the Mac

- Two background agents worked at once: one on auto-merge (https://github.com/ekvedaras/guided-review/issues/95) in `.claude/worktrees/pr-status`, one on a fix for snapshot refs in linked worktrees in `.claude/worktrees/diagnose-local-open`. Both share one `target/` ([AGENTS.md](../../AGENTS.md), "The gate").
- At the maintainer's request (disk: `target/` had grown to 55 GB, 42 GB of it stale `target/debug/build`), the coordinating session deleted `target/` while both agents were building. Both then rebuilt everything from zero at the same time, and each ran the full `scripts/check` several times in a row.
- The maintainer's Mac reached load 20 with `syspolicyd` at 470% CPU: macOS checks every new binary before it runs, and each build and test run makes new ones. Other services failed (sketchybar could not start). The machine had run 11 days without a restart. Both agents were stopped.

Decided by the maintainer the same day, and written into AGENTS.md "The gate":

- One agent builds at a time. A coordinator runs build-heavy agents in sequence, not in parallel.
- Never delete `target/` while a build runs. Clean it only when no agent is building.
- Agents run `cargo fmt` and clippy first, filtered tests in the inner loop, and the full `scripts/check` once at the end, not after every fix.

Open as of 2026-11-17: `target/` grows without bound across builds; a periodic `cargo sweep` was offered to the maintainer and not yet decided.
