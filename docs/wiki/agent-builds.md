# Agent builds and the maintainer's machine

On 2026-11-17 two background agents built at once after the coordinating session deleted `target/` (it had grown to 55 GB, 42 GB of it stale `target/debug/build`) while both were building. Both rebuilt everything from zero and each ran the full `scripts/check` several times. The maintainer's Mac reached load 20 with `syspolicyd` at 470% CPU, since macOS checks every new binary before it runs and each build and test run makes new ones. Other services failed (sketchybar could not start). Both agents were stopped.

The maintainer decided the same day, and wrote into [AGENTS.md](../../AGENTS.md) ("The gate"):

- One agent builds at a time. A coordinator runs build-heavy agents in sequence, not in parallel.
- Never delete `target/` while a build runs. Clean it only when no agent is building.
- Agents run `cargo fmt` and clippy first, filtered tests in the inner loop, and the full `scripts/check` once at the end, not after every fix.
- Agent sessions and worktrees share the main checkout's `target` (`CARGO_TARGET_DIR` in `.claude/settings.json`), because a `target/` per worktree (3-5 GB each) filled the disk on 2026-11-16. Two builds at once then wait for each other's cargo lock.

Open: `target/` grows without bound across builds. A periodic `cargo sweep` was offered to the maintainer and not decided.
