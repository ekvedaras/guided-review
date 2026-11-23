# Glossary

**Tour**: A reading route through one review subject: a guided tour or a basic tour. Both are pinned to the same base and head.

**Review subject**: What a review reads: a PR, or a local change set. A type in the code, never a flag or a string checked for a prefix. Its one text form is the subject key: a PR's is its URL, a local change set's is `local:<folder>@<branch>` (an `@` or `%` in the folder written `%40`, `%25`). Everything a review keeps is keyed by it. Avoid: target.

**Local change set**: One branch of one working tree: its commits that are not on the repo's default branch (from their merge base), plus the uncommitted and untracked files git does not ignore. Its identity is the folder (the working tree's root) and the branch; the repo, from `origin`, only names it. Avoid: local PR.

**Local review**: A review of a local change set, with no GitHub in it: started with `guided-review open <path>` or from the PR switcher, read as a basic tour first, its comments tasks for the change set's agent.

**Snapshot**: The commit object the app makes of the working tree when a local review opens and at Update, with `HEAD` as its parent (`HEAD` itself when the tree is clean). A ref under `refs/guided-review/` keeps it from `git gc`; the app never pushes it. What a local review reads never moves under the reviewer. Avoid: stash, live view.

**Changed on disk**: A local review's working tree no longer matches its snapshot: the branch got commits or files changed. The sidebar says so with Update, which re-pins the tour to a new snapshot. Avoid: new commits (that is a PR's).

**Task**: A comment of a local review, for the change set's agent. It never leaves the machine. Avoid: local comment, todo.

**Queued task**: A task held back by Add to tasks, shown under its lines, editable and deletable until Send tasks. Avoid: pending task, draft (that is a comment box's text).

**Add to tasks / Send now / Send tasks**: A local review's send actions. Add to tasks queues the comment as a queued task; Send now hands it to the change set's agent at once; Send tasks, the finish of a local review, hands every queued task over as one message in reading order. They mirror a PR's Add to review, Comment now and Finish review.

**Change set's agent**: The herdr agent that takes a local review's tasks: the terminal that ran `guided-review open` when it runs Claude, else the only Claude terminal working in the folder, else the one the reviewer picks among several, else one the app starts there. Claimed by the herdr label `gr.local=<subject key>`. Unlike a tour agent it is not told to commit and push. Avoid: local agent.

**Move to PR**: Carrying a local review over to the PR opened from its branch: read ticks carry by the re-pin rule, open tasks become draft comments in the PR's review, resolved ones are dropped, and the local review goes. Only on the reviewer's say. Avoid: migrate, convert.

**Open PR**: A local review's action, until its branch has an open PR: it types `pr it` into the change set's agent, so the agent's own skills open the PR. Move to PR is offered once GitHub has it. Avoid: create PR.

**Branch link**: What a review knows of the other side of Move to PR: in a local review, the open PR GitHub says is opened from its branch; in a PR's review, the local review on disk of the branch the PR is opened from. It decides what the review offers, Open PR, Move to PR or Keep local review, from whichever side. Only a decline is saved, in the local review's state file, so it holds on both sides. Avoid: PR match, pairing.

**Guided tour**: A tour the tour writer wrote with the `tour` skill: summary, steps and safety net, top-down: main flow first, details later. A tour file with no `kind` is guided. Avoid: AI tour, full tour.

**Basic tour**: A tour made with no AI: every changed file of the PR with its diff, one screen per file, the sidebar a file tree like an IDE. It has no steps, no summary and no safety net; its file says `"kind": "basic"`, holds the pin and the PR's own title (`pr_title`), and sits beside the guided tour's (`<number>.basic.json`). A PR with no tour opens as one. The window names it Basic mode and offers Write guided tour. Avoid: empty tour, plain tour, quick tour.

**Guided tour ready**: A guided tour its writer finished while the reviewer reads the PR's basic tour. The viewer holds that fact and shows it in a lasting banner; it never reloads or switches by itself, only when the reviewer asks, and then the basic tour goes. Avoid: reload, auto-switch.

**Overview**: The first screen of a basic tour: the PR's title and description, its counts, its changed files with their read ticks, and Write guided tour. Avoid: summary (that is the guided tour's).

