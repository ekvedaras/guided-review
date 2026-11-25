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
