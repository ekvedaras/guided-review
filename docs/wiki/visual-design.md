# Visual design rules

2026-11-12, kept current to 2026-11-27. The rules the app's look follows, from a design pass over every snapshot scene in both appearances (commits b103e63, 1775047, 0028f62, 1b942e8, 376d47c). The look is "Ink & Trail" (735546b); see [Review UX](review-ux.md) for behaviour. Check the cited code before trusting a number here.

## Tokens live in one place

Components read colour roles from `Colors` and the three radii from `src/app/components/palette.rs`. They never read the colour ladders, and a hardcoded colour or radius in a component is a bug. To change a look everywhere, change the role or the shared component (`button`, `card`, `chip`, `callout`, `notice_band`, `key_cap`, `sidebar_row`, `note_label`), not a call site.

## Radii

Three, so corners agree: `CARD_RADIUS` 12 for cards, file boxes, callouts and sheets; `CONTROL_RADIUS` 8 for buttons, fields, rows and segmented tracks; `INNER_RADIUS` 6 for what sits inside a control or is too small for 8 (a segment's thumb, a chip, a key cap). `rounded_full` is for dots, discs and meters only, not for labels: a pill around a label is the tell 1775047 removed.

gpui clips children to a rectangle, never to the parent's rounded corners, even with `overflow_hidden`. So a child that paints its own ground along the edge of a rounded surface (the Settings sheet's action bar, a picker's key footer, a suggested change's head, a diff box's rows) rounds the corners it touches itself, with `inside_edge(radius)` from `palette.rs`: the surface's radius less its 1 px border. Without it the child's ground shows as a square patch past the curve, as the Settings sheet's foot and the pickers' footers did in both appearances.

## Spacing and edges

The scale is gpui's 4 px steps. What lines up:

- The page column has 24 px sides (`px_6`) and reads at most `READING_WIDTH` (760) for prose. The title bar's PR name and its actions sit on the same 24 px edges (376d47c), and so do notice bands and the sticky step bar.
- The sidebar is 300 wide. Its header content sits 16 px in (`px_4`); clickable rows have their ground 8 px in (`mx_2`), and so does Settings at its foot, whose icon then lands on the header's 16 px edge (376d47c).
- Two columns side by side start level: the summary's column beside the tour begins on the same line as the page's place label (1b942e8).

## Border or ground

One edge per level. A thing gets a border or a change of ground, rarely both, and never a box inside a box inside a box.

- Cards on the page (`card`): `surface` ground, `subtle_border`, a hairline lift in the light. Inside a card, rows split by ground or a hairline, not by more cards.
- Overlays (palettes, the switcher, sheets, tooltips): `overlay` ground and `overlay_shadow`; what is inside them sits on that ground. A Settings section is a title and its controls on the sheet, split from the next by a hairline; it had been a card with an icon tile inside the sheet (b103e63).
- Bands under the title bar (`notice_band`) are a tone's ground with an edge below, across the page column.
- A control that can be pushed (`Secondary` button, segment thumb) is raised: `raised` gradient, `border`, `control_shadow`. A `Ghost` button has no edge until hovered.

On 2026-11-13 the maintainer asked the "Writing the tour" screen to read as one page: the sidebar column, the title bar and the page all on the page's `background` token (the review window's root ground is `background` instead of `sidebar_ground` while a tour is written), and no shadow under the header bar. That shadow was the review page underneath bleeding up into the title bar, so the page content is not built while the loading screen is up (commit 8b5a2fe).

## Accent

Iris is for what the reader acts on and where they are: the one `Primary` button of a screen (the way on), links, the focus ring, the current row, a toggle that is on. It is not for labels, headings or decoration. The trail gradient (iris to azure, green once done) is for progress only: the meter, the current step's dot, the busy sweep, the page line under the title bar. Green, red and amber are states (`Tone`): success, danger, warning; on this app's pages green also means added lines, so nothing else is green for looks. Inline code in prose is outlined in the text's own colour (0028f62) and set in the diff's monospaced font (2026-11-20, issue 156). Its ground is its own role, `Colors::inline_code` (`34363c` dark, `dfe3ef` light), since `surface` is one faint step from the page in the dark; a contrast test holds it at 1.3:1 (dark) and 1.2:1 (light) against the page and a card's `surface` (2026-11-20).

## Text

One family for text (`TEXT_FONT`) and one for code (`CODE_FONT`); every text names its family. The platform picks both ([Platform](platform.md)).

