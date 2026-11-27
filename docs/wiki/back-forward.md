# Back and Forward

The 2026-11-27 decisions that gave the reviewer a history of where they have been, like a browser's.

## Decisions, 2026-11-27 (maintainer)

- **Inputs.** The mouse side buttons (gpui `MouseButton::Navigate`, on the layout's root so they work over any overlay, through `Intent` so a reopening review refuses them), and the commands `HistoryBack` / `HistoryForward` on `Cmd+[` / `Cmd+]`, in the one `Command` table (`history_back`, `history_forward` in `[keys]`), window scope.
- **Every change of screen is a visit.** The research proposed that only jumps push and that Right and Left do not, so that Back would not duplicate Left. The maintainer decided on a browser's rule instead: next, previous, next unread, sidebar, palette, Definition card, comments and files lists, route stops, all of them. A move after going back drops the forward list. Going back and forward are not visits, and neither are overlays, Settings or the scroll script.
- **Back restores the screen and a viewer anchor.** The anchor is the first code line shown at or after the top row of the view (`ReadingSpot`, `Page::spot_at`), searched within 40 rows; the row index is kept as a hint, used when that line is no longer on the page (`Page::row_of_spot`). Row indices shift when threads and boxes load, a line's place does not. The offset inside the row is not kept: the row lands at the top of the view.
- **Where it lives.** `ReviewCore` holds the history (`viewer/visits.rs` for the visits, the moves in `review/reading.rs`). The one place a screen changes is `ReviewCore::move_to`. `ReviewSession::change_session`, which every change of the review goes through, tells the review which row is at the top of the view before the change (`Review::note_scroll_top`) and scrolls to the restored row after it (`Review::take_row_to_scroll_to`).
- **Scope.** Per review. A re-pin carries it over (`carry_on_from`) and the reopened review resumes on its screen without a visit (`resume_on`); entries for screens the new tour no longer has are passed over. It is gone when another PR is opened (Cmd+P), since that replaces the layout and the review.

- **Jumps on the same screen are visits too (2026-11-27, after trying the first version).** A jump to a `JumpTarget` (palette, Definition card, comments and files lists) pushes the place it left even when it lands on the open screen, so Back returns to the line jumped from instead of the previous screen (`ReviewCore::jump_to`). An entry is a screen plus a reading spot; scrolling by hand does not push. A jump that lands on the very row already at the top still leaves an entry, so one Back then looks like it did nothing.

## Open

- Back does not cross a PR switch.
- The palette has no Back or Forward entries.
- The page has no buttons for them; the mouse and the keys are the only inputs.
