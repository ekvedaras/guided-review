# Glossary

**Tour**: A guided review of one PR. It shows the changes in top-down order: main flow first, details later.

**Summary**: The short text at the start of a tour. At most three bullets that say what the PR does.

**Step**: One stop in a tour. It holds diff pieces, a diagram, or both, and a short text that says what they do and why they come at this point. A step can have child steps: the details of that step. The reader sees a step first, then its child steps.

**Check hint**: A one-line note on a risky step that tells the reviewer what to verify.

**Skim step**: A step the reviewer only needs to glance at, for example callers that only pass a flag.

**Diff piece**: A part of a PR diff that a step can point to. Usually one changed symbol: a method, function or class. Some pieces carry a mark: `format` (only layout changed), `moved` (same code, new place), `generated` (made by a tool).

**Outline**: A short list of all diff pieces in a PR, with no code. A tour writer reads it first and opens only the pieces it needs.

**Tour file**: The saved form of a tour. Any viewer can show it.

**Viewer**: A thing that shows a tour file to the reviewer and tracks which steps were read.

**Read tick**: A mark that says the reviewer has read a step. The viewer owns it. A step can get a read tick or lose it when the reviewer marks its files as viewed, or not viewed, on GitHub.

**Re-pin**: Moving a tour to the PR's new head after new commits, with no AI. Each diff piece becomes same, changed, gone or new. Steps with changed pieces lose their read tick.

**Refresh**: An AI pass on a re-pinned tour. It rewrites only the steps whose pieces changed and places new pieces into steps.

**Ask**: A question to the AI about one place in a tour (selected lines, a step, or the whole tour), with its answers and follow-ups. The viewer keeps it; it is not part of the tour.

**Test set**: A fixed list of real PRs used to score the tour writer. Every change to how tours are written runs on it.

**Reviewer note**: A short note per PR in the test set, written once by the reviewer. It says what a good tour of that PR must show: the main flow and the diagrams it needs.
