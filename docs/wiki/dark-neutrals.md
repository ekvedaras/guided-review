# Dark neutrals

2026-11-12, commit c0ac79a. Code: `Colors::dark` in `src/app/components/palette.rs`.

## Dark neutrals

Dark greys carried a blue cast (OKLCH chroma about 0.025 to 0.04 at the brand's hue) where light mode's greys sit at about 0.007 to 0.017. Each dark neutral kept its lightness and its chroma was cut to 30 percent (grounds, surface, overlay, raised, inset, hover, pressed, borders, text, sidebar text, muted text, code comment), so contrast against the grounds is unchanged. The quiet page wash stays a faint cool tint on purpose (`page_wash`). Accent, state and trail colours and the `ink` ladder (which light mode shares) are untouched; dark roles hold their own values instead of reading `ink` steps.

The 30 percent figure is one number from one script run, judged by the maintainer by hand. Another step is a cheap change per role.

## The Ask panel's look (removed)

The Ask panel was removed on 2026-11-27 ([agent-threads.md](agent-threads.md)); `ask_panel.rs` is gone. Two rules from its look still apply to any panel that sits beside the page:

- It lies on the page as the page lies on the sidebar (38a3dd5): rounded corners on the side that faces the page (`CARD_RADIUS`), a hairline `subtle_border` and the leftwards `page_shadow`, one shadow for every such layer. Because gpui clips children to a rectangle, a wrapper with the page's `background` goes under the rounded corners, so they cut into the page and not into the sidebar's ground.
- Rejected: letting the page run under the panel with a negative margin. It hides the page's right edge, scrollbars included.
