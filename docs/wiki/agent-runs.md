# Agent runs the app starts

How the app hands work to Claude, what went wrong, and what was decided.

## The refresh that failed without a word (2026-11-06)

The maintainer reported that Refresh spun, then showed the old tour and asked to refresh again. Two faults, found from four real refresh transcripts of 2026-11-03 to 05:

- The refresh writer runs `claude -p` with `--permission-mode dontAsk` and a fixed tool list. Claude chained `guided-review tour-path … && …` in one Bash call. `dontAsk` denied it, because `tour-path` was not on the list and chained calls do not match a single-command rule. Claude said so and exited 0.
- The app took exit 0 as success and threw away what Claude said.

Fixed in `ekvedaras/guided-review@8030ad0`, `a8cdb64` and `8a99930`: a run that leaves the refresh marks fails with Claude's words, the sidebar names it until a refresh goes through, `tour-path` is allowed, and the prompt asks for one command per Bash call.

Lessons that hold beyond this bug:

- Exit 0 from an agent proves nothing. Check the outcome the run was meant to produce (here: the marks are gone).
- An agent with a fixed tool list fails on chained shell commands. Prompts and skills it reads should ask for one command per call.
- As of `8a99930` the app keeps no log of an agent run's output, so the only evidence was Claude Code's own transcripts. Open.

## One window, a PR switcher, and the tour agent hears comments (2026-11-06)

The maintainer asked for: comments reach the tour's agent at once; a PR switcher that starts a tour writer for a PR with no tour; one window that every CLI command reuses. The spec goes to a GitHub issue ([where work is tracked](delivery.md)).

The maintainer decided on 2026-11-06:

- On a comment the agent answers it and fixes the tour if the comment shows the tour is wrong. It does not change the PR's code. Reason: the user reviews other people's PRs, where the agent has no business pushing code.
- herdr (`hc`) is required for agent features. No hidden `claude -p` fallback for them. Reason: one way to talk to agents, and the user can watch or take over any agent in herdr-app. Without herdr the app turns those features off and says why.
- The switcher lists every open PR that involves the user plus PRs with their team's review requested, no date limit, newest update first. It starts from the maintainer's own PR badge query, without its date limit.

