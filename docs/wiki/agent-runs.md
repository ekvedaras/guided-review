# Agent runs the app starts

How the app hands work to Claude, what went wrong, and what was decided.

## The refresh that failed without a word (2026-11-06)

The maintainer reported that Refresh spun, then showed the old tour and asked to refresh again. Two faults, found from four real refresh transcripts of 2026-11-03 to 05:

- The refresh writer runs `claude -p` with `--permission-mode dontAsk` and a fixed tool list. Claude chained `guided-review tour-path … && …` in one Bash call. `dontAsk` denied it, because `tour-path` was not on the list and chained calls do not match a single-command rule. Claude said so and exited 0.
- The app took exit 0 as success and threw away what Claude said.

Fixed in `ekvedaras/guided-review@b98b0eb`, `83ac0ac` and `b9a2fec`: a run that leaves the refresh marks fails with Claude's words, the sidebar names it until a refresh goes through, `tour-path` is allowed, and the prompt asks for one command per Bash call.

Lessons that hold beyond this bug:

- Exit 0 from an agent proves nothing. Check the outcome the run was meant to produce (here: the marks are gone).
- An agent with a fixed tool list fails on chained shell commands. Prompts and skills it reads should ask for one command per call.
- As of `b9a2fec` the app keeps no log of an agent run's output, so the only evidence was Claude Code's own transcripts. Open.

## One window, a PR switcher, and the tour agent hears comments (2026-11-06)

The maintainer asked for: comments reach the tour's agent at once; a PR switcher that starts a tour writer for a PR with no tour; one window that every CLI command reuses. The spec goes to a GitHub issue ([where work is tracked](delivery.md)).

The maintainer decided on 2026-11-06:

- On a comment the agent answers it and fixes the tour if the comment shows the tour is wrong. It does not change the PR's code. Reason: the user reviews other people's PRs, where the agent has no business pushing code.
- herdr (`hc`) is required for agent features. No hidden `claude -p` fallback for them. Reason: one way to talk to agents, and the user can watch or take over any agent in herdr-app. Without herdr the app turns those features off and says why.
- The switcher lists every open PR that involves the user plus PRs with their team's review requested, no date limit, newest update first. It starts from the maintainer's own PR badge query, without its date limit.

Superseding part of the above, on 2026-11-06 the maintainer decided that not every comment goes to the tour agent (a side review pointed out that comments meant for the PR author would get a 🤖 reply under the maintainer's name). The comment box gets two send actions: "Comment" goes to GitHub for the author with no agent, and "Ask agent" goes to GitHub and to the tour agent, which answers on that thread with 🤖. The maintainer kept the agent Q&A on GitHub on purpose: it is a lasting trace of the discussion about the change.

On 2026-11-06 the maintainer set which terminal becomes a PR's tour agent on `open` (a side review found that a plain shell could be claimed, and `hc send` would run a question as a shell command): the Claude session that ran `open`; else, when `open` ran in a plain shell, a Claude terminal in the same herdr session; else the app starts its own agent. The coordinator added two limits: only a terminal that runs Claude is ever claimed, and with several Claude terminals in that session none is claimed, because a busy unrelated session would get the PR's questions mid-work.
