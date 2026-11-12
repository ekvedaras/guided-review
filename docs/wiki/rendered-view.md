# Rendered view

2026-11-12. A file box of a Markdown file or an SVG can show the file as it reads instead of its diff lines, switched per file in the box header. Code: `src/viewer/rendered_view.rs`, the page build in `src/viewer/page/build.rs`, drawing in `src/app/components/diff_view.rs` (`rendered_block_row`). Tests: `tests/it/rendered_view.rs`.

## Blocks, not lines

A rendered Markdown file is cut into its top-level blocks by pulldown-cmark's own source ranges, so a block is exactly what the renderer draws as one. Each block is marked by the lines the PR changed in it: every line added is added, some lines added or removed inside is changed. A run of removed lines that falls between blocks at head is cut into blocks of its own and shown, dimmed, where it was. A deleted file is its base, every block removed.

Each removed line learns the head line it sat under by walking base and head side by side, so the marking works the same for local diffs (`-U0`) and GitHub patches (with context).

The blocks are cut once, when the files at head land (`ReviewCore::set_files_at_head`), not on every page build.

Rejected: a word-level rendered diff. Formatting a merged base/head text needs a Markdown-aware diff, and the line diff is one click away.

## Starting view

Markdown starts as code: its text is what the PR changes and what line comments go on. An SVG starts rendered: its code is drawing coordinates nobody reviews by reading. A raster picture has no code to show, so it has no switch.

## Comments

A rendered box has no lines on the page (`Page::lines` skips it), so a drag over it opens no comment box. Threads on its lines are listed after the file inside the box, so they stay readable and answerable. Comments on lines are made in the code view.

## Not saved, no palette action

The view is a display option like whitespace and wrap, so it is not in the state file. There is no palette action: an `Intent` names no file, and one switch for the whole screen would mix Markdown (starts code) with pictures (start rendered).

## Open

- Relative images inside a rendered Markdown file are not loaded from the repo.
- A removed run that ends a list or a quote is shown as its own removed block after it, not as a change of it.
