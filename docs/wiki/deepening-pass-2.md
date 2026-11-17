# Second deepening pass

2026-11-17. Commits `ca8ee8d` to `52291ed` on the `deepen/after-local-review` branch.

An architecture review of what grew after the [first pass](deepening-pass.md) (local review, PR status, auto-merge, Move to PR) found rules spread over several builders, the session and `ReviewCore`. Five candidates moved each into one module, one at a time, tested through its interface. Three of them found bugs that came from the spread itself: a fact set in one builder and forgotten in the others.

## The five candidates

1. **`ReviewBuilder`** (`ca8ee8d`, `src/viewer/review_builder.rs`). Open, Update, reload, Refresh and the switch to guided build the `Review` in one place, with every fact from config and disk: more lines, the merge method, the working copy, a local change set's facts and repo, `LocalToursNotRead`. `carry_on_from` keeps only session state. Before, only the open applied config, so a rebuilt review lost the merge method and more lines, and a local review opened by its key lost its repo and was never offered Move to PR.
2. **`ReplacingJobs`** (`789f888`, `src/viewer/replacing_jobs.rs`). Update, Refresh, reload, the switch, Move to PR and Drop review are one value the review owns: `start_job` is the one single-flight rule, `land_job` says what follows (`AfterJob`), `JobProgress` feeds the banners. The session lost six job fields and seven separate checks. Before, a reload, Refresh or switch could swap in an unguarded review while Update ran. The clone offer stays outside on purpose: it replaces nothing, and a long clone must not block Update.
3. **`BranchLink`** (`2f66af9`, `src/viewer/branch_link.rs`). What a review knows of the other side of Move to PR: a local review's PR, or a PR's local reviews and head branch, the look-up backoff and the declines. It answers `branch_offer()`. A PR review's Keep writes the local review's file through `LocalReviewsOfRepo::decline`, not by reaching around the `SavedState` gate.
4. **`PrControls`** (`b9ed261`, `src/viewer/pr_controls.rs`). A PR review's status, merge method and the two press-again changes (`D`, `Shift+M`), `Some` only for a PR. One `PressAgain<T>` holds the rule for both keys. Before, a rebuilt review lost the status chip and the merge button until GitHub answered again.
5. **`OpenedSubject`** (`52291ed`, `src/opened_subject/`). The CLI and the app ask one opened subject for its code, code at a pin, basic tour, repin and GitHub port; the PR and local adapters are a private enum, so callers cannot match on the kind. An enum, not a trait: the set is closed at two and no test fakes an adapter. `LocalWorkTree::snapshot` is gone, so only the local adapter moves a snapshot ref, inside the pin lock.

Looked at and not done: splitting `one_step.rs` (wide, not shallow: drawing, with decisions already behind `decide`), and `PageRows` (its own `Arc<Page>` is load-bearing: the list's row numbers and heights belong to the page it was spliced from).

## Open behaviour questions

Each candidate kept the behaviour it found, except the bugs above. Lettered on from the first pass.

- (h) A refused job says nothing: Refresh clicked during Update does nothing.
- (i) A job's failure lasts until that job starts again, across a swap: a failed Update keeps Try again after a reload.
- (j) A clone runs beside the jobs; the reload it asks for waits for them.
- (k) The switch to guided writes step ticks off-thread while the basic review can still save; the hand-over heals it, but an app that ends between the two loses those step ticks.
- (l) Reload, the switch and Drop now save typing before they start.
- (m) A failed Keep local review: the local side hides the offer anyway, the PR side keeps it for another try.
- (n) A rebuilt PR review keeps the old branch link, so a local review of its branch started after the open is seen only on the next open.
- (o) A local review kept away from its PR offers neither Move to PR nor Open PR until GitHub stops listing the PR.
- (p) Keep from a PR review writes the local review's file; a window showing that local review offers the move until it reopens.
- (q) A first `Shift+M` before a rebuild whose config names another merge method asks again after it.
- (r) A carried first press keeps its time, so the 10 s count from before the rebuild.
- (s) A rebuilt PR review reads its status from GitHub again though it carries the old one.
- (t) **Likely bug.** With a basic and a guided tour on disk, the CLI's `repin` moves a PR's guided tour but a local review's basic tour. The local guided tour then stays on a snapshot no ref keeps, which `git gc` may take before the switch or a refresh reads it.
- (u) An `agent-reply` written to a local thread after `PrMover::carry` read the threads and before the local state file is deleted is lost. The pin lock would not close it.
- (v) The app's open no longer reads `origin` again for a local subject that carries its repo.
- (w) The app's Update lets a missing tour fail after the GitHub call; the CLI's PR `repin` checks first.
- From candidate 1: a rebuild now reads `config.toml` again, so a broken config fails Update; display choices other than more lines (hidden whitespace, rendered view) are neither built nor carried, which was not checked.
