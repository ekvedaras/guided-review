# deslop-please

## 2026-11-25: the linter joins the gate

- [deslop-please](https://github.com/deterministic-machines/deslop-please) 0.1.4 lints module structure: module cycles, tangles (items that know each other in a circle), fields used outside their type, modules that show most of their functions, directories over 15 entries, files over 2000 lines. It reads the index `rust-analyzer scip` writes, so `rust-toolchain.toml` now installs `rust-analyzer` and `rust-src`, and the index matches the pinned Rust.
- `scripts/deslop` pins the version and installs it when missing. `scripts/check` runs it after clippy, in CI too.
- On its first run it found 160 errors on macOS (13 module cycles, 13 tangles, 121 field leaks, 4 shallow modules, 5 wide directories, 4 long files). They went into allow lists unfixed. The gate fails only on new ones.
- One run checks one configuration. `--all-features` is the one the tests build. The default features gave 162 errors, mostly the same: their circles are smaller (the `snapshots` code is gone), and 5 field leaks show only there. Running both would double the time for those 5.
- One allow list per target triple: Linux adds `shallow-module guided_review::platform`, since `src/platform` has other code there. A single list would tell a Mac run that the Linux entry is fixed, and following its tip to update the list would then break Linux CI. `scripts/deslop --update-allow` rewrites every list from a Mac with `--target`.

Open as of 2026-11-25: the 160 errors.

## 2026-11-25: module cycles and tangles, wave 1

Commit `6f8bf7ef`. The 13 module cycles and 13 tangles went to 3 and 5; the allow list went from 160 to 144 entries. Behaviour is unchanged: the full test suite passed before and after.

### Layers

The crate root, top to bottom: `main` and `app`; `open_review` and `opened_subject` (wiring); the adapters (`github`, `claude_cli`, `hc_tour_agent`); `viewer`; `config`; `instructions`; the git and code readers (`local_git`, `local_work_tree`, `code_source`); `navigation`; `pieces`, `symbols`, `language`; the value modules (`keys`, `merge_method`, `more_lines`, `piece_kind`, `version`, `build`). `update` sits above `app_folders`, which sits above `platform`, which sits above `app_icon`.

Inside `viewer` the rule is: a value type (a line's place, a screen, a thread as GitHub has it, a comment's place) sits below the code that acts on it. Most of the 71-module cycle was value types written in the module of their busiest user.

### What moved, and why

- `keys/` (chord, keymap, shortcuts, send_action), `more_lines` and `merge_method` left `viewer`: `config` reads them and `viewer` reads `config`. `Command::from(SendAction)` replaced `SendAction::command`, ending the send_action/shortcuts cycle.
- `local_reviews_on_disk` holds the local reviews on disk and the snapshot-ref clean-up, below `viewer`. `TourRepinner` moved to `opened_subject`; `PrMover` is handed the drop of the local review (`DropLocalReview`), as it is handed its code source.
- The review agent's tool lists are worked out by the herdr adapter, as the tour agent's are: `ReviewAgentStart` carries the place, findings file and skill.
- `Config::read_instructions` and `Config::skill_for_launch` replace `Instructions::of` and `skill_for_launch`: the config reads the instructions it holds. The off-switch env names belong to `InstructionsKind`. `AskSettings::from_config_text` is gone.
- `LocalWorkTree` reads its own `origin`; `PieceKind` has its own module; the names a test file imports are test pairing's; `Codebase::of(subject)`.
- `platform`: `write_desktop_entry_templates` lives in `platform.rs`, which owns both adapters; `platform::app_folders` gives the OS's folders and `AppFolders::xdg_of` builds from them; a Dock icon is an `AppIcon`, so `platform` never asks which build runs. `update::running_build()` replaces `Build::running()`. `Mover::of` decides who moves the app folders.
- `navigation`: `Lookup` and `Reach` are the navigator port's answer, written beside it; `FilesWithWord` lives with its git grep adapter. `language_server::downloads` holds the download cache under the recipes; the npm download is `NpmPackage`'s.
- `app`: a button's cap ground is the button's; `outdated_lines` holds the block both thread and finding views draw; `AskSetup` is part of `OpenedReview`.
- `viewer` new modules, each a layer below its users: `line_location` (Side, LineLocation), `screen` (Screen, ReadSubject, NextScreen), `review_thread` (a thread as GitHub has it), `comment_send` (sending one comment), `diff_line`, `piece_diff`, `carried` (what one review carries into another), `refresh_brief`, `local_thread_store` (the store over the state file, above the local threads it changes). Smaller moves put a type with its lowest user: `Audience` in comments, `Focus` in display_options, `ReviewVerdict` in ports, `Read` in github_outage, `SubmittedReview` and `ReviewOutcome` in pr_reviewers, `TourFreshness` in thread_actions, `OpenOrigin` in tour_agent, `WritingFact` in active_pr, `RepinnedTour` in replacing_jobs, `TourReloader` in review_builder, `shell_word` in `prompt_safety`.
- Tangles: `land_elsewhere` settles a comment outcome in a state file, so `CommentOutcome` no longer knows `CommentDesk`; highlight configurations are one array of `OnceLock`s; `index_of_focus` is a function, not a `Focus` method.

### Allow-list lines added, all from renames or a cut that showed what was hidden

- `module-cycle viewer::focused_change,page,replacing_jobs,review,screen_fold` and `module-cycle viewer::hand_offs,local_thread_store,review_state`: what is left of the 71-module cycle.
- `tangle viewer::hand_offs::…,local_thread_store::…,review_state::…` and `tangle viewer::replacing_jobs::{AfterJob,JobOutcome,RepinnedTour},review::kept::Review`: the same tangles, their members renamed by the moves.
- `field-leak keys::keymap::KeyChanges::send_choice` and `field-leak viewer::comment_send::CommentRequest::draft`: renamed keys of existing entries.
- `field-leak viewer::comment_send::CommentRequest::mode` and `::origin`: `CommentDesk::settle` read them before; the CommentDesk/CommentRequest tangle hid them, and cutting it shows them.
- `shallow-module keys`: `KeyChanges` showed its 11 functions to Settings inside `viewer` too, where viewer's size hid it. Narrowing it is the shallow-module wave's.

### Left, and why

- `module-cycle viewer::{review,page,replacing_jobs,focused_change,screen_fold}`. `Review` keeps its `Page` and `page` builds it from `ReviewCore` (58 items): `ReviewCore`'s methods are written across `review/*`, so taking it out of `review` means moving most of `review.rs`'s 3,066 lines. A job's outcome carries the next `Review` (`RepinnedTour`), and `Review` lands its jobs: that circle is the data's own. `focused_change` and `screen_fold` hold both page data (`PageChange`) and `impl Review` actions; a split needs `PageChange`'s fields seen outside it, which only trades a cycle for field leaks.
- `module-cycle viewer::{hand_offs,local_thread_store,review_state}` and its 9-thing tangle. `HandOffs` lives in the state file and builds `AgentReplyRequest`s that write through the local thread store, which locks that file. Breaking it means `ThreadHome` naming its store without holding it: a change to how agent replies reach a local review, not a move.
- `module-cycle app` (11 modules) and `tangle app::apply_keymap,…` (19 things). `app::components` holds both the leaf widgets (icon, palette, toast stack) the app's helper modules draw with and the composite panels (Ask panel, Settings, PR switcher, finish card) that read the session. Breaking it means splitting `components` in two, a move of most of its 46 entries in gpui code with no window tests.
- `tangle app::components::palette::Appearance,app::snapshots::appearance`: the snapshot harness stages the appearance, read behind `cfg(feature = "snapshots")`. Inverting it needs a staged global set before the first read, which only a snapshot run proves; none was run.
- `tangle viewer::page::Page,…,review::ReviewCore,…`: the same Page/ReviewCore knot as the first cycle above.

Open as of 2026-11-25: 144 entries on macOS.

## 2026-11-25: field leaks of the shared viewer types, wave 2

Commit `7798bb32`. 22 field leaks fixed on each target; the macOS allow list went from 144 to 122 entries, and `scripts/deslop --update-allow` only removed lines. Behaviour is unchanged: the full test suite passed before and after.

### What changed, and why

- **`ReviewCore`, 13 of its 23 leaks.** `Review` (`review/kept.rs`) keeps the page and derefs to `ReviewCore`; the work agenda, the replacing jobs, the stations, Ask why, the carry-on after a re-pin and the take-over from a basic review were written as `impl Review` and worked on `ReviewCore`'s fields through `Deref`, with `work_mut` and `jobs_mut` as ways round the page drop. That logic is now `impl ReviewCore`. `Review` keeps only what is about the page: it forwards, without dropping the page, the changes that are no input of it (end of screen seen, `due_work`, `pull_from_github`, `due_job`, `job_did`), and it lands a job (`land_job`, now in `review.rs`), since a job's outcome carries the `Review` that replaces it. `ReviewCore::job_failed` no longer returns `AfterJob`: that return made `ReviewCore` know its own replacement, which merged the jobs tangle into the Page tangle.
- The app asks `ReviewCore` what it wants: `job_progress` and `job_is_running` (not `jobs()`), `has_last_review`, and the count `findings_done` returns (not `ai_review().counts()`). `switched_from_basic(carrier)` replaces the two setters `TourSwitcher` called; `steps_carried_read_ticks_are` is a snapshot scene's only.
- **`TourPin`, all 5.** Callers cut the pinned diff, compared the head and moved the pin by hand. The pin now `cut`s its own diff, `move_to`s new commits (keeping `previous_head`), and answers `needs_pinning`, `is_at_head` and `names_a_subject`; callers that need both commits take `pinned_tips()`. `tour` now uses `pieces` and `code_source`, neither of which uses `tour`.
- **`TourWriting`, 2 of 3.** The loading screen, the station and the guided-tour line matched on `stage` and `status` and read `terminal`. It now says `failure`, `phase` (`WritingPhase`: waits for you, starting, writing), `status_line` (moved from the loading screen), `last_lines`, `waits_for_an_agent` and `runs_in(terminal)`.
- **`ActivePr`**: `shows_key` for the four agent-reply commands, `is_shown` in the PR switcher, `is_switching` and `is_switching_to`. Its three entries stay (below).
- **`Asks`, its 1.** `asked_lines_shown_on` and `ids_where` replace walking `all()` from `AskDesk` and `ReviewCore`.
- The kept page now drops in a few more places: `start_job`, `land_job` and Ask why to the review agent go through `DerefMut` before they know whether they change anything. The page is built again from the same review, so nothing shown changes.

### Left, and why

- `ReviewCore::screen` and `::subject`: every view reads which screen is open and what is reviewed, as values. The lint counts a method on a borrowed value (`==`, `clone`, `short`) as a leak; returning copies would only hide that.
- `ReviewCore::display_options` and `::ask_desk`: `DisplayOptions` and `AskDesk` are the deep modules the app works with (the first deepening pass made `AskDesk` the Ask panel's interface and left display options outside the save gate). Forwarding their interfaces through `ReviewCore` would widen the widest type in the crate and hide nothing.
- `ReviewCore::guided_tour`: the app learns the guided tour's progress from the tour agent and tells the review; the setter is that interface.
- `ReviewCore::ai_review_run`, `::pr_origin`, `::steps_started_read`: only snapshot scenes set them now.
- `ReviewCore::working_copy`: one setter, from `ReviewBuilder`, for a fact it reads from disk; for the single-use wave.
- `ReviewSession` (5): the session owns the `Review`, its rows, the sidebar rows, the clone offer and the old head, and the layout and the Ask panel read the review and the rows in 82 places. A method per drawn fact would move the layout's reading onto the session, not narrow it.
- `ActivePr::shown` and `::switching`: what is left reads the subject as a value to name it (Back to, Opening). `ActivePr::replies_for_reopened`: `take_replies_for_reopened` hands the held replies over; the lint reads `mem::take` as a getter.
- `TourWriting::screen`: `screen_is` is the event "the agent's screen shows these lines", from `ActivePr::writer_screen` and a scene.
- Not looked at in this wave: `FileBlock`, `CommentRequest::draft`, `NameHover`, the fields of `DisplayOptions`, `CommentDesk::selection`, `Page`.

Open as of 2026-11-25: 122 entries on macOS. `move_to_pr::a_failed_look_up_of_the_head_branch_is_tried_again_later_and_later_but_not_on_focus` failed once in a loaded full test run and passed alone three times and in the gate; it times its retries from `Instant::now()` taken before the PR opens.

## 2026-11-25: long files and shallow modules, wave 3

Commit `a1c766bc`. Three of the four long files are split; the macOS allow list went from 122 to 119 entries. `scripts/deslop --update-allow` removed the three `long-file` lines and renamed one key (below). Behaviour is unchanged: the code moved without edits but for imports, visibility and module docs, and the full gate passed.

### What moved, and why

- **`src/app/layouts/one_step.rs`, 6,138 lines to 1,612.** The layout keeps its state, `new`, the gestures, `act` and `render`. Each part of the window it draws has a file under `one_step/`: `page.rs` (the page's rows, the overview, the toolbar, the screen footer, the free functions that draw them), `title_bar.rs` (title, merge route, status, checks, related issues, the page hairline and its test), `sidebar.rs` (the offers under the stations, the file tree or route, the settings row), `scroll.rs` (sticky step bar, pinned file header, scrollbar, focused change in view, diagrams let go off screen), `comments.rs` (comment boxes, threads, the finish dialog) and `findings.rs` (the findings' buttons, the why questions, the fixes). The four station blocks (AI review, Send fixes, tour agent, guided tour) moved into `stations_view.rs`, the stepper that draws them: in `sidebar.rs` they made a sidebar/stations_view cycle. `box_fold_toggle` lives in `page.rs`, which draws file boxes, and `scroll.rs` borrows it for the pinned header; the other way round made a page/scroll cycle. `ai_review_icon`, used by `page` and `stations_view`, stays in `one_step.rs`.
- **`src/viewer/review.rs`, 3,181 lines to 1,934.** `ReviewCore`'s methods by topic, beside the existing `review/*` files: `pr.rs` (the other side of the branch, route, related issues, the description's notes, the reviews, the reviewing mark, Mark done and the finish), `comments.rs` (threads on the page, the comment boxes, selection, the queue, landing a sent comment, hand-offs) and `reading.rs` (reading order and the moves through it, read ticks, folds, progress, the viewed sync). They call each other one way (`comments` and `pr` read `reading`; `pr` reads `comments`). What two of them need stays in `review.rs`, which every child may call: the jump palette (`jump_targets`, `palette_with`, `palette_actions_with`), `place_threads`, `tour_freshness`, `start_sending`, `old_head`, `pr_head_is`, `set_pr_description` and `finish_action`. A first cut that moved them made a 6-module cycle. `review/` holds 15 files, the most a directory holds before `wide-directory`.
- **`src/app/components/settings_panel.rs`, 2,350 lines to 1,294.** One file per Settings page, as `keys.rs` already was: `agents.rs` (instructions, skill picker, Ask), `code.rs` (editor, more lines, language servers, clones), `pull_requests.rs` (send actions, merge method). The shared row helpers stay in `settings_panel.rs`.

### Allow-list line changed

- `tangle guided_review::app::apply_keymap,…`: the same 19 members; seven of them are one_step free functions now named by their file (`one_step::page::finish_card`, `lead_button`, `route_stop`, `screen_footer`, `step_heading`, `tick_button`, `one_step::title_bar::merge_button`).

### Left, and why

- `long-file src/app/review_session.rs` (2,200 lines). The one seam that takes it under 2,000 is the snapshot staging (38 methods, about 600 lines, all behind `snapshots`). deslop-please judges `shallow-module` only for modules with child modules, so a `review_session/staging.rs` makes it judge `ReviewSession`, which shows 95 of its 130 functions: a new error. A sibling module to avoid that would be a file moved to dodge a count. Narrowing what the layout asks `ReviewSession` comes first; wave 2 left its 5 field leaks for the same reason.
- `shallow-module viewer::review` (shows 199 of 360 functions; `ReviewCore` 172). `ReviewCore` is the one interface the app has to the viewer: the session and the layouts ask it one drawn fact or one action at a time. Splitting its file changes no count, since the lint judges the module with its children. Deepening it means the app asking for something wider (a view of a screen region, more of the actions through `decide(Intent)`), a change of the app/viewer interface in gpui code with no window tests, not a structure move.
- `shallow-module viewer::review_state` (shows 91 of 113; `ReviewState` 75). `ReviewState` is the state file's record, changed only through the `SavedState` gate (first deepening pass); its functions are the record's reads and writes that `ReviewCore` decides. Moving those decisions into the record would put review rules into the persisted shape. The lint judges this module only because of its inline `pull_request_links` serde module; removing that to pass would only hide the error.
- `shallow-module app::components::palette` (shows 27 of 36; `Colors` 17). `Colors` is the design tokens of [visual design](visual-design.md): each call site picks one named shadow, fill or text colour. Fewer functions would only move those names into an enum argument.
- `shallow-module keys` (shows 38 of 67; 11 things, `KeyChanges` 11). `keys` is the vocabulary config, viewer and app share (chord, key, command table, keymap, send actions), used as values. Moving the `[keys]` TOML read and write from `viewer::settings` into `KeyChanges` would replace `entries` and `written` with two new shown functions, about 35 shown against 31 hidden, still shallow (estimated from the call sites, not run).
- The wave's brief named `update`; no `update` entry is on either list.

Open as of 2026-11-25: 119 entries on macOS.

## 2026-11-25: what was left, wave 4

Commit `4ae77c74`. 19 field leaks fixed on each target; the macOS allow list went from 119 to 100 entries, and `scripts/deslop --update-allow` only removed lines. Behaviour is unchanged: the code moved into the type whose fields it read, and the full gate passed. `scripts/check` now warns about lines a change adds to an allow list.

### What changed, and why

- **`SyncOutcome`, all 4, and `Coverage::files_by_subject`.** `ViewedSync::take_in` and `SyncRequest` built and read the outcome of a viewed sync field by field. The outcome now says how it came about (`failed`, `pushed`, `baseline_taken`) and `land`s itself in the review state; `push` returns what it sent and where it stopped. `Coverage::held_files` replaces walking its map from `baseline_wishes`.
- **`CommentRequest`, all 3.** `CommentDesk::settle` and three free functions read the request's origin, draft and mode to decide what the state file keeps. The request now does it: `leave`, `keep_for_retry`, `keep_as_draft`, `sent_comment` (what a hand-off is made from) and `leaves_a_pending_review`. `CommentOutcome::thread_made` finds the thread the comment became; `findings_carried` names the findings the draft carries.
- **`CommentDesk::comment_edits` and `::delete_asked`.** `ReviewCore` built `ThreadWork` from two getters; `CommentDesk::thread_work(reply_drafts)` builds it.
- **`Standing`, both.** The sidebar line, the "already listed" check and your review's status read the reviewer and the stance. `Standing::line` (was `ReviewerLine::of`), `is_by` and `status` answer them. The fields stay `pub` for the tests that read a parsed answer.
- **`SymbolPairing`, both.** The cut of a file into symbol pieces reached through the pairing to the base and head symbols. The pairing now answers `head_slot_of_added_line`, `symbols_in`, `tokens_in` and `has_same_text_in` for a slot pair.
- **`BladeScan`, both.** `highlight_blade` coloured the scan's marks and PHP parts. `BladeScan::coloured` does, with `PhpCode::coloured` for one PHP part; `html_only` and `PhpCode`'s fields are private now.
- **`RepinRecord::pieces`.** `RefreshBrief` counted the record's pieces; `new_or_changed_of_all` gives both counts.
- **`KeyChanges::send_choice`.** Settings read the send choice, changed it and wrote it back; `choose_primary` and `choose_secondary` replace `set_send_choice`.
- **`InlineText::code_ranges`.** `InlineText::code_backgrounds` draws the grounds of its own code; `width_units` and `widest_word_units` give a table column's widths.

### Tried and undone

- Drawing `InlineText` as its own method (`styled_text` and `styled_links`) made a new 14-item tangle: the link click opens through `code_links`, which reads the session, which holds the page rows, which hold the prepared markdown. Prepared text stays below the drawing; `InlineText::text` and `::links` stay listed.
- `SavedState::thread_home` in place of `SavedState::file`: `ReviewCore::state_file` hands the same file to the app's session for its background work, so the getter stays.

### Left, and why

- **Values the app draws** (`Colors`, 12; `Snippet`, 2; `YourReview::avatar_url`; `CloneOffer::state`; `AskDesk::target`; `NameHover::open`; `PageRows::description_hidden`; `InlineText::text` and `::links`; `PreparedMarkdown::blocks`). Each is the viewer's or the prepared rows' answer of what to draw, read as a value. `Colors` is the design tokens of [visual design](visual-design.md). A method per drawn fact would move drawing decisions into the viewer, or forward to the one function that draws them.
- **`EditState`, 14.** It is the text box's state across frames, shared by `TextBox` and `TextBoxElement` in one file; gpui's `Element` trait splits that code by layout phase, not by data. Moving `request_layout` and `prepaint` into `EditState` is a rewrite of a gpui element with no window tests.
- **Events and hand-overs the lint reads as getters or setters**: `PrSwitcher::hovered` and `::selected` (`hover` and `select` are the pointer's events), `AskDesk::code_changed` (`code_changed_is` is the event), `DiagramMemory::pending_svgs`, `ReviewState::typed` and `Saves::typing_is_unsaved` (each `mem::take`s work to hand it over), `SavedState::state` (`keep_until_repinned` is the save gate's closure), `PressAgain::to_send` (set only by a snapshot scene) and `NameHover::pointer` (read only by one).
- **`ReleaseInstall::desktop_entry`**: the install hands out its OS adapter (`dyn DesktopEntry`), which `launch`, `install_app`, `start_app` and the updater use as a port. Forwarding six port methods through the install would widen it and hide nothing.
- **`OpenedPr::folders` and `OpenedChangeSet::folders`**: `OpenedSubject` is one enum over the two adapters; it reads each adapter's app folders to do what both share (the state file, the shown tour). Moving that into each adapter writes it twice.
- **`SavedState::file`**: see "Tried and undone".
- **`Signature::text` and `::origin`**: `Signature` builds a declaration line and where its bytes came from; its two fields are what it builds, and `Definition` stores them as two arrays. A `finish` returning both would be the same read under another name.
- **`DisplayOptions`, 3**: the reader's display choices, asked by the page builder and `ReviewCore` one choice at a time; the first deepening pass left them outside the save gate on purpose.
- **`Page`, `PageChange`, `FileBlock` (10)** and **`ReviewCore`, `ReviewSession`, `ActivePr`, `TourWriting`**: the Page/ReviewCore knot and the reasons waves 1 and 2 gave stand. The page's types are read by the builder and the focused-change moves that wave 1 left in one cycle with them.
- **`module-cycle` and `tangle` (8)**, **`shallow-module` (4)** and **`long-file src/app/review_session.rs`**: unchanged since waves 1 and 3, for the reasons given there.
- **`wide-directory` (5)**: `src/` (71 entries), `src/app/` (22), `src/app/components/` (46), `src/viewer/` (113), `tests/it/` (164). A folder per group is a module per group, and deslop-please checks cycles among siblings: `viewer`'s modules still form the two cycles above, so a split of its 110 files into folders of at most 15 puts those cycles between the new folders. Grouping waits for the cycles. `tests/it/` is one module per topic (AGENTS.md); its 163 topics have no grouping that is not by the `src` module they test, which would follow the same split.

### The gate warns about new allow-list lines

`scripts/deslop-added <base>` lists the lines the allow lists on disk have that they did not have at the fork point with the base; a list that did not exist there is named with its count. `scripts/check` runs it after `scripts/deslop`, outside CI, against the same base as the change notes. It warns and never fails: a rename may need a new key.

Open as of 2026-11-25: 100 entries on macOS, 101 on Linux.
