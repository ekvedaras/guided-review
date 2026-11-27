# Basic mode

On 2026-11-16 the maintainer decided that a PR can be read in the app with no AI pass first, as a basic tour, and that a guided tour is asked for when the PR deserves it. The plan was the spec in GitHub issue #81; this page keeps the decisions and why. Reviewing local changes builds on it: [Local review](local-review.md).

## Decisions of 2026-11-16

- **A PR with no tour opens as a basic tour at once**, from the switcher, `guided-review open` and a Dock launch. The reviewer never waits on a loading screen for a story about three files.
- **The viewer never reloads or switches by itself when the guided tour is ready.** It holds that fact, shows a lasting banner, and switches only on the reviewer's click. Switching by itself would move the page under a reviewer who is mid-file, and a lost place costs more than a missed banner.
- **One screen per file, the sidebar a file tree like an IDE.** A basic tour has no steps to group files by, so the tree's order is the reading order.
- **A read tick covers a whole file, and ticks carry from basic to guided.** A step whose files were all read starts read, unless a piece changed since, so the guided tour does not ask for what was read.
- **The two kinds are separate files side by side.** The basic tour is `<number>.basic.json` with `"kind": "basic"` (`src/tour.rs`); the guided tour stays at the path `tour-path` prints, and a file there with no `kind` is guided. The tour writer, `check`, `refresh` and the `tour` skill only ever see the guided path, so none of them needed to learn about the basic kind, and an older build sees no tour rather than a fake guided one.
- **Which kind a PR shows is decided by what is on disk and what the viewer holds**, never by a stored flag. A guided tour never turns back into a basic one.

## In the code

- `tour::Tour` is `Basic(BasicTour)` or `Guided(GuidedTour)`; both hold one `TourPin`. A basic file that holds `title`, `summary`, `reviewer_summary` or `steps` is refused. `Tour::load` reads either kind; `GuidedTour::load` refuses a basic file. Code that reads steps or summaries takes `GuidedTour`, so the compiler lists every place that assumed steps exist.
- The switch (`viewer::TourSwitcher`) builds the guided review off the main thread while the basic review stays on screen and can still tick files. It also finds, with git, what the carry needs for the basic tour's head (`ReadCarrier`), so no git runs on the main thread.
- The hand-over is one take-over of the basic review's whole state, not one carry per kind: `ReviewState::take_over_from_basic` takes its viewer sections and typed text (file ticks, viewed marks, queued comments, hand-offs, drafts, the review body) as of the hand-over, then `Review::carry_on_from` carries the reads into the steps and saves. Before, the guided review saved the copy it loaded when the switch started and dropped whatever the reviewer queued, handed off or typed meanwhile.

## Kept clear for local review

The basic tour's code does not tie itself to GitHub: the basic tour maker takes a code source and tips and fetches nothing from GitHub (the PR title is an argument), read ticks are the source of truth with GitHub's viewed marks as an optional mirror, the new types hold no `PullRequestUrl` (the pin keeps `pr`), no store is keyed by PR URL, and re-pin of a basic tour makes no GitHub calls past resolving tips. [Local review](local-review.md) keeps the decisions built on this.