**Summary**: The short text at the start of a tour. At most three bullets that say what the PR does.

**Reviewer summary**: An extra summary at the start of a tour, written to the reviewer's own summary instructions. It is saved in the tour file next to the summary (`reviewer_summary`, Markdown, no length limit). Avoid: custom summary, extra summary, brief.

**Summary instructions**: What tells the tour writer how to write the reviewer summary: a file, a skill (`summary_instructions = { skill = "<name>" }`) or off, set in `config.toml` as `summary_instructions`. With no key, `summary.md` in the config folder (see App folders) is used when it exists. `guided-review outline` hands the text to the writer. Avoid: prompt, template.

**Primary and secondary action**: The two send actions of a comment or reply box the reviewer reaches for most, each one of Add to review, Comment now, Suggest change, Ask agent or Ask agent now, set in Settings as `primary_comment_action` and `secondary_comment_action`; never the same one (picking the other's action swaps them). The primary gets the accent button, comes first and has Cmd+Enter; the secondary comes second and has Cmd+Shift+Enter; every other action has a key of its own. A box that lacks a chosen action (no pending-review-free Comment now, no agent link, a reply has no Suggest change) lets the nearest one it has stand in, which then shows the main key; a local review maps Add to review to Add to tasks and Comment now to Send now. Avoid: default action, main button.

**Settings**: Where a reviewer edits their settings in the app: summary instructions, clones, IDE, Ask model and Ask effort, the primary and secondary actions. Save checks every setting first and writes nothing while one is wrong; it writes only the keys that changed back to `config.toml` in the config folder, so the reviewer's comments stay. A custom IDE template or Ask model is kept when a preset is picked, in `ide_custom` and `ask_model_custom` beside `ide` and `ask_model`; only Settings reads those two keys. It has pages, picked in a list on its left (`viewer::SettingsPage`): Agents (summary and review instructions, Ask), Code (IDE, more lines, clones), Pull requests (comments, merge; not named after one host, so it fits GitLab too), Keys and About (version). A page with a problem has a red mark in the list, and a refused Save shows the first page with a problem unless the shown page has one. With a PR open it is a sheet over the whole window; with no PR open, `guided-review settings` shows it alone in the app's one window. Avoid: settings screen (**Screen** means summary, step or safety net), preferences.

**Clone offer**: The sidebar's offer to clone the PR's repo when no local clone was found and its code is read from GitHub. The folder is suggested beside the clones Settings names, else in a usual code folder of home (`~/Work`, `~/Code`, `~/Projects`, `~/PhpStormProjects`, …), else in home; a picked folder that already is a clone is used as it is. `gh repo clone` makes it, Settings saves it, and the tour is read again on it in place. Not now hides it for this review only.

**Related issue**: The GitHub issue, or ticket in another tracker, a review is about, which the title bar's button and the key `I` open. The first source that names any wins: the issues GitHub links to the PR (its closing references), then the ones its description names after GitHub's closing keywords (`Closes`, `Fixes`, `Resolves` and their forms), `Part of` (`#12`, `owner/repo#12` or a link) or `Ticket:` (any link), then the guided tour's `issue`, then the branch name (`123-totals`, `feat/123-totals`, `issue-123`, `gh-123`). With several, the button lists them, each opened by a click or its number key. Avoid: linked issue (that is only GitHub's source).

**Merge route**: The branch a review's change goes into (base) and the one it comes from (head), shown in the title bar as `base ← head`. A PR's is what GitHub says now, read again since the PR can be retargeted, and a head from a fork is written `owner:branch`; a local review's is its repo's default branch and its own branch. Avoid: target branch (**target** is not a review subject).

**PR author**: Who opened the PR, shown in the title bar as a round picture beside the merge route. A PR whose author's account is gone has none, and a local review never does. Avoid: owner, opener.

**Issue card**: What a hover of the issue button, or of a row of its list, shows: for a GitHub issue its title, state (open, closed as completed or as not planned), author, labels, assignees and description cut to a set height; for a ticket only its link. The viewer reads the details of the shown issues from GitHub in one trip, again on window focus and whenever the shown issues change, and keeps the last answer when a read fails.

