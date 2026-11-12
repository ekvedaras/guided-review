# guided-review

Turn a PR diff into a top-down guided review. An agent writes a tour of a GitHub PR: a summary, then steps from the main flow down to the details, each with the diff pieces and diagrams it talks about. The app shows the tour next to the diff and lets you comment on it.

> [!NOTE]
> Mandatory `vibe` alert! I do not have the capacity to review all of this code. The irony here is not lost on me 🙃 I do care, PRs and issues are welcome!

## Install with your agent

Paste this into Claude Code (or any coding agent with a shell):

```text
Install guided-review (https://github.com/ekvedaras/guided-review) for me from its latest GitHub release. Do it yourself, one step at a time, and stop and tell me if any step fails:
1. Find my Mac's architecture with `uname -m`: arm64 means target aarch64-apple-darwin, x86_64 means x86_64-apple-darwin. Stop if this is not macOS.
2. Find the latest release and its assets: `gh release view --repo ekvedaras/guided-review --json tagName,assets` (or, without gh, https://api.github.com/repos/ekvedaras/guided-review/releases/latest). Pick guided-review-<version>-<target>.tar.gz and its .sha256.
3. Download both into a new empty folder ~/.local/share/guided-review/<version>/, check the archive with `shasum -a 256 -c <archive>.sha256`, and unpack it there with `tar xzf`.
4. Link the binary: `mkdir -p ~/.local/bin` and `ln -sf` the unpacked guided-review binary to ~/.local/bin/guided-review. Remove the macOS quarantine flag from the unpacked binary with `xattr -d com.apple.quarantine <path>` (ignore "No such xattr").
5. Link the tour skill: point ~/.claude/skills/tour at the unpacked skills/tour folder with `ln -sfn`. If ~/.claude/skills/tour exists and is a real folder rather than a link, ask me before replacing it.
6. Put the app in ~/Applications: run the unpacked binary with `install-app`. It makes ~/Applications/Guided Review.app from the release's bundle, for the Dock, Spotlight and Raycast.
7. If ~/.local/bin is not on my PATH, tell me which line to add to my shell profile; do not edit it yourself.
8. Run `guided-review --version` and tell me the installed version.
```

After that, the app updates itself: when a newer release is out, an Update button shows in the sidebar, above Settings. One click downloads it, checks it, moves the links to it, refreshes Guided Review.app and restarts the app. Each version keeps its own folder, so the old one is still there.

Guided Review.app starts the app from the Dock, Spotlight or Raycast and shows the PR switcher to pick a PR. It runs the app through your login shell, so `gh`, `claude` and `hc` are found as in a terminal.

Other ways to install, and what it needs: [Requirements](#requirements) and [Install](#install).

## Tour

The tour of [ekvedaras/guided-review#12](https://github.com/ekvedaras/guided-review/pull/12), the PR that added the Settings page.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/tour-summary-dark.png">
  <img alt="The tour summary with the PR description beside it and the list of steps on the left" src="docs/assets/tour-summary.png">
</picture>

The summary and the PR description beside it. The sidebar lists the steps and how far you have read.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/tour-step-diagram-dark.png">
  <img alt="A step with a diagram and the diff it explains" src="docs/assets/tour-step-diagram.png">
</picture>

A step: what to check, a diagram, then the diff pieces it talks about. Changed words are marked inside rewritten lines.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/tour-step-thread-dark.png">
  <img alt="A comment box inside the diff" src="docs/assets/tour-step-thread.png">
</picture>

Comments and GitHub review threads sit inline in the diff, in a box that splits it. Add a comment to your review, send it now, or hand it to the tour's agent.

## Requirements

- macOS (the only platform tested; CI runs on macOS).
- A Rust toolchain, stable (edition 2024), only to build from source.
- [`gh`](https://cli.github.com), logged in. The app reads the token from `gh auth token`.
- [Claude Code](https://claude.com/claude-code) (`claude`), which writes the tours.
- [herdr](https://herdr.dev) (`hc` and `herdr-app`), only for the agent features: the PR switcher starting a tour writer, comments reaching the tour's agent, Refresh. Without herdr the app turns those features off and says why; opening and reading an existing tour does not need it.

## Install

### Prebuilt (no Rust needed)

Download the archive for your Mac from the [latest release](https://github.com/ekvedaras/guided-review/releases/latest): `aarch64-apple-darwin` for Apple Silicon, `x86_64-apple-darwin` for Intel. Unpack it into its own version folder and link the binary and the tour skill into it. The app updates itself only when it is installed this way:

```sh
VERSION=0.4.0 TARGET=aarch64-apple-darwin   # the release you downloaded
shasum -a 256 -c guided-review-$VERSION-$TARGET.tar.gz.sha256   # optional
mkdir -p ~/.local/share/guided-review/$VERSION ~/.local/bin ~/.claude/skills
tar xzf guided-review-$VERSION-$TARGET.tar.gz -C ~/.local/share/guided-review/$VERSION
RELEASE=~/.local/share/guided-review/$VERSION/guided-review-$VERSION-$TARGET
ln -sf "$RELEASE/guided-review" ~/.local/bin/guided-review
ln -sfn "$RELEASE/skills/tour" ~/.claude/skills/tour
"$RELEASE/guided-review" install-app   # ~/Applications/Guided Review.app
```

`~/.local/bin` must be on `PATH`, also for agents the app starts.

The binary is not signed or notarized, so macOS Gatekeeper blocks a downloaded copy. Remove the quarantine flag once:

```sh
xattr -d com.apple.quarantine "$RELEASE/guided-review"
```

The CLI embeds the skill, so keep both from the same release. An update the app installs needs no quarantine step: macOS puts the flag only on files that browsers and similar apps download.

### From source

```sh
cargo install --path .
```

The `guided-review` binary must be on `PATH`, also for agents the app starts.

Link the tour skill so Claude Code finds `/tour`:

```sh
ln -s "$PWD/skills/tour" ~/.claude/skills/tour
```

The CLI embeds the skill, so keep both from the same checkout.

## Use

In Claude Code:

```
/tour <pr-url>
```

The skill writes the tour with the `guided-review` CLI and opens it. To open a PR in the app:

```sh
guided-review open <pr-url>
```

A PR with no tour yet gets one written, as when you pick it in the PR switcher: the app shows the writer's progress and opens the tour when it is ready (needs herdr).

In the app, `→` or `J` (the Next button) goes on to the next step not read yet, `N` jumps to the next unread step from anywhere, `M` marks a step read, `⌘K` jumps to a step, file or action (Settings, Finish review, Restart app here, which `guided-review restart` also does), `?` lists every shortcut and `⌘W` closes the window and quits the app. With summary instructions set in Settings, the tour can also carry a reviewer summary; pictures it shows sit in the tour's own folder.

Tours and app state live under `~/.guided-review/`. `guided-review --help` lists the other commands (`outline`, `show`, `check`, `repin`, `refresh`, `settings`, `restart`, `eval`).

## How it works

[GLOSSARY.md](GLOSSARY.md) defines the words (tour, step, piece). [docs/wiki](docs/wiki/index.md) records the decisions and why.

## Contributing

Read [CODING_STANDARDS.md](CODING_STANDARDS.md) first. The [wiki](docs/wiki/index.md) records decisions. `scripts/check` is the gate before every commit.
