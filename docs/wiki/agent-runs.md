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

## Hand-offs and herdr, checked on a real terminal (2026-11-06)

- `hc events --json` writes a snapshot line, then JSON-RPC lines: `params.type` is `status` (with `params.data.status.state`) or `turn`, and `params.terminal` names the terminal. Recorded in `tests/fixtures/hc/events-claude-turn.jsonl`. The first parser expected the snapshot shape for changes too and would never have seen one.
- `hc send --if idle` refuses with exit 10 and a JSON error on stderr (`kind: status_mismatch`, "t2b is working, not idle"). Fixtures are in `tests/fixtures/hc/`.
- After a turn herdr reports `done` until someone looks at the terminal, and `--if idle` refuses `done` too. An agent that works in the background would never take a comment, so the adapter sends plainly when the refusal says `done`. A `working` or `blocked` agent is still never sent to.
- Not done yet: the hand-off state is not shown under the comment in the thread, and no tour agent is started when none is found.

## Writing a tour from the PR switcher (2026-11-06)

- The writer is an interactive `claude` in herdr under `--permission-mode dontAsk` and `tour_agent_tools()`, the setup that made Refresh fail silently. A test (`tests/it/tour_skill.rs`) holds every command the tour skill names in backticks to that list. It found `mkdir -p`, which the list denies; the skill now leaves the folder to the Write tool. The skill and the writer's first task ask for one command per Bash call.
- The writer's `open` is the only "done" signal. A writer that goes idle after working without that `open`, whose terminal ends, or that could not start (the cap of 3) fails; the loading screen shows why with its last screen lines, Try again and Show agent, and the switcher row says "writing failed".
- An `open` from a terminal the app started (`gr.started-by=app`) of a PR not on screen never switches the window; the CLI answers `kept_in_background`. Which tours are being written and which PRs got an agent reply while not shown live in the app's memory only, so a restart forgets them.
- Checked on a probe terminal: a writer with that tool list and first task wrote, checked and opened the tour of ekvedaras/guided-review#82 (one file) with no denied call, making the tour folder through Write. `open` was stubbed so the maintainer's app was not reached, and the installed skill was the one before this change.
- `hc read <t> --json --lines N` answers `{"complete","how","text"}`; fixture in `tests/fixtures/hc/read-lines.json`. Of a Claude terminal the last 20 lines are mostly its prompt box and status bar (fixed below).

## Agents the app started could not find `guided-review` (2026-11-06)

The maintainer's hand test: Ask agent on a PR with no tour agent started t2q, then showed "Hand-off failed: `hc send` failed: {json…}"; macOS asked for permissions at that moment. Picking a PR with no tour in the switcher started t2r, and the loading screen soon said the tour agent stopped without a tour, showing only Claude's empty prompt box.

Found from `hc get`/`hc read` of t2q and t2r and their Claude transcripts:

- Both agents got their first task (on Claude's command line) and worked on it. In both, the first `guided-review` call failed with `command not found`. `hc run` starts a terminal with the herdr server's environment, not the caller's: their `PATH` was `~/.local/bin:/opt/homebrew/bin:/usr/bin:/bin`, without `~/.cargo/bin`. t2r said so and stopped, so the writing failed rightly. t2q, told to find the tour, searched with Glob over the whole home folder (ripgrep timed out after 20 s) and `~/Library/Application Support`. That search is what raised the macOS permission prompts. The app runs no AppleScript.
- The hand-off to t2q was sent while Claude was busy with its first task. Herdr refuses to type into a terminal whose screen has not settled since it started: exit 5, `kind: unavailable`, "t2q is still starting: its output has not settled" (reproduced on a probe, fixture `tests/fixtures/hc/send-refused-starting.json`). The adapter took every refusal other than `status_mismatch` as a failure.
- `hc read --lines N` gives N lines of history above the whole screen, not the last N lines. A Claude screen is 120 rows with its work at the top, blank rows, then the prompt box and status bar, so the last 20 lines held none of the work. Fixture of t2r's screen: `tests/fixtures/hc/read-claude-after-turn.json`.

Fixed: an agent the app starts runs under `env PATH=<the running guided-review's folder>:<the app's PATH>` (checked on a probe: herdr still sees `claude-code`, and `guided-review` runs). "Still starting" keeps the hand-off queued and the app tries again after 3 s. Herdr's errors read as its message, not its JSON, and a failed hand-off in a thread has Try again. The loading screen shows the last non-blank lines above Claude's prompt box.

Open: the agents' Read, Grep and Glob are not limited to the clone and the tours folder, so an agent that goes searching can still touch the whole home folder and raise permission prompts.

## Agents the app started were not in herdr-app (2026-11-06)

The maintainer could not see or watch the agents the app starts: live in herdr-core, absent from the herdr-app window. Herdr-app shows only terminals labelled `herdr.space=<id>` (and `herdr.tab=<id>`); `hc run` without them stays a hidden worker. Checked on probe terminals with `scripts/herdr-probe start-placed`:

- Any id string works. An id herdr-app does not know becomes a new space or tab, so labelling at `hc run` places the agent with the window closed too; the window takes it up when it opens.
- Herdr-app takes a new terminal up about a second after `hc run` returns. Before that `herdr-app do space.rename` answers `{"error":"no space …"}` with exit 1. With no window, every `herdr-app do` exits 2 ("no Herdr window"); it does not open one.
- A terminal labelled after it started is taken up too. Relabelling one already shown does not move it.
- A program that ends at once closes its pane, and the last tab closing closes the space.

Decided (`ekvedaras/guided-review@7f304f2`, `599eeca`): every agent the app starts is labelled `herdr.space=guided-review`, `herdr.tab=<its terminal name>`. The adapter then renames the space "guided-review agents" and the tab `owner/repo#N`, retrying for up to 3 s, and gives up quietly with no window. Not plain "guided-review": the maintainer's own space of this repo is already called that. Show agent labels an app-started agent that has no place yet (one started before this change), names a place made while the window was closed, and never moves a terminal the reviewer started.

Open: an agent started while the window is closed shows in an unnamed space (called after its clone's repo) and a tab called by its number, until Show agent or the next start with the window open names them.
