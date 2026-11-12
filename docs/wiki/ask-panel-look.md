# Ask panel look and dark neutrals

2026-11-12. Commit c0ac79a. Code: `src/app/components/ask_panel.rs`, `Colors::dark` in `src/app/components/palette.rs`.

## The panel lies on the page, as the page lies on the sidebar

Commit 38a3dd5 put the page on top of the sidebar. The Ask panel does the same on its side: rounded left corners (`CARD_RADIUS`), the same hairline `subtle_border` on its left edge and the same leftwards `page_shadow`, which is why that shadow's doc now names both. One shadow for both keeps the stack of layers reading as one idea.

The panel is a row sibling of the page, so behind its rounded corners would be the window's sidebar ground. A wrapper with the page's `background` sits under it, so the corners cut into the page. The slide-in moves the panel inside that wrapper.

Rejected: letting the page run under the panel with a negative margin. It would hide the page's right edge, scrollbars included.

## Less plain, without decoration

The header lost its band and its bottom rule and shows an accent sparkle, "Ask" in semibold and the place in muted text. The empty state (no Asks yet, or Ask unavailable) is centred in the body with a sparkle tile, a title and one muted line; the unavailable reason moved there from the foot, where it looked like a stray error. The box is taller and has a key hint under it (enter, esc) using the app's own `key_hint`. The top rule above the input is gone. No gradients, glows or extra borders were added.

## Dark neutrals

Dark greys carried a blue cast (OKLCH chroma about 0.025 to 0.04 at hue of the brand) where light mode's greys sit at about 0.007 to 0.017. Each dark neutral kept its lightness and its chroma was cut to 30 percent (grounds, surface, overlay, raised, inset, hover, pressed, borders, text, sidebar text, muted text, code comment), so contrast against the grounds is unchanged. The quiet page wash stays a faint cool tint on purpose. Accent, state and trail colours and the `ink` ladder (which light mode shares) are untouched; dark roles now hold their own values instead of reading `ink` steps.

Open: the maintainer judges the result by hand; the 30 percent figure is one number in one script run, so another step is a cheap change per role.
