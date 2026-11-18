# Floating panels (2026-11-18)

## What went wrong

`gpui::deferred` paints a panel on top of the page, but gpui still sends hover, moves and the scroll wheel to what is under it unless the panel occludes. Each new popup that forgot `.occlude()` leaked: the PR switcher (6f93d20), the jump palette and loading screen (5b1a8bd), then the checks list, the issue list, the reaction picker and the issue card, where scrolling the card scrolled the page (#133).

## The decision

- `overlay::popover(panel)` in `src/app/components/overlay.rs` is the one way to build a panel that floats beside what opens it. It always does `deferred(anchored().snap_to_window_with_margin(8px))` around an occluding wrapper.
- `clippy.toml` lists `gpui::deferred` under `disallowed-methods`; only `popover` carries the `#[expect]`. `scripts/check` runs clippy with `-D warnings`, so a bare `deferred` fails the gate.
- Layers that cover the window (`modal_layer`, `modal_layer_for`, `input_blocker`) already occlude and stay as they are.

## Open

- A panel that needs another anchor position than `snap_to_window_with_margin` should add a parameter to `popover`, not call `deferred` itself.
