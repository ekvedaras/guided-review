# Keys and the focused change

Decisions on keys you can change, new keys, a focused change and piece labels. Plans and tickets live in GitHub issues ([Where work is tracked](delivery.md)); this page keeps the decisions and why.

## Where it came from

On 2026-11-13 a reviewer who uses the app asked for two things: a way to hide the piece labels (the thin `Class::method` lines that mark where each diff piece starts), because in an interface they come after every line; and keys to show about 10 more lines above or below a change, instead of reading the whole file. The same day the maintainer added: keys for Hide whitespace, Ask and the Code / Rendered switch, and every key changeable in Settings.

## Decisions, 2026-11-13 (maintainer)

- **Keys are changeable.** Each `viewer::Command` keeps its default keys; the reviewer's `[keys]` table in `~/.guided-review/config.toml` replaces a command's keys by its config name (`wrap_lines = "l"`, or a list `next_screen = ["down", "j"]`). An empty list leaves a command with no key. Settings edits them with a "press the new key" box and a reset per command. The text boxes' own editing keys (`cmd-left`, `alt-backspace` …) are not changeable: they copy macOS text keys.
- **One bad `[keys]` line does not reset the others.** The first draft used all defaults while any line was wrong; the maintainer rejected that ("don't be ridiculous"). The maintainer confirmed the same day ("one bad key should not break everything"): a line with an unknown name or a key the app cannot read is skipped; two commands on one key both fall back to their defaults; the app says so once with a toast when it opens, and Settings shows each problem.
- **A key is checked against every place it can reach.** `KeyScope::overlaps`: the window and app scopes reach every place; the page and "not typing" reach each other; a text box reaches neither. So `close_box = "w"` beside `wrap_lines = "w"` is allowed.
- **Simple keys:** `S` Hide whitespace, `A` Ask about this step, and each toolbar button shows its key. `W` Wrap lines was already there (only in a tooltip, so easy to miss).
- **A focused change.** A change (the changed lines with the unchanged lines around them, GitHub's hunk) can have the focus. `Tab` / `Shift+Tab` jump to the next or previous change; `↓` / `↑` first scroll inside a change taller than the screen, then move to the next. A click inside a change focuses it. When the focused change leaves the screen, the first visible change takes the focus. A thin accent bar marks it. Not in text boxes. No focus on screens with no code.
- **`[` / `]`** show more lines above / below the focused change, the same 20 lines as the gap buttons (`MORE_CONTEXT_LINES`), not the asked-for 10: the key and the button should do one thing.
- **Per-file keys** act on the focused change's file: `R` Code / Rendered, `L` piece labels.
- **Piece labels:** a per-file toggle in the file header, with a starting state from a rule in the app (labels start hidden when most pieces in the file are one or two lines), not chosen by the tour writer. Why: it is a fact of the code, free, works for old tours and needs no tour-format change. The tour writer may get an override later if the rule is often wrong.
- **Try the focus before its PR.** The maintainer asked on 2026-11-13 to try the focused change and its look by hand from an installed build (`cargo install --path . --locked`, AGENTS.md "Install") before any PR for it is opened.
- **Order:** (1) keymap, (2) simple keys, (3) focused change, (4) per-file keys and labels. Each is a PR; (4) needs (3).

## Progress

- 2026-11-13: spec ekvedaras/guided-review#54, tickets #55 (S, A, key caps), #56 (focused change), #57 (R, L, piece labels; blocked by #56). The planner found, as of `a914a37`: Hide whitespace and Code / Rendered are not `Command`s yet; the label rule has no threshold (left to a test); `↑` / `↓` need the viewport from the app; `Tab` may meet gpui's own focus order.

- 2026-11-13, branch `configurable-keys` (based on `ekvedaras/guided-review@a914a37`, first commit `3978c0d`): `viewer::Chord` (modifiers and one key, parsed from and written as gpui keystroke text), `viewer::Keymap` and `KeyChanges`, `Config::keymap`, Settings reading and writing `[keys]`, buttons / sheet / palette reading the keymap through `app::keymap`. Tests in `tests/it/keymap.rs` and `tests/it/settings.rs`.
- 2026-11-13, same branch: Settings has a Keys section (commands by group, each with its caps, Change, Cancel, Reset, a Remove for names that are no command); Save applies the keys at once; one bad `[keys]` line no longer resets the others (commands that clash fall back to their defaults, repeated until no fallback clashes) and the app opens with one toast when something is wrong. While a key is being pressed, `Esc` cancels, so `Esc` can only be given back by Reset. Commit `ed8271b`.
