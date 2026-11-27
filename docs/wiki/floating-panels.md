# Floating panels

2026-11-18. `gpui::deferred` paints a panel on top of the page, but gpui still sends hover, moves and the scroll wheel to what is under it unless the panel occludes. Six panels leaked this way, one after another, until the rule below (the PR switcher 6f93d20, the jump palette and loading screen 5b1a8bd, then the checks list, the issue list, the reaction picker and the issue card, where scrolling the card scrolled the page; #133).

## The decision

- `overlay::popover(panel)` in `src/app/components/overlay.rs` is the one way to build a panel that floats beside what opens it. It wraps the panel in `deferred(anchored().snap_to_window_with_margin(8px))` around an occluding `div`. `popover_at(position, panel)` is the same with its top left corner at a point in the window. `popover_by(word, panel)` puts the panel under a word in drawn text, or over it when the window has no room below; the Definition card uses it.
- `clippy.toml` lists `gpui::deferred` under `disallowed-methods`; only `popover`, `popover_at` and `popover_by` carry the `#[expect]`. `scripts/check` runs clippy with `-D warnings`, so a bare `deferred` fails the gate.
- Layers that cover the window (`modal_layer`, `modal_layer_for`, `input_blocker`) already occlude and stay as they are.
- A panel that needs another anchor than these three give should add a function beside them, not call `deferred` itself.
