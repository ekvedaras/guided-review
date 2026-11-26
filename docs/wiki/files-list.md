# Files list

2026-11-26, issue #137 ("a key that shows the changed files as a tree"). Code: `src/viewer/files_list.rs`, `ReviewCore::files_list` in `src/viewer/review/files.rs`, drawing in `src/app/components/files_sheet.rs`. Tests: `tests/it/files_list.rs`.

## Decisions

- **A popup, opened with `T`.** A prototype showed three looks: a panel on the right that stays open, a popup over the page, and a Files tab in the sidebar. The maintainer picked the popup: it is a key that opens a list, like `C` and `F`, and it has room for the steps of a file that many steps show. The prototype stays on the branch `prototype-137-file-tree`.
- **A file lists every screen that shows it, in reading order.** Return goes to the first, 1-9 or a click to that one, and the page lands on the file's box. A click on a row only moves the cursor, so a reviewer can look at a file's steps before going.
- **One model for both tour kinds.** A basic tour's file has one place, its own screen. The tree is `FileTree`, the one the basic sidebar draws, so both order and join folders the same way.
- **Read means every step that shows the file is read**, the rule the carry of reads after a re-pin uses. The safety net is not ticked, so it does not count.
- **Left on a file folds its folder** and puts the cursor on the folder, as an IDE's tree does (maintainer, 2026-11-26).
- **`?` searches.** The box in the header keeps the files whose path holds every word typed, ignoring case, with their folders unfolded; the folds come back when the box is empty. Escape in the box empties it and gives the keys back to the tree. Up and Down still move the cursor while typing, and Return goes to the first screen of the file under it.
- **The cursor starts on the first file the screen on show shows**, and is kept by name, so a fold or a refresh while the list is open does not move it.

## Known open

- The prototype pictures came from a PR where almost every file is in one step, so the many-steps case was picked without being seen.
- Folds are not kept between openings of the list.
