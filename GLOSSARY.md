# Glossary

**Tour**: A guided review of one PR. It shows the changes in top-down order: main flow first, details later.

**Summary**: The short text at the start of a tour. At most three bullets that say what the PR does.

**PR description**: The PR's own body on GitHub, shown under the summary. The viewer keeps the last one it read; it is not part of the tour.

**Step**: One stop in a tour. It holds diff pieces, a diagram, or both, and a short text that says what they do and why they come at this point. A step can have child steps: the details of that step. The reader sees a step first, then its child steps.

**Check hint**: A one-line note on a risky step that tells the reviewer what to verify.

**Skim step**: A step the reviewer only needs to glance at, for example callers that only pass a flag.

**Diff piece**: A part of a PR diff that a step can point to. Usually one changed symbol: a method, function or class. Some pieces carry a mark: `format` (only layout changed), `moved` (same code, new place), `generated` (made by a tool).

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
