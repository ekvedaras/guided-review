# Glossary

**Tour**: A guided review of one PR. It shows the changes in top-down order: main flow first, details later.

**Summary**: The short text at the start of a tour. At most three bullets that say what the PR does.

**Reviewer summary**: An extra summary at the start of a tour, written to the reviewer's own summary instructions. It is saved in the tour file next to the summary (`reviewer_summary`, Markdown, no length limit). Avoid: custom summary, extra summary, brief.

**Summary instructions**: What tells the tour writer how to write the reviewer summary: a file, a skill (`summary_instructions = { skill = "<name>" }`) or off, set in `config.toml` as `summary_instructions`. With no key, `~/.guided-review/summary.md` is used when it exists. `guided-review outline` hands the text to the writer. Avoid: prompt, template.

**Settings**: Where a reviewer edits their settings in the app: summary instructions, clones, IDE, Ask model and Ask effort. Save checks every setting first and writes nothing while one is wrong; it writes only the keys that changed back to `~/.guided-review/config.toml`, so the reviewer's comments stay. A custom IDE template or Ask model is kept when a preset is picked, in `ide_custom` and `ask_model_custom` beside `ide` and `ask_model`; only Settings reads those two keys. Avoid: settings screen (**Screen** means summary, step or safety net), preferences.

**PR description**: The PR's own body on GitHub, shown under the summary. The viewer keeps the last one it read; it is not part of the tour.

**Step**: One stop in a tour. It holds diff pieces, a diagram, or both, and a short text that says what they do and why they come at this point. A step can have child steps: the details of that step. The reader sees a step first, then its child steps.

**Check hint**: A one-line note on a risky step that tells the reviewer what to verify.

**Skim step**: A step the reviewer only needs to glance at, for example callers that only pass a flag.

**Diff piece**: A part of a PR diff that a step can point to. Usually one changed symbol: a method, function or class. Some pieces carry a mark: `format` (only layout changed), `moved` (same code, new place), `generated` (made by a tool).

**Piece id**: The name of a diff piece: `<file path>#<symbol path>`, or just the file path for a whole-file piece. A `hunk-N` symbol stands for a file that could not be cut into symbols. A state file writes a whole file whose name holds a `#` with a trailing `#` (`docs/C#.md#`), so it is not read as a symbol.

**File block**: The one box a page shows for all its diff pieces of one file, in line order. A thin label marks where each piece starts: at the first line of its symbol, not at its first changed line. Blank lines and lines with only closing brackets never move a label.

**File path**: A file's path in the repo, relative to its root, with `/` separators. A renamed file is named by its new path on both sides.

**Outline**: A short list of all diff pieces in a PR, with no code. A tour writer reads it first and opens only the pieces it needs.

**Tour file**: The saved form of a tour. Any viewer can show it.

**Viewer**: A thing that shows a tour file to the reviewer and tracks which steps were read.

**Read tick**: A mark that says the reviewer has read a step. The viewer owns it. A step can get a read tick or lose it when the reviewer marks its files as viewed, or not viewed, on GitHub.

**Safety net**: The last screen of a tour. It shows every diff piece no step points to, so nothing in the PR goes unseen. It is computed from the tour and the diff, never stored.

**Screen**: What the viewer shows now: the summary, one step, or the safety net.

**Page**: The content of one screen, laid out for reading: a step page holds the step's text, its pieces and their threads; the safety net page holds the uncovered pieces.

**Code link**: A link to a file or a line of the PR's code: a diff line, a piece, or a link in the tour's text. The viewer opens it in the reviewer's IDE when the file is in the working copy of a local clone, else on GitHub, and says why. When the clone is not checked out clean at the tour's head, it still opens in the IDE, with a note that the lines may differ. Removed lines and deleted files always open on GitHub, at base.

**Re-pin**: Moving a tour to the PR's new head after new commits, with no AI. Each diff piece becomes same, changed, gone or new. Steps with changed pieces lose their read tick.

**Refresh**: An AI pass on a re-pinned tour. It rewrites only the steps whose pieces changed and places new pieces into steps.

**Ask**: A question to the AI about one place in a tour (selected lines, a step, or the whole tour), with its answers and follow-ups. The viewer keeps it; it is not part of the tour.

**Test set**: A fixed list of real PRs used to score the tour writer. Every change to how tours are written runs on it.

**Reviewer note**: A short note per PR in the test set, written once by the reviewer. It says what a good tour of that PR must show: the main flow and the diagrams it needs.

**Last review**: The reviewer's latest sent review of the PR, as GitHub holds it: how it ended (approved, changes requested, commented or dismissed), its text, when, and the PR head it was sent on. A review sent from the GitHub web page counts too. The viewer reads it and does not keep it. While it exists the finish button reads "Update review", and it is named as old when the PR head has moved past its commit.

**Tour agent**: The AI session, in a herdr terminal, that answers for the tour of one PR. At most one per PR. The viewer finds it by the herdr label `gr.pr=<url>`, never by a stored terminal id, and a terminal claimed for another PR loses the label.

**Agent link**: Whether the viewer can reach tour agents at all, which needs herdr (`hc`) on PATH and its server running. When it is off, the sidebar says why and the tour agent features are off.

**Ask agent**: The comment box's second send action. The comment or reply is posted to GitHub like any other and also handed off to the tour agent. A comment sent with the plain action is for the PR author only; the agent never sees it.

**Hand-off**: One reviewer comment sent with Ask agent, as passed to the tour agent, with where it is: queued, delivered, working, working on it (the agent posted "Working on it…" and its answer will replace that comment), answered, ended without an answer, or failed. It holds the id of its GitHub thread and of the reviewer's comment.

**Seen mark**: The 👀 the viewer puts on the reviewer's comment on GitHub when its hand-off is delivered to the tour agent. The agent's first reply takes it off; so does the viewer when the agent ends without answering. When GitHub refuses either, the line under the thread says so.

**PR notes**: The `Notes` section of the PR description. A step's diagram can be added to it from the button under the diagram; a description with no such section gets one at its end.