**PR description**: The PR's own body on GitHub, shown under the summary. The viewer keeps the last one it read; it is not part of the tour.

**Step**: One stop in a tour. It holds diff pieces, a diagram, or both, and a short text that says what they do and why they come at this point. A step can have child steps: the details of that step. The reader sees a step first, then its child steps.

**Check hint**: A one-line note on a risky step that tells the reviewer what to verify.

**Skim step**: A step the reviewer only needs to glance at, for example callers that only pass a flag.

**Diff piece**: A part of a PR diff that a step can point to. Usually one changed symbol: a method, function or class. Some pieces carry a mark: `format` (only layout changed), `moved` (same code, new place), `generated` (made by a tool).

**Piece id**: The name of a diff piece: `<file path>#<symbol path>`, or just the file path for a whole-file piece. A `hunk-N` symbol stands for a file that could not be cut into symbols. A state file writes a whole file whose name holds a `#` with a trailing `#` (`docs/C#.md#`), so it is not read as a symbol.

**File block**: The one box a page shows for all its diff pieces of one file, in line order. A thin label marks where each piece starts: at the first line of its symbol, not at its first changed line. Blank lines and lines with only closing brackets never move a label.

**Rendered view**: A file block showing its file as it reads instead of its diff lines: a Markdown file formatted, each top-level block (heading, paragraph, list, fence) with a bar for added, changed or removed, removed blocks dimmed where they were; an SVG as its before and after pictures. The Code / Rendered switch in the box header changes it, per file. Markdown starts as code, an SVG rendered; a raster picture has no code, so it shows its pictures and offers no switch. A rendered box takes no line comments; the threads on its lines are listed after the file. The choice is not saved. Avoid: preview, rich diff.

**File path**: A file's path in the repo, relative to its root, with `/` separators. A renamed file is named by its new path on both sides.

**Outline**: A short list of all diff pieces in a PR, with no code. A tour writer reads it first and opens only the pieces it needs.

**Tour file**: The saved form of a tour. Any viewer can show it.

**Viewer**: A thing that shows a tour file to the reviewer and tracks which steps were read.

**Read tick**: A mark that says the reviewer has read a step, or in a basic tour a whole changed file. The viewer owns it. A step can get a read tick or lose it when the reviewer marks its files as viewed, or not viewed, on GitHub. On the switch to a guided tour, a step whose files were all read starts with a read tick.

**Safety net**: The last screen of a tour. It shows every diff piece no step points to, so nothing in the PR goes unseen. It is computed from the tour and the diff, never stored.

**Screen**: What the viewer shows now: in a guided tour the summary, one step, or the safety net; in a basic tour the overview or one changed file.

**Page**: The content of one screen, laid out for reading: the summary page holds the summary, the route and the PR description; a step page holds the step's text, its pieces and their threads; the safety net page holds the uncovered pieces. Under a line it shows, in this order: threads, queued comments, comment boxes and the ask marker of lines with Asks.

**Code link**: A link to a file or a line of the PR's code: a diff line, a piece, or a link in the tour's text. The viewer opens it in the reviewer's IDE when the file is in the working copy of a local clone, else on GitHub, and says why. When the clone is not checked out clean at the tour's head, it still opens in the IDE, with a note that the lines may differ. Removed lines and deleted files always open on GitHub, at base.

**Re-pin**: Moving a tour to the PR's new head after new commits, or a local review's tour to a new snapshot, with no AI. Each diff piece becomes same, changed, gone or new. Steps with changed pieces lose their read tick. In a basic tour, files with changed or new pieces lose their read tick.

**Refresh**: An AI pass on a re-pinned tour. It rewrites only the steps whose pieces changed and places new pieces into steps.

**Ask**: A question to the AI about one place in a tour (selected lines, a step, or the whole tour), with its answers and follow-ups. The viewer keeps it; it is not part of the tour.

**Test set**: A fixed list of real PRs used to score the tour writer. Every change to how tours are written runs on it.

**Reviewer note**: A short note per PR in the test set, written once by the reviewer. It says what a good tour of that PR must show: the main flow and the diagrams it needs.

