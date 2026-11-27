# Back and Forward

The 2026-11-27 decisions that gave the reviewer a history of where they have been, like a browser's.

## Decisions (maintainer, 2026-11-27)

- **Inputs.** The mouse side buttons (gpui `MouseButton::Navigate`, on the layout's root so they work over any overlay, through `Intent` so a reopening review refuses them), and the commands `HistoryBack` / `HistoryForward` on `Cmd+[` / `Cmd+]` (`history_back`, `history_forward` in `[keys]`), window scope.
- **Every change of screen is a visit.** The research proposed that only jumps push, so that Back would not duplicate Left. The maintainer chose a browser's rule: next, previous, next unread, sidebar, palette, Definition card, comments and files lists, route stops, all of them. A move after going back drops the forward list. Going back and forward are not visits, and neither are overlays, Settings or the scroll script.
- **Jumps on the same screen are visits too.** A jump to a `JumpTarget` pushes the place it left even when it lands on the open screen, so Back returns to the line jumped from (`ReviewCore::jump_to`). An entry is a screen plus a reading spot; scrolling by hand does not push. A jump to the place already shown leaves no entry: `Review::jump_to` asks the page where the jump lands (`Page::row_of_jump`) and pushes nothing when that is the open screen's row at the top of the view, since one Back would then look like it did nothing. A folded step does not count as shown, since the jump opens it and moves its rows.
- **Back restores the screen and a viewer anchor.** The anchor is the first code line shown at or after the top row of the view (`ReadingSpot`, `Page::spot_at`), searched within 40 rows (`SPOT_REACH`); the row index is kept as a hint, used when that line is no longer on the page (`Page::row_of_spot`). Row indices shift when threads and boxes load; a line's place does not. The offset inside the row is not kept: the row lands at the top of the view.
- **Where it lives.** `ReviewCore` holds the history (`viewer/visits.rs`, the moves in `review/reading.rs`). The one place a screen changes is `ReviewCore::move_to`. `ReviewSession::change_session`, which every change of the review goes through, tells the review which row is at the top of the view before the change (`Review::note_scroll_top`) and scrolls to the restored row after it (`Review::take_row_to_scroll_to`).
- **Scope.** Per review. A re-pin carries it over (`carry_on_from`) and the reopened review resumes on its screen without a visit (`resume_on`); entries for screens the new tour no longer has are passed over. It is gone when another PR is opened (`Cmd+P`), since that replaces the layout and the review.

## Open

- Back does not cross a PR switch.
- The palette has no Back or Forward entries, and the page has no buttons for them; the mouse and the keys are the only inputs.
