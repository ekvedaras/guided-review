# deslop-please

[deslop-please](https://github.com/deterministic-machines/deslop-please) lints module structure: module cycles, tangles (items that know each other in a circle), fields used outside their type, modules that show most of their functions, directories over 15 entries and files over 2000 lines. How to run it and how to fix an error is in [AGENTS.md](../../AGENTS.md).

## Decisions

### The linter is in the gate (2026-11-25)

- `scripts/deslop` pins the version (0.1.4) and installs it when missing. `scripts/check` runs it after clippy, in CI too.
- The linter reads the index `rust-analyzer scip` writes, so `rust-toolchain.toml` installs `rust-analyzer` and `rust-src`, and the index matches the pinned Rust.
- On its first run it found 160 errors on macOS (13 module cycles, 13 tangles, 121 field leaks, 4 shallow modules, 5 wide directories, 4 long files). They went into allow lists unfixed, so the gate fails only on new errors.

### It checks all features

One run checks one configuration, and `scripts/deslop` uses `--all-features`, the one the tests build. The default features gave 162 errors, mostly the same ones. Their circles are smaller (the `snapshots` code is gone) and 5 field leaks show only there. Running both would double the time for those 5.

### One allow list per target triple

`.deslop-please/<target>.allow` has one file per target, since code behind a `cfg` differs per OS. Linux has `shallow-module guided_review::platform` that macOS does not, because `src/platform` holds other code there. A single list would tell a Mac run that the Linux entry is fixed, and following its tip to update the list would then break Linux CI. `scripts/deslop --update-allow` rewrites every list from a Mac with `--target`.

### The gate warns about added allow-list lines (2026-11-25)

`scripts/deslop-added <base>` lists the lines the allow lists on disk have that they did not have at the fork point with the base. A list that did not exist there is named with its count. `scripts/check` runs it after `scripts/deslop`, outside CI, against the same base as the change notes. It warns and never fails, because a rename may need a new key.

### How the 160 errors were worked down (2026-11-25)

Four waves, each behaviour-preserving and through the full gate, cut the lists from 160 to 100 entries. Each was fixed from structure, not by moving a count (the commits `6f8bf7ef`, `7798bb32`, `a1c766bc`, `4ae77c74` hold the details). The rules that came out of them:

- **A value type sits below the code that acts on it.** A line's place, a screen, a thread as GitHub has it and a comment's place are written below the modules that use them. Most of the first 71-module cycle in `viewer` was value types written in the module of their busiest user.
- **A shared vocabulary leaves the module of its user.** `keys`, `more_lines` and `merge_method` are below `viewer`, because `config` reads them and `viewer` reads `config`. The config reads the instructions it holds, not the other way round.
- **A type answers the question; callers do not read its fields.** A field leak is fixed by moving the code into the type whose fields it read, and the type then says what happened (`SyncOutcome::land`, `CommentRequest::leave`, `TourPin::move_to`, `Standing::status`). Every fix of this kind kept behaviour.
- **A file is split by what it draws or knows, with children calling each other one way.** `one_step.rs`, `review.rs` and `settings_panel.rs` were split into files per drawn part, per topic and per Settings page. What two children both need stays in the parent file, because moving it made a cycle.
- **The kept page's rule.** `Review` (`src/viewer/review/kept.rs`) is `ReviewCore` plus its page; logic that changes the review is `impl ReviewCore`, so it does not drop the page through `DerefMut` before it knows it changes anything. `Review` keeps only what is about the page (see [deepening pass](deepening-pass.md)).
- **A lint hit that is a pointer event, a hand-over or a value drawn is not fixed by renaming.** The lint reads `mem::take` as a getter and a setter as a leak. Where the method is the event (`hover`, `select`, `code_changed_is`) or the value is what the app draws, a forwarding method would only hide the entry.
- **Tried and undone:** drawing `InlineText` as its own method made a 14-item tangle through the link click, `code_links`, the session and the prepared markdown, so prepared text stays below the drawing. `SavedState::thread_home` could not replace `SavedState::file`, because `ReviewCore::state_file` hands the same file to the app's session for background work.
- **A file moved only to dodge a count is not a fix.** deslop-please judges `shallow-module` only for modules with child modules, so splitting a file into a child module can create a new shallow-module error. This is why `review_session.rs` stays whole.

## What is left, and why (2026-11-27)

Counted on 2026-11-27: 89 entries on macOS and 90 on Linux (the `platform` entry). 3 module cycles, 5 tangles, 71 field leaks, 4 shallow modules, 5 wide directories and 1 long file. The Ask panel's entries left with the panel (see [agent threads](agent-threads.md)).

### Module cycles and tangles

- `viewer::{review,page,replacing_jobs,focused_change,screen_fold}` and the tangles of `Page`/`ReviewCore` and of `AfterJob`/`JobOutcome`/`RepinnedTour`/`Review`. `Review` keeps its `Page` and `page` builds it from `ReviewCore`. `ReviewCore`'s methods are written across `review/*`, so taking it out of `review` means moving most of `review.rs`. A job's outcome carries the next `Review` (`RepinnedTour`), and `Review` lands its jobs: that circle is the data's own. `focused_change` and `screen_fold` hold both page data (`PageChange`) and `impl Review` actions; a split needs `PageChange`'s fields seen outside it, which only trades a cycle for field leaks.
- `viewer::{hand_offs,local_thread_store,review_state}` and its tangle. `HandOffs` lives in the state file and builds `AgentReplyRequest`s that write through the local thread store, which locks that file. `AgentReplyRequest` holds a `ThreadHome`, which holds a store of the state file that holds the hand-offs. Breaking it means `ThreadHome` naming its store without holding it, a change to how agent replies reach a local review.
- `app` (a module cycle) and the tangle through `app::apply_keymap`. `app::components` holds both the leaf widgets (icon, palette, toast stack) the app's helper modules draw with and the composite panels (Settings, PR switcher, finish card) that read the session. Breaking it means splitting `components` in two, a move of most of its entries in gpui code with no window tests.
- `app::components::palette::Appearance` and `app::snapshots::appearance`: the snapshot harness stages the appearance, read behind `cfg(feature = "snapshots")`. Inverting it needs a staged global set before the first read, which only a snapshot run proves.

### Shallow modules

- `viewer::review`: `ReviewCore` is the one interface the app has to the viewer, one drawn fact or one action at a time. Splitting its file changes no count, since the lint judges the module with its children. Deepening it means the app asking for something wider (a view of a screen region, more actions through `decide(Intent)`), a change of the app/viewer interface in gpui code with no window tests.
- `viewer::review_state`: `ReviewState` is the state file's record, changed only through the `SavedState` gate. Its functions are the record's reads and writes that `ReviewCore` decides. Moving those decisions into the record would put review rules into the persisted shape. The lint judges this module only because of its inline `pull_request_links` serde module; removing that to pass would only hide the error.
- `app::components::palette`: `Colors` is the design tokens of [visual design](visual-design.md). Each call site picks one named shadow, fill or text colour, and fewer functions would only move those names into an enum argument.
- `keys`: the vocabulary config, viewer and app share (chord, key, command table, keymap, send actions), used as values. Moving the `[keys]` TOML read and write from `viewer::settings` into `KeyChanges` would replace two shown functions with two new ones and stay shallow (estimated from the call sites, not run).

### Field leaks

- **Values the app draws:** `Colors` (12), `Snippet`, `YourReview::avatar_url`, `CloneOffer::state`, `NameHover::open`, `PageRows::description_hidden`, `InlineText::text` and `::links`, `PreparedMarkdown::blocks`. Each is the viewer's or the prepared rows' answer of what to draw, read as a value. A method per drawn fact would move drawing decisions into the viewer, or forward to the one function that draws them.
- **`EditState` (5 fields left):** the text box's state across frames, shared by `TextBox` and `TextBoxElement` in one file. gpui's `Element` trait splits that code by layout phase, not by data. Moving `request_layout` and `prepaint` into `EditState` is a rewrite of a gpui element with no window tests.
- **Events and hand-overs the lint reads as getters or setters:** `PrSwitcher::hovered` and `::selected`, `DiagramMemory::pending_svgs`, `ReviewState::typed` and `Saves::typing_is_unsaved` (each `mem::take`s work to hand it over), `SavedState::state` (the save gate's closure), `PressAgain::to_send` (set only by a snapshot scene), `NameHover::pointer` and `ActivePr::replies_for_reopened`.
- **`ReviewCore`:** `screen` and `subject` are read as values by every view, and the lint counts a method on a borrowed value (`==`, `clone`) as a leak. `display_options` is a deep module the app works with, left outside the save gate on purpose. Forwarding its interface through `ReviewCore` would widen the widest type in the crate and hide nothing. `guided_tour` is the interface through which the app tells the review the tour agent's progress. `ai_review_run`, `pr_origin` and `steps_started_read` are set only by snapshot scenes. `working_copy` has one setter, from `ReviewBuilder`, for a fact it reads from disk.
- **`ReviewSession` (5):** the session owns the `Review`, its rows, the sidebar rows, the clone offer and the old head, and the layout reads the review and the rows in many places. A method per drawn fact would move the layout's reading onto the session, not narrow it.
- **`ActivePr::shown` and `::switching`:** the subject is read as a value to name it (Back to, Opening).
- **`TourWriting::screen`:** `screen_is` is the event "the agent's screen shows these lines", from `ActivePr::writer_screen` and a scene.
- **`ReleaseInstall::desktop_entry`:** the install hands out its OS adapter (`dyn DesktopEntry`), which `launch`, `install_app`, `start_app` and the updater use as a port. Forwarding six port methods through the install would widen it and hide nothing.
- **`OpenedPr::folders` and `OpenedChangeSet::folders`:** `OpenedSubject` is one enum over the two adapters; it reads each adapter's app folders to do what both share (the state file, the shown tour). Moving that into each adapter writes it twice.
- **`Signature::text` and `::origin`:** `Signature` builds a declaration line and where its bytes came from; `Definition` stores them as two arrays. A `finish` returning both would be the same read under another name.
- **`DisplayOptions` (3), `Page`, `PageChange`, `FileBlock`, `CommentDesk::selection`, `SavedState::file`:** the Page/ReviewCore knot and the reasons above stand; the display choices were left outside the save gate on purpose.

### Wide directories and the long file

- `src/`, `src/app/`, `src/app/components/`, `src/viewer/` and `tests/it/` are over 15 entries. A folder per group is a module per group, and deslop-please checks cycles among siblings: `viewer`'s modules still form the cycles above, so a split of its files into folders puts those cycles between the new folders. Grouping waits for the cycles. `tests/it/` is one module per topic (AGENTS.md), and its topics have no grouping that is not by the `src` module they test, which would follow the same split.
- `long-file src/app/review_session.rs`. The one seam that takes it under 2,000 lines is the snapshot staging (all behind `snapshots`), but a `review_session/staging.rs` makes the lint judge `ReviewSession` as a shallow module, a new error. Narrowing what the layout asks `ReviewSession` comes first.

## Known flake

`move_to_pr::a_failed_look_up_of_the_head_branch_is_tried_again_later_and_later_but_not_on_focus` failed once on 2026-11-25 in a loaded full test run and passed alone three times and in the gate. It times its retries from `Instant::now()` taken before the PR opens.