**Last review**: The reviewer's latest sent review of the PR, as GitHub holds it: how it ended (approved, changes requested, commented or dismissed), its text, when, and the PR head it was sent on. A review sent from the GitHub web page counts too. The viewer reads it and does not keep it. While it exists the finish button reads "Update review", and it is named as old when the PR head has moved past its commit.

**Standing**: Where one reviewer is on the PR, as GitHub counts it: their latest approval or change request, which a later comment does not undo, else their latest review (commented or dismissed), else asked to review and waiting. The PR author answering threads is not a reviewer. The sidebar's reviews lead with the reviewer's own standing, in the colour of that state, then everyone else's: change requests, approvals, comments, then who is waited on; a review on a commit the PR has moved past is marked older. Avoid: review state, verdict (that is what the reviewer sends).

**Mark done**: Ending a re-review with nothing to send: the reviewer's last review saw an older commit, and no pending review waits. It is saved in the state file at the tour's head, so new commits undo it; nothing goes to GitHub, and the reviewing mark comes off. Approve again, beside it, sends a fresh approval instead. Avoid: finish locally, dismiss.

**Tour agent**: The AI session, in a herdr terminal, that answers for the tour of one review subject. At most one per subject. The viewer finds it by a herdr label, `gr.pr=<url>` for a PR and `gr.local=<subject key>` for a local change set, never by a stored terminal id, and a terminal claimed for another subject loses the label. A local review's tour agent is its change set's agent.

**Agent link**: Whether the viewer can reach tour agents at all, which needs herdr (`hc`) on PATH and its server running. When it is off, the sidebar says why (`hc` missing, the herdr server not running, or herdr running but a command failed) and the tour agent features are off. The app looks again every 30 s and on Retry, and the features come back when herdr answers.

**Text box**: A box the reviewer writes text for GitHub in, or in a local review for the change set's agent. There are four kinds: a comment box (a new comment on lines or on a file, its text called a draft), a reply box under a thread, the edit of one of the reviewer's own comments, and the body of the review being finished. Each kind opens, takes typing, and is left, given up or sent in the same way. A box left with nothing written in it goes. The text of every kind but an edit is saved while typing, so it comes back after a restart. A comment box of a local review sends tasks (Add to tasks, Send now), never to GitHub. Avoid: composer.

**Ask agent**: The comment box's second send action. The comment or reply is posted to GitHub like any other and also handed off to the tour agent. A comment sent with the plain action is for the PR author only; the agent never sees it.

**Hand-off**: One reviewer comment sent with Ask agent, as passed to the tour agent, with where it is: queued, delivered, working, working on it (the agent posted "Working on it…" and its answer will replace that comment), answered, ended without an answer, failed, or held back because its agent does not report its state to herdr (the reviewer looks at the agent, since it may be waiting on a question, and chooses Send anyway, which types it in whatever the state). It holds the id of its GitHub thread and of the reviewer's comment. In a local review each task is a hand-off to the change set's agent, its thread a local one, with no seen mark: delivery is its state.

