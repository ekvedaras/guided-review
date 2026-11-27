# Keys and the focused change

Decisions on keys the reviewer can change, a focused change, per-file keys and piece labels. Plans and tickets live in GitHub issues ([Where work is tracked](delivery.md)); this page keeps the decisions and why. The full key table and the send-action keys are in [Review UX](review-ux.md).

On 2026-11-13 a reviewer who uses the app asked for a way to hide the piece labels (the thin `Class::method` lines that mark where each diff piece starts, which in an interface come after every line) and for keys to show more lines above or below a change instead of reading the whole file. The maintainer added keys for Hide whitespace and the Code / Rendered switch, and every key changeable in Settings (spec #54).

## Changeable keys (maintainer, 2026-11-13)

- Each `viewer::Command` keeps its default keys. The reviewer's `[keys]` table in `config.toml` replaces a command's keys by its config name (`wrap_lines = "l"`, or a list `next_screen = ["down", "j"]`). An empty list leaves a command with no key. Settings → Keys edits them with a "press the new key" box, Cancel, and a reset per command; Save applies them at once. While a key is pressed, `Esc` cancels, so `Esc` can only be given back by Reset; the maintainer kept this since Esc is the usual way out of a key recorder. The text boxes' own editing keys (`cmd-left`, `alt-backspace` …) are not changeable: they copy macOS text keys.
- One bad `[keys]` line does not reset the others. A first draft used all defaults while any line was wrong; the maintainer rejected it ("one bad key should not break everything"). A line with an unknown name or a key the app cannot read is skipped; two commands on one key both fall back to their defaults (repeated until no fallback clashes); the app says so once with a toast when it opens, and Settings shows each problem.
- A key is checked against every place it can reach (`KeyScope::overlaps`): the window and app scopes reach every place; page and "not typing" reach each other; a text box reaches neither, but another text-box key. So `close_box = "w"` beside `wrap_lines = "w"` is allowed.
- Simple keys are `Command`s in page scope, so they do nothing in text boxes and overlays: `S` Hide whitespace (nothing on a screen whose toolbar has no such button), `W` Wrap lines, `O` fold (below), and `Cmd+[` / `Cmd+]` for [Back and Forward](back-forward.md). Toolbar buttons show their key cap.
- `O` (`Command::ToggleFold`, maintainer, 2026-11-16) does what the step toolbar's fold button does: opens every box of a skim, ticked or gone step, and folds them again when pressed again. On a basic tour's file screen, which has no step fold, it opens or folds the screen's own boxes (`ScreenFold`; `Page::screen_fold` says which a screen has). The palette offers it only where the toolbar has the toggle.
- `D` (draft) and `Shift+M` (auto-merge) and their press-again rule are in [Review UX](review-ux.md).

## The focused change

- A change (the changed lines with the unchanged lines around them, GitHub's hunk) can have the focus, a `ChangeId` in `DisplayOptions`. A thin accent bar marks it. A click inside a change focuses it. When the focused change leaves the screen, the first visible change takes it, and every code screen gives it to the first change in view so the bar shows at once. No focus on screens with no code.
- `Tab` / `Shift+Tab` jump to the next or previous change and wrap at the ends. They do not meet gpui's focus order, which only moves on an explicit `focus_next`, and text boxes bind no `tab`.
- `↓` / `↑` scroll inside a change taller than the view (steps of 0.8 of the view) and then move to the next or previous; they do nothing at the last / first change. With no focus they take the first / last. A moved focus lands at the top of the change, also going up. The maintainer tried this by hand on 2026-11-13 and called it good enough.
- A file box shown in two parts (a `rest` piece leaves middle lines to another step) holds the change in both parts, and `Focus::Change` carries `part`, so `↓` walks both. When more lines join the parts the focus goes to the last one left. Before 2026-11-15 a cut part held no change, was counted as a rendered file, and every focus landed on the first.
- A file box shown rendered is one change with no hunk (`Focus::RenderedFile`): `R` switches it back and Tab walks through it like any change.

## More lines

`[` / `]` show more lines above / below the focused change, the same number as the gap buttons, so the key and the button do one thing. That number is the setting `more_lines` (`MoreLines` in `src/more_lines.rs`): a whole number from 1 to 200, default 20, in Settings → Code. Below 1 a step shows nothing; above 200 one step is several screens and the gap's "all" button already shows everything. Any other value is a Settings problem that blocks Save; the app itself opens with 20. Twenty typed or left blank writes no key. Save reopens the PR, so the number reaches the review like the other settings.

## Per-file keys and piece labels

- `R` (Code / Rendered) and `L` (piece labels) act on the focused change's file, are page-scope `Command`s in `[keys]`, and the palette offers them only with a focused change whose file has the switch.
- Piece labels are a per-file toggle ("Labels" in the header of a file box of several pieces). The starting state comes from a rule in the app, not from the tour writer: labels start hidden when more than half the file's pieces are tiny (`TINY_PIECE_LINES` = 2 added plus removed lines, `PieceLabels::starting_for_sizes`); exactly half keeps them. Why a rule: it is a fact of the code, free, works for old tours and needs no tour-format change. The rule counts the lines changed in the PR, so Hide whitespace does not move it, and the reviewer's own choice is kept. The tour writer may get an override if the rule is often wrong.
- Hiding drops only the marker rows, so per-piece buttons on a marker are gone while hidden, and a jump to a piece of such a file lands on the file header. Folded boxes keep their labels.