Superseding part of the above, on 2026-11-06 the maintainer decided that not every comment goes to the tour agent (a side review pointed out that comments meant for the PR author would get a 🤖 reply under the maintainer's name). The comment box gets two send actions: "Comment" goes to GitHub for the author with no agent, and "Ask agent" goes to GitHub and to the tour agent, which answers on that thread with 🤖. The maintainer kept the agent Q&A on GitHub on purpose: it is a lasting trace of the discussion about the change.

On 2026-11-06 the maintainer set which terminal becomes a PR's tour agent on `open` (a side review found that a plain shell could be claimed, and `hc send` would run a question as a shell command): the Claude session that ran `open`; else, when `open` ran in a plain shell, a Claude terminal in the same herdr session; else the app starts its own agent. The coordinator added two limits: only a terminal that runs Claude is ever claimed, and with several Claude terminals in that session none is claimed, because a busy unrelated session would get the PR's questions mid-work.

## The tour agent may change the PR's code (2026-11-10)

The maintainer asked for a 👀 on a comment as soon as the agent gets it, taken off when it replies, and for a "Working on it…" reply that the agent's answer replaces when it goes off to change code. This reverses the 2026-11-06 decision that the agent never changes the PR's code.

The maintainer decided on 2026-11-10:

- The agent may change code on any PR, not only the maintainer's own.
- It works in its own folder when that is a clone of the repo, with no restrictions; else in a cheap worktree. The message and the tour skill add: only when the clone is on the PR's branch with none of the reviewer's own changes, and the agent commits and pushes to the PR's branch.
- An agent the app starts runs with `--permission-mode auto` instead of `dontAsk`, so it can edit, test and push. Chosen over `bypassPermissions` to keep auto mode's block on destructive commands, since the agent reads comments and code of other people's PRs. The tool list stays as the calls auto mode allows without its classifier. The writer of a new tour from the PR switcher is the same terminal, so it runs in auto mode too. The refresh writer (`claude -p`) keeps `dontAsk`.

How it is built: the viewer puts the 👀 (the seen mark) when the delivery goes through, not the agent, so it cannot be forgotten. `agent-reply --working` posts the reply and the hand-off keeps its comment id; the next `agent-reply --body` edits that comment. Both take the 👀 off. Open: whether GitHub takes a reaction on a comment still in a pending review is not tried live; if it refuses, the line under the thread says so.

## Hand-offs and herdr, checked on a real terminal (2026-11-06)

- `hc events --json` writes a snapshot line, then JSON-RPC lines: `params.type` is `status` (with `params.data.status.state`) or `turn`, and `params.terminal` names the terminal. Recorded in `tests/fixtures/hc/events-claude-turn.jsonl`. The first parser expected the snapshot shape for changes too and would never have seen one.
- `hc send --if idle` refuses with exit 10 and a JSON error on stderr (`kind: status_mismatch`, "t2b is working, not idle"). Fixtures are in `tests/fixtures/hc/`.
- After a turn herdr reports `done` until someone looks at the terminal, and `--if idle` refuses `done` too. An agent that works in the background would never take a comment, so the adapter sends plainly when the refusal says `done`. A `working` or `blocked` agent is still never sent to.
- Not done yet: the hand-off state is not shown under the comment in the thread, and no tour agent is started when none is found.

## Writing a tour from the PR switcher (2026-11-06)

- The writer is an interactive `claude` in herdr under `--permission-mode dontAsk` and `tour_agent_tools()`, the setup that made Refresh fail silently. A test (`tests/it/tour_skill.rs`) holds every command the tour skill names in backticks to that list. It found `mkdir -p`, which the list denies; the skill now leaves the folder to the Write tool. The skill and the writer's first task ask for one command per Bash call.
- The writer's `open` is the only "done" signal. A writer that goes idle after working without that `open`, whose terminal ends, or that could not start (the cap of 3) fails; the loading screen shows why with its last screen lines, Try again and Show agent, and the switcher row says "writing failed".
- An `open` from a terminal the app started (`gr.started-by=app`) of a PR not on screen never switches the window; the CLI answers `kept_in_background`. Which tours are being written and which PRs got an agent reply while not shown are kept in `~/.guided-review/state/app.json` (2026-11-06), so a restart keeps them. After a restart each writing finds its terminal again by the PR's `gr.pr` label and is followed on; with no such terminal it fails with a reason. Only the PR urls are stored, never terminal ids. Not hand-checked on a real restart.
- Checked on a probe terminal: a writer with that tool list and first task wrote, checked and opened the tour of ekvedaras/guided-review#82 (one file) with no denied call, making the tour folder through Write. `open` was stubbed so the maintainer's app was not reached, and the installed skill was the one before this change.
- `hc read <t> --json --lines N` answers `{"complete","how","text"}`; fixture in `tests/fixtures/hc/read-lines.json`. Of a Claude terminal the last 20 lines are mostly its prompt box and status bar (fixed below).

## Agents the app started could not find `guided-review` (2026-11-06)

The maintainer's hand test: Ask agent on a PR with no tour agent started t2q, then showed "Hand-off failed: `hc send` failed: {json…}"; macOS asked for permissions at that moment. Picking a PR with no tour in the switcher started t2r, and the loading screen soon said the tour agent stopped without a tour, showing only Claude's empty prompt box.

Found from `hc get`/`hc read` of t2q and t2r and their Claude transcripts:

- Both agents got their first task (on Claude's command line) and worked on it. In both, the first `guided-review` call failed with `command not found`. `hc run` starts a terminal with the herdr server's environment, not the caller's: their `PATH` was `~/.local/bin:/opt/homebrew/bin:/usr/bin:/bin`, without `~/.cargo/bin`. t2r said so and stopped, so the writing failed rightly. t2q, told to find the tour, searched with Glob over the whole home folder (ripgrep timed out after 20 s) and `~/Library/Application Support`. That search is what raised the macOS permission prompts. The app runs no AppleScript.
- The hand-off to t2q was sent while Claude was busy with its first task. Herdr refuses to type into a terminal whose screen has not settled since it started: exit 5, `kind: unavailable`, "t2q is still starting: its output has not settled" (reproduced on a probe, fixture `tests/fixtures/hc/send-refused-starting.json`). The adapter took every refusal other than `status_mismatch` as a failure.
- `hc read --lines N` gives N lines of history above the whole screen, not the last N lines. A Claude screen is 120 rows with its work at the top, blank rows, then the prompt box and status bar, so the last 20 lines held none of the work. Fixture of t2r's screen: `tests/fixtures/hc/read-claude-after-turn.json`.

Fixed: an agent the app starts runs under `env PATH=<the running guided-review's folder>:<the app's PATH>` (checked on a probe: herdr still sees `claude-code`, and `guided-review` runs). "Still starting" keeps the hand-off queued and the app tries again after 3 s. Herdr's errors read as its message, not its JSON, and a failed hand-off in a thread has Try again. The loading screen shows the last non-blank lines above Claude's prompt box.

Later the same day: the writer's and the tour agent's tool lists name `Read(//<clone>/**)` and `Read(~/.guided-review/tours/**)` and no bare Read, Grep or Glob, so `dontAsk` denies a search of the home folder. Only `Read(path)` rules match: Claude Code's docs say `Grep(path)` and `Glob(path)` are accepted but never applied. Open: that an allow `Read` rule also governs Grep and Glob outside the clone is not stated in the docs and not yet tried on a live agent.

## Agents the app started were not in herdr-app (2026-11-06)

The maintainer could not see or watch the agents the app starts: live in herdr-core, absent from the herdr-app window. Herdr-app shows only terminals labelled `herdr.space=<id>` (and `herdr.tab=<id>`); `hc run` without them stays a hidden worker. Checked on probe terminals with `scripts/herdr-probe start-placed`:

- Any id string works. An id herdr-app does not know becomes a new space or tab, so labelling at `hc run` places the agent with the window closed too; the window takes it up when it opens.
- Herdr-app takes a new terminal up about a second after `hc run` returns. Before that `herdr-app do space.rename` answers `{"error":"no space …"}` with exit 1. With no window, every `herdr-app do` exits 2 ("no Herdr window"); it does not open one.
- A terminal labelled after it started is taken up too. Relabelling one already shown does not move it.
- A program that ends at once closes its pane, and the last tab closing closes the space.

Decided (`ekvedaras/guided-review@1fbdb5a`, `a8bad50`): every agent the app starts is labelled `herdr.space=guided-review`, `herdr.tab=<its terminal name>`. The adapter then renames the space "guided-review agents" and the tab `owner/repo#N`, retrying for up to 3 s, and gives up quietly with no window. Not plain "guided-review": the maintainer's own space of this repo is already called that. Show agent labels an app-started agent that has no place yet (one started before this change), names a place made while the window was closed, and never moves a terminal the reviewer started.

Open: an agent started while the window is closed shows in an unnamed space (called after its clone's repo) and a tab called by its number, until Show agent or the next start with the window open names them.

2026-11-06, tried live with `scripts/herdr-probe` (`claude --permission-mode dontAsk`, cwd the clone): with only `Read(//<clone>/**)` and `Read(~/.guided-review/tours/**)` listed, the agent had no Grep and no Glob tool at all, and said so; Read inside the clone was allowed and Read of `~/.zshrc` was denied. With bare `Grep,Glob` added, both ran and `Read(~/.zshrc)` was still denied. So the tour agent's and the writer's lists now carry bare `Grep` and `Glob`; the agent's cwd is the clone, but a Grep or Glob given another path is not stopped. Supersedes the "Open" note above.

## An Ask handed to the reviewer's own agent may change files (2026-11-09)

The maintainer asked Ask for a flow diagram saved as a markdown file near the code. Ask runs Claude with read-only tools, so it pasted the file into its answer instead. Comment hand-offs could not do it either: they tell the tour agent "Never change the PR's code", and an agent the app started has no tool that writes outside the tours folder.

Decided: an Ask stays a fast, private, read-only chat. Its "Hand to your agent" action types the whole chat into the PR's tour agent with leave to change files in the agent's own folder, and nothing goes to GitHub. It goes only to an agent the reviewer started (one their `guided-review open` claimed), never to one the app started, which cannot write; then the panel says why and how to claim one. There is no queue: a busy agent is a "try again" line. Comment hand-offs keep "Never change the PR's code".

Considered and turned down: an Ask that answers through the tour agent. It needs a reply path back into the panel, fills the agent's context with every small question, and loses the three Asks that run side by side.

## Save in Settings reopens the PR, but waits for work in flight (2026-11-10)

A saved `config.toml` only reaches a PR when it is built, so Save opens the PR on screen again behind the Opening veil. Reopening swaps the window's root, and the old root owns the Ask processes and the tasks that type hand-offs into the tour agent: dropping it mid-turn would cut an Ask's answer and a hand-off half typed (`ekvedaras/guided-review@fb856ef`, `fcbda27`).

Decided: the reopen waits until no Ask turn is answering, no Ask is being handed to the reviewer's agent and no hand-off is being delivered, and the window says which in a line under the title bar (`ReopenAfterSave`, `InFlight`). Saving again while it waits is still one reopen. The wait lives in the layout, so a switch to another PR, which drops the layout, ends it.

An agent reply that lands while the build runs is applied to the old window as before and also held by `ActivePr`, because the build may have read the state file before the old session saved the reply; the new window applies the held replies again (harmless, it only sets a hand-off's state). Likewise the old session must not save reviewer input after the build read the state file, since the new session writes its sections whole: while the reopen builds, the layout saves typing and takes focus out of the boxes, and ignores the navigation, wrap, jump and settings keys; the veil keeps the mouse out.

Open: a reopen that finds another PR building is dropped (the build reads the new config anyway), and a failed reopen shows the reason in the window and does not retry.