**Agent comment**: A comment whose text starts with 🤖, which a tour agent posted under its person's account. The app names it `<login>'s agent` in the accent colour, with the agent's mark as its picture and the person's picture small at its corner, and shows the text without the 🤖. On GitHub it stays as posted.

**Seen mark**: The 👀 the viewer puts on the reviewer's comment on GitHub when its hand-off is delivered to the tour agent. The agent's first reply takes it off; so does the viewer when the agent ends without answering. When GitHub refuses either, the line under the thread says so.

**Reviewing mark**: The 👀 the viewer puts on the PR itself while the reviewer reviews it, so its author sees someone is on it. It goes on when a review opens on a PR that is not the reviewer's own, unless the reviewer's last review was sent on the tour's head or they marked it done there (opened again only to look), and comes off once a review of the reviewer's is sent after that, from the app or from the GitHub web page, at Mark done, and when the review closes: the app quits (it waits up to 2 s for GitHub) or switches to another PR. When GitHub refuses either, the sidebar says so, and it is tried again when the window comes to the front. Avoid: eyes reaction, seen mark (that is the 👀 on a comment).

**PR notes**: The `Notes` section of the PR description. A step's diagram, a picture of the reviewer summary, or the whole reviewer summary can be added to it, each folded in a `<details>`, from the button under it; a description with no such section gets one at its end. Pictures are uploaded to GitHub first, where only those who can see the repository can open them; the summary's links to other files of the tour's folder become plain text. A hidden comment in the fold tells the app a picture or summary is there already.

**Shortcuts sheet**: The overlay that lists every key the app answers, by heading (Navigate, Read, Comment, View, App), opened with `?`. It reads the same table the keys are bound from. Avoid: cheat sheet, help.

**Primary modifier**: The key the app's own shortcuts are held with: Cmd (⌘) on macOS, Ctrl on Linux, where Super belongs to the desktop. `config.toml` writes it `primary-` (`primary-k`); a `cmd-` saved before reads as Primary everywhere, and on Linux so does `ctrl-`. A key is spelled in text and on caps as its OS does: `⌘K` on macOS, `Ctrl+K` on Linux. Avoid: Cmd key, command key, secondary.

**Text keys**: The keys a text box edits with that differ by OS: on macOS Option moves by word and Cmd by line, on Linux Ctrl moves by word and Home/End by line. They are not the reviewer's to change.

**Window buttons**: The OS's close, minimise and zoom buttons. On macOS they are the traffic lights, drawn on the app's own title bar, which keeps room free left of them (the button inset). On Linux the app draws none and keeps no room; the window manager decides the decorations. Avoid: traffic lights (macOS only), window controls.

**Palette action**: Something the Primary+K palette does rather than a place it goes to, such as Settings or Mark step as read. A query that starts with `>` lists only actions.

**Gap row**: The one row of a file box that stands for unchanged lines it does not show, between two changes or at either end, with how many and the buttons that show them.

**Change**: The changed lines of a file box with the unchanged lines around them, as a file box shows them between its gap rows (GitHub's hunk). One change of the screen can be focused: a thin accent bar marks it and the change keys act on it. The viewer keeps the focus as a display option, not saved and not part of the tour, named by the piece and the number of the change, the same name a request for more lines uses. Tab and Shift+Tab move it, a click sets it, and it moves to the first change in view when the focused one scrolls away; a screen with no code has none. Avoid: hunk (in text the reviewer reads), section, block.

**Release install**: The app as the README installs a release: each version in its own folder under `~/.local/share/guided-review/<version>/`, with `~/.local/bin/guided-review` (and usually `~/.claude/skills/tour`) linked into the newest. Only a release install updates itself. Avoid: managed install.

**Desktop entry**: What the OS launcher starts the app from, put in place from the release install by `guided-review install-app` and kept current by each update. It starts the release install's bin link, marks the run as its launch, and that run starts itself again through the reviewer's login shell, so `gh`, `claude` and `hc` are found. Launched with no PR, it shows the PR switcher alone in the window. On macOS it is the App bundle; on Linux it is `~/.local/share/applications/io.github.ekvedaras.guided-review.desktop` with its icon in `~/.local/share/icons/hicolor/`, and the app's window carries the same app id. Avoid: launcher, shortcut, wrapper.

**App bundle**: The Desktop entry on macOS: `~/Applications/Guided Review.app`, what the Dock, Spotlight and Raycast start. Its binary is a link to the release install's bin link. Avoid: launcher, wrapper.

**App folders**: Where the app keeps what it writes for the reviewer, by the XDG layout on every OS: the config folder (`config.toml`, `summary.md`; `~/.config/guided-review`), the state folder (tours, state files, Asks, evals, the app lock, socket and log; `~/.local/state/guided-review`) and the cache folder (image previews; `~/.cache/guided-review`). There is no runtime folder since 2026-11-14 (#75 review): the socket sits beside the lock, so a CLI started with another `XDG_RUNTIME_DIR` still finds the app. Until 2026-11-14 all were `~/.guided-review`, which the first start moves into them once; since the #75 review only a start of the release install, or of any build on a machine with none, moves it, file by file, never writing over what the app folders already hold. The release install's folders are not app folders. Avoid: guided-review home, data dir.

**In the way**: Of the release install, while it runs: another `guided-review` that comes before its bin link on `PATH`, so the tour skill and a terminal run that one and miss updates. Typically a source build in `~/.cargo/bin`. The sidebar lists it as a problem naming the binary. Avoid: shadowing.

**Build**: Which app runs: the release install (no badge; it alone checks for updates, also from the palette's Check for updates), a local build such as `cargo install` (badge `DEV`), or the snapshot build (badge `TEST`). The badge is beside the app's name, after every window title ("Settings (DEV)"), and on a local build's Dock icon; all three ask the one build. A local build running is not a problem; its badge says it. Avoid: dev mode, debug build (a `cargo install` build is a release build, and still `DEV`).

**Update**: A published release newer than the running app, built for this OS and CPU, that the app offers in the sidebar, above Settings. One click downloads it, checks it, unpacks it beside the old version, moves the links to it and restarts the app. Avoid: upgrade.

**Toast**: A short note at the bottom right of the window about an outcome that is otherwise easy to miss. It leaves by itself unless it tells of a failure. Avoid: notification, snackbar.

**Presence**: Whether an overlay (palette, PR switcher, Settings, shortcuts sheet, Ask panel, zoomed picture) is on screen, and whether it is leaving. A leaving overlay is still drawn, plays the reverse of its entrance and takes no input; a reopening, or a close during the entrance, continues from where the move had got to; once the move has played it is dropped. Closing under reduced motion drops it at once, and opening one that is leaving brings it back. Avoid: fade-out state, closing flag.

**GitHub outage**: GitHub not answering reads that are fine: a server error (5xx), a timeout or no connection. A refused token or a missing page (4xx) is not one, as the reviewer can put those right and trying again changes nothing. While it lasts the sidebar holds one problem for all the reads it keeps from going through, with what may be old under it, and the reads are tried again by themselves, 30 s, 1 min, 2 min, up to every 5 min, besides when the window comes to the front. It ends when each of those reads goes through. Avoid: network error, downtime.

**PR status**: What the title bar says of the PR besides its number. While the PR is open: whether it is a draft or ready for review (the chip's words), and its checks: the rolled-up state (none, running, passing or failing) as the chip's tint and icon, and each check in GitHub's words (failing, running, passing, neutral or skipped), failing first, in a list a hover opens, where a click opens that check's run on GitHub. A skipped or neutral check does not fail the PR; the roll-up is GitHub's. Once the PR is merged or closed, the status is only that: a Merged chip in GitHub's merged colour or a Closed one in red, with no merge button, and `D` and `Shift+M` do nothing. Read from GitHub, kept old while GitHub does not answer, and read again every 30 s while a check is running. The key `D` turns a draft ready or a ready PR into a draft, when the reviewer may change it, and only the same press again within 10 s does it. Avoid: CI badge.

**Auto-merge**: GitHub merging the PR by itself once nothing blocks it (required checks, reviews, rules), with the merge method it was turned on with. It applies only while something blocks the merge, and only where the repo allows it. The key `Shift+M` and the title bar's button turn it on or off, where GitHub says the reviewer may, and only the same press again within 10 s does it. Avoid: auto merge request, merge queue (a repo's queue is GitHub's; auto-merge on such a repo adds the PR to it).

**Merge method**: How a merge lands on the base branch: a merge commit, a squash or a rebase. The reviewer's choice is a setting, `merge_method` (a merge commit when not set), which `[merge_methods]` can change per repo; when the repo does not allow it, its first allowed method is used, and the toast that asks for the second press names the one used. Avoid: merge strategy, merge type.

**Merge now**: What the auto-merge key and button offer instead when nothing blocks the PR, so GitHub would refuse auto-merge: a merge at once with the merge method, confirmed the same way. Avoid: force merge, admin merge.

**AI review**: A code review of a whole review subject that the app asks a review agent to do, when the reviewer wants it. Its result is the findings file, shown in the sidebar and, in later steps, on the lines. Avoid: auto review, bot review.

**Finding**: One thing an AI review reports, before the reviewer acts on it: an id, a severity, a Markdown body and an anchor (lines of either side of one changed file, a whole changed file, or the whole change). Avoid: AI comment, suggestion (a GitHub suggestion is a code change in a comment), issue.

**Findings list**: The sheet that lists every open finding as one compact row (severity, `file:line`, first line), to tick, open and decide in a batch: Dismiss, or Comment (each as is, marked 🤖, into the pending review or the tasks of a local review). Avoid: findings panel.

**Severity**: How much a finding matters: `must fix`, `should fix` or `nit`, most severe first. Avoid: priority, level.

**Use / Use as is / Dismiss**: What the reviewer does with one finding. Use puts its text in a comment box on its lines (a task in a local review) to edit and send; Use as is sends it at once to Add to review (Add to tasks), starting with 🤖; Dismiss drops it, and the next AI review is told not to report it again. A decided finding is kept in the review's state file. A dismissed one stays under Dismissed in the Findings list, kept with its text even when a later run's file no longer holds it, and Restore opens it again, so the next run is no longer told of it; a used or fixed one is not shown again. The comment box, queued comment and thread made from a used finding carry it: the thread's header shows AI and its severity, with its fix request and Ask why talk, kept in the state file, so the link outlives a reopen and a re-pin.

**Findings file**: What the review agent writes beside the tour (`<number>.findings.json`, the path `guided-review findings-path <key>` prints): the tour's pin (`pr` or `local`, `base`, `head`) and `findings`. `check-findings` checks it and pins it, `findings-done` tells the app, which reads the file whole, never half of it. The file is the state: an app that is not running reads it when it opens the review. Avoid: report, review file.

**Review agent**: The AI session, in its own herdr terminal and apart from the tour agent, that does one review subject's AI review. Avoid: reviewer (that is the person), review subagent.

**Review worktree**: The throwaway checkout of the reviewed head (a PR's pinned head, a local review's snapshot commit) that the review agent works in, outside the clone. It is made when the run starts and removed when the agent ends; the app sweeps leftovers at start. Avoid: sandbox, scratch checkout.

**Run (AI review)**: One go of the review agent, from Run AI review to `findings-done`: not run, starting, running, or failed (it went idle, errored or ended without `findings-done`). Run again and Try again start a new run that replaces the agent and the worktree of the old one. Avoid: job, session.

**Ask why**: The reviewer's question about one finding. While the review agent lives, it goes onto the finding's thread (kept in the review's state file, for a PR too) and the review agent answers with `guided-review agent-reply <key> <finding id>`; once it is gone, the question opens in the Ask panel on the finding's lines, with the finding told. Avoid: explain, challenge.

**Outdated finding**: A finding whose lines new commits changed after the review agent wrote it. A re-pin moves every finding with its lines, like a local thread, and pins the findings file to the new head; an outdated one is kept, marked Outdated, with what its lines said, since it may already be fixed. Its decisions, fix requests and questions follow it. Avoid: stale finding.

**Fix request**: A finding the reviewer sent to the tour agent to fix in code, alone or in a batch with others. Fix now sends one finding at once; Add to fixes keeps it, marked "To fix", until Send fixes in the sidebar sends every one added as one batch. Fix with note adds what the reviewer wants of the fix: Enter adds it to fixes, the fix-now key sends it at once; the note goes to the tour agent with the finding and shows on its thread. Dismissing or using a finding takes it out of the fixes to send. While the tour agent works on a fix, the finding cannot be used or dismissed; once it asks the reviewer a question, it can again, which drops the question. A batch goes as one message in reading order; the tour agent starts one subagent, makes one commit per finding, and answers each with `guided-review fix-reply` (`--working`, `--fixed <commit> --body` with what it did, or `--not-fixed --body`). When it needs a decision first it asks with `--question --body`: the question shows on the finding's thread, the finding waits for the reviewer, and the reviewer's answer is typed to the tour agent as the next message. A fixed finding stays open, with the commit and what the agent did on its thread, until the reviewer picks Mark fixed, which closes it like a dismissed one, or asks for more with Fix now or Fix with note; its commit is named in the sidebar. A not fixed one stays, with the reason. Avoid: task (that is a local review's comment), auto-fix.
