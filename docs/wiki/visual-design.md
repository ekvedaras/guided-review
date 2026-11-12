# Visual design rules

2026-11-12. The rules the app's look follows, read from the code and from a design pass over every snapshot scene in both appearances (commits b103e63, 1775047, 0028f62, 1b942e8, 376d47c). The look itself is "Ink & Trail" (735546b); see [Review UX](review-ux.md) for behaviour. Check the cited code before trusting a number here.

## Tokens live in one place

Components read colour roles from `Colors` and the three radii from `src/app/components/palette.rs`. They never read the colour ladders, and a hardcoded colour or radius in a component is a bug. To change a look everywhere, change the role or the shared component (`button`, `card`, `chip`, `callout`, `notice_band`, `key_cap`, `sidebar_row`, `note_label`), not a call site.

## Radii

Three, so corners agree: `CARD_RADIUS` 12 for cards, file boxes, callouts and sheets; `CONTROL_RADIUS` 8 for buttons, fields, rows and segmented tracks; `INNER_RADIUS` 6 for what sits inside a control or is too small for 8 (a segment's thumb, a chip, a key cap). `rounded_full` is for dots, discs and meters only, not for labels: a pill around a label is the tell 1775047 removed.

2026-11-12: gpui clips children to a rectangle, never to the parent's rounded corners, even with `overflow_hidden`. So a child that paints its own ground along the edge of a rounded surface (the Settings sheet's action bar, a picker's key footer, a suggested change's head, a diff box's rows) rounds the corners it touches itself, with `inside_edge(radius)` from `palette.rs`: the surface's radius less its 1 px border. Without it the child's ground shows as a square patch past the curve, as the Settings sheet's foot and the pickers' footers did in both appearances.

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

## Accent

Iris is for what the reader acts on and where they are: the one `Primary` button of a screen (the way on), links, the focus ring, the current row, a toggle that is on, Ask. It is not for labels, headings or decoration. The trail gradient (iris to azure, green once done) is for progress only: the meter, the current step's dot, the busy sweep, the page line under the title bar. Green, red and amber are states (`Tone`): success, danger, warning; on this app's pages green also means added lines, so nothing else is green for looks. Inline code in prose is outlined in the text's own colour (0028f62).

## Text

One family for text (`TEXT_FONT`) and one for code (`CODE_FONT`); every text names its family.

- `text_2xl` semibold: a screen's or a sheet's title.
- `text_base` semibold: a section title inside a sheet.
- `text_sm`: body, controls, sidebar rows. `Large` and `Regular` buttons use it; `Small` buttons `text_xs`.
- `note_label`: `text_xs` semibold capitals in `muted_text` (or a tone's text inside a callout). Every label over a heading, a section or a note uses it: a screen's place ("STEP 3 OF 11"), "THE TOUR", "REVIEWER SUMMARY", "PR DESCRIPTION", "WHAT TO CHECK", the shortcuts sheet's groups (1775047, 1b942e8).
- `muted_text` for secondary lines; `sidebar_text` a step quieter than `text`, so the page leads.

## Icons

Drawn icons from `Icon`, never emoji. Sizes follow the button size (`ButtonSize::icon_size`: 16, 14, 13); a free-standing icon beside `text_sm` is 14, beside `text_xs` 12 or 13. An icon has a job (what a button does, a state); one that only repeats the title next to it goes (b103e63).

## Known open

- The Settings IDE tiles still mark the chosen editor's glyph with the `icon_tile` gradient; it is the gradient's last use.
- A step screen can show two Ask buttons at once: the toolbar's (the whole step) and a file header's (that file). They ask about different things, but read as one action twice.
- When the tour is older than the PR, the comment box's refusal ("Update tour to comment") shows as a second amber band under the new-commits band, which already says so.
- The sidebar's foot puts two key-cap clusters side by side (Settings ⌘ , and the shortcuts sheet's ?).