- `text_2xl` semibold: a screen's or a sheet's title.
- `text_base` semibold: a section title inside a sheet.
- `text_sm`: body, controls, sidebar rows. `Large` and `Regular` buttons use it; `Small` buttons `text_xs`.
- `note_label`: `text_xs` semibold capitals in `muted_text` (or a tone's text inside a callout). Every label over a heading, a section or a note uses it: a screen's place ("STEP 3 OF 11"), "THE TOUR", "REVIEWER SUMMARY", "PR DESCRIPTION", "WHAT TO CHECK", the shortcuts sheet's groups (1775047, 1b942e8).
- `muted_text` for secondary lines; `sidebar_text` a step quieter than `text`, so the page leads.
- `faint_text()`, `muted_text` at 60 %, for what is there to look up and not read day to day: only the version line above Settings at the foot of the sidebar (2026-11-12). It is a link with no link look at rest; under the mouse it turns `accent` with a pointer, and opens the release's page (`AppVersion::page`).

## Icons

Drawn icons from `Icon`, never emoji. Sizes follow the button size (`ButtonSize::icon_size`: 16, 14, 13); a free-standing icon beside `text_sm` is 14, beside `text_xs` 12 or 13. An icon has a job (what a button does, a state); one that only repeats the title next to it goes (b103e63).

A Markdown table (`PreparedTable`, drawn by `table_view`) is one bordered box: `subtle_border` hairlines between rows and columns, the header bold on `inset`, cells padded 12 by 4. It wraps inside its cells and never scrolls: a summary's table is prose, and a column scrolled out of sight hides what the reviewer is meant to read (code fences scroll because a line of code cannot be reflowed). Text is not measured, so column widths come from character counts: each column's basis is its longest cell, its floor its longest word, and the whole table is no wider than its cells need on one line. A long unbreakable token in a cell is the one thing that can still overflow.

## Known open

- The Settings IDE tiles still mark the chosen editor's glyph with the `icon_tile` gradient (`settings_panel/code.rs`); it is the gradient's last use.
- When the tour is older than the PR, the comment box's refusal ("Update tour to comment") may show as a second amber band under the new-commits band, which already says so (not checked since 2026-11-12).
- The sidebar's foot is one row of four icon buttons (changed files, comment threads, Settings, shortcuts), each with its key cap (`sidebar.rs`), above the version line.
- The sidebar's station minutes move only when the sidebar is drawn again (each sync try, each change), not on a timer (not checked since 2026-11-23).

## The title bar

- Nothing in the title bar is clipped off the window. The PR's name and title give way; every control to their right is a `flex_none` button and keeps its width. Order of giving way: the head branch first, then the base branch down to `BASE_BRANCH_MIN_WIDTH` (120, never more than its chip, so `main` leaves no gap), then the PR's title down to an ellipsis. `PR_TITLE_MIN_WIDTH` (290: icon, repo#number which never truncates, chevron, switch keys and a few title characters) is what the name and title keep (`title_bar.rs`; 6df63985, bf835350).
- Why flex layout and not a width estimate: the layout already knows every width. A character-count guess (`ButtonDetail`) and a clipped wrapping button (`giving_way`) were both tried on 2026-11-17 and removed: the first estimated widths in characters instead of measuring them, the second left an empty gap in the button's ground. Do not bring either back.
- The controls share one size: the PR status chip (`D`, toggle draft), the auto-merge button, Jump and GitHub are `button` at `ButtonSize::Small`, so height, padding, radius, font and key cap size match. Each shows its key cap after its label through `button_keys`, so a changed `[keys]` binding shows. The auto-merge button hugs its content (icon, label and, when auto-merge is on, the method word); its tooltip says "Click twice to confirm".
- The Merged and Closed chips (`closed_pr_chip`, scenes `72-title-bar-merged`, `73-title-bar-closed`) are strong flat fills with white content (`Colors::strong_tone`: `merged_fill` iris, `closed_fill` red, one step brighter in the dark). On 2026-11-20 the maintainer found the pale tint too quiet next to GitHub's purple badge. The "Merging" chip shares the code. A contrast test holds white at 4.5:1 on each fill and the fill at 3:1 against the page. The tinted `Tone` stays for every other chip, so the states that end a PR's life are the only loud chips.

## Lists of touching rows

Every list of touching rows builds its rows from `touching_places` and `picker_row_at` (`overlay.rs`): the findings sheet, the jump palette and the PR switcher. A selected row and its hovered neighbour square the corners they share. A row with its body open, or with a heading between it and the next, has no neighbour on that side. The maintainer reported the rounded notch three times (2026-11-20); `CODING_STANDARDS.md` makes it a review rule.

## Scrollbars

One look, `scrollbar` in `src/app/components/scrollbar.rs` (2026-11-18): a 12 px track on the right edge with a 6 px thumb in its middle, `muted_text` at 30 %, 60 % under the pointer or while dragged. The page, the PR switcher and the issue card draw it. One drag, `ScrollbarDrag` over a `ScrollSource` (gpui's `ListState` or a `ScrollHandle`), serves all three. It listens to the whole window (`drag_listener`), because an element's `on_mouse_move` fires only while the pointer is over it, and the issue card floats above the page, so the page heard no moves while the card's scrollbar was dragged. The issue card stays open while its scrollbar is dragged and closes when the drag ends off it. Scene `76-issue-card-dragged` drags it for real, with a frame between press and move.

## The PR switcher

- Rows are grouped by repo (`PrSwitcher::group_visible`, #146, 2026-11-18): each repo's name once as a heading with its row count and a hairline above it, its PRs then its local reviews under it, the repos in the order of their newest row.
- A row is 56 px: the PR author's picture (`avatar_circle`, 30 px; their first letter while it loads or when there is none; GitHub's `avatarUrl`, kept in `pull-requests.json`) or a folder tile for a local review, then the title or branch over a quiet line of facts (#number, author, why listed; or "local review" and commits). States sit at the right edge in one fixed order: draft, open PR, disk state, agent replied, viewing, tour. A guided tour's chip carries the Sparkles icon; a basic tour is not named. The selected row is marked by its ground only.
- The switcher is a `gpui::list`, so it scrolls by pixels. `PrSwitcher::lines` gives every repo name and row, each drawn at a fixed height (rows 56, names `HEADING_HEIGHT` 34 with a 6 px gap above) so the view places them before gpui measures them. The list is up to eight rows and a name high and ends flush on its last row, with an 8 px gap below it so a selected row touches neither the next name nor the footer. The name of the repo at the top stays while its rows pass under it, and the next name pushes it up. When the list changes, the line under the pointer, or else the top one, stays where it was on screen (`anchor_at`, `line_after_change`). Keys scroll only as far as the selected row needs, below the stuck name. The footer says "Looking in your folders..." until the local rows are in.
- Finding the local rows took 16 s on the maintainer's Mac, since each folder's uncommitted files were counted by adding the whole work tree to a fresh index, one folder after another (2026-11-18). A glance now counts them with `git status` (`--no-optional-locks`, all untracked, no renames, the same files as a snapshot's diff), and the folders, the reviews on disk and the open-PR lookups each run on their own thread (`in_parallel` in `local_change_sets.rs`): 1.7 s. The rows are not kept across a restart.
- Snapshot runs give the switcher herdr without the reviewer's own terminals (`NoReviewerTerminals`), since the folders their Claude terminals work in may be private repos. The one real local row left is the checkout the run starts from.

## The sidebar's stations

Between the progress bar and the steps the sidebar is one stepper of stations (`Stations` in `src/viewer/stations.rs`, drawn by `stations_view.rs`; #174, #157, 2026-11-23): Tour, Review, Fixes, You, GitHub as round icons in equal columns joined by 2 px rails, each icon in its health's tone (failed red, waiting amber, working accent, done green, not started muted), no edges. One card under it, raised on `surface`, opens on the station that failed, else waits, else works, else the AI review; a click on an icon picks another. Every other station that failed or waits gets one line under the card, so a GitHub problem never hides behind a colour. Problems sit in their station's card (`Stations::home_of`); those about this Mac stay in a red box above the stepper. The GitHub station says how many viewed marks wait and for how long; the start time is kept in the review state (`viewed_waiting_since`), so a restart keeps it. The design was picked from six prototypes.

A station's card head (2026-11-27) is the tile and one wrapping line: the name, and the state at the right while both fit, else under the name. A long state ("90 viewed marks waiting · 12 min" next to "Code host") ran over the name when the state was a shrinking box with right-aligned text. Every station uses the one card. Scene `101-sidebar-github-card-long-state` shows it; `95-sidebar-stations-busy` stages the same long state.
