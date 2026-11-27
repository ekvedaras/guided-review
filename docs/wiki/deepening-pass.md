# Deepening pass

2026-11-12. An architecture review found decision logic in `src/app/` (gpui, no window tests) that belonged in `src/viewer/` (window-free, tested through the fakes in `tests/it/support/fake_ports.rs`). Six candidates moved it behind small viewer interfaces, each behaviour-preserving and tested through the new interface (commits `535e9eb` to `70804ab`, `git log --oneline f9d9e8d..70804ab`). The reason is the same for all: a rule the app keeps cannot be tested, and a rule spread over the session, the layout and a component is changed in one place and forgotten in another.

## The rules

1. **One change gate, `SavedState`** (`src/viewer/saved_state.rs`). Every change of the saved review state goes through it: `change` for the reviewer's actions (refused while a re-pin runs), `type_in` for keys (saved by the typing save), `record` for outcomes from GitHub or an agent (made always, only the save refused while re-pinning). Before, each `Review` method checked the re-pin and saved by itself, and some forgot. Display options are not saved, so they are reached through `display_options_mut` and are no part of the gate.
2. **One `Problems` value** (`src/viewer/problems.rs`). The failures the reviewer must know about are one `Problems` keyed by `ProblemKind`, whose doc on each variant says what clears it. A new failure is a new variant. A failed Show agent and a tour that could not be read again are problems too.
3. **`CommentDesk` owns the text boxes.** A draft, a reply, an edit and the finish body are one `TextBox` with one way to open, type in, leave and give up. Whether a box composes, sends or keeps a problem is the desk's, which decides what each send outcome does to its box. `open_comment_box(CommentGesture)` opens a box from a drag or a picture's button. Before, the session and `RowFacts` kept copies of that state.
4. **The kept `Page`.** The viewer lays out the page of the open screen once per change and keeps it beside `ReviewCore` in `Review` (`src/viewer/review/kept.rs`). Reads go through `Deref`; the only `&mut ReviewCore` is `DerefMut`, which drops the page first, and the two fields are private to `kept.rs`, so no change can leave a stale page. Any `&mut` drops it, even one the page does not read (a drag selection, typing). The app's list holds the `Arc<Page>` it took at its last rebuild, so those changes cost no rebuild. Two changes keep the page on purpose, each in `kept.rs`: the seen end of the screen and the work agenda, which the page does not read. Selection, suggestion and picture queries read one page instead of building the step again.
5. **The work agenda.** `Review::due_work(now)` gives the off-thread work due now (reads of GitHub and the clone, the viewed sync, queued comments, seen marks, hand-off delivery), and `Review::land` takes each outcome in and makes due what follows. Which work waits, is dropped or replaces the one running is the agenda's, so the session has no single-flight flags, and tests drive the same loop with `drive_until_idle`.
6. **`Intent` and `Effect`: one `decide`.** What a key, button or palette action does is `Review::decide(Intent, WindowMoment) -> Effect` (`src/viewer/intent.rs`), with the refusals while the PR reopens and while Settings is open in `Intent::refusal`. The layout carries the effect out in one `act`. The read rule of going on and the scroll script's step into the next screen are the viewer's too.

A seventh candidate, the Ask desk (`AskDesk` and `AskProcesses`), moved the Ask panel's decisions into the viewer. It was removed on 2026-11-27 with the Ask panel; see [agent threads](agent-threads.md).

## Open behaviour questions

Each pass kept the behaviour it found, even where it looked wrong. These are for the maintainer to decide (not rechecked in the code after 2026-11-12 unless noted):

- (a) The forward key ticks the step it leaves only after the step's end was seen; the footer's Mark as read and go on always ticks (`f96632e`, a test names the difference).
- (b) The refusals while the PR reopens are not consistent: the palette's Hide/Show whitespace, Finish review, Open on GitHub, Restart and the PR switcher still act. Settings open does not block the shortcuts sheet, though [review UX](review-ux.md) says it waits.
- (c) During a re-pin, `open_reply` and the `discard_*` actions change a box in memory through `record`, and the reload undoes it.
- (d) A sent or queued comment stops composing even when another box is composing. Leaving a draft also drops a queued comment at the same place. A refused-drag notice stays after a picture's comment box opens. A problem saved after a failed save stays hidden and comes back if a box opens again at that place.
- (e) A comment-and-ask-agent comment sent later from the queue does not start a delivery until the next trigger.
- (f) `switch_failed` is never cleared on purpose, and neither is `ProblemKind::TourNotRefreshed`: a successful switch or refresh replaces the review.
- (g) A failed Show agent lists in the problems list and posts a toast that stays until dismissed.
