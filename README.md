# guided-review

Turn a PR diff into a top-down guided review. An agent writes a tour of a GitHub PR: a summary, then steps from the main flow down to the details, each with the diff pieces and diagrams it talks about. The app shows the tour next to the diff and lets you comment on it.

> [!NOTE]
> Mandatory `vibe` alert! I do not have the capacity to review all of this code. The irony here is not lost on me 🙃 I do care, PRs and issues are welcome!

## Install with your agent

Paste this into Claude Code (or any coding agent with a shell):

```text
Install guided-review (https://github.com/ekvedaras/guided-review) for me from its latest GitHub release. Do it yourself, one step at a time, and stop and tell me if any step fails:
1. Find my OS and CPU with `uname -sm`: `Darwin arm64` means target aarch64-apple-darwin, `Linux x86_64` means x86_64-unknown-linux-gnu. Stop on anything else.
2. Check what it needs and tell me what is missing, but go on: `gh auth status` (gh, logged in), `command -v claude` (Claude Code) and `command -v hc` (herdr, only for the agent features).
3. Find the latest release and its assets: `gh release view --repo ekvedaras/guided-review --json tagName,assets` (or, without gh, https://api.github.com/repos/ekvedaras/guided-review/releases/latest). The version is the tag without its leading v. Pick guided-review-<version>-<target>.tar.gz and guided-review-<version>-<target>.tar.gz.sha256.
4. Make the folder ~/.local/share/guided-review/<version>/ and, inside it, download both files, check the archive with `shasum -a 256 -c <archive>.sha256` and unpack it with `tar xzf <archive>`. It unpacks to guided-review-<version>-<target>/, holding the guided-review binary, skills/tour and the files of the Desktop entry; that folder is the release folder below.
5. Link the binary: `mkdir -p ~/.local/bin`, then `ln -sf <release folder>/guided-review ~/.local/bin/guided-review`. On macOS, remove the quarantine flag from that binary with `xattr -d com.apple.quarantine <path>` (ignore "No such xattr").
6. Link the tour skill: `mkdir -p ~/.claude/skills`, then `ln -sfn <release folder>/skills/tour ~/.claude/skills/tour`. If ~/.claude/skills/tour exists and is a real folder rather than a link, ask me before replacing it.
7. Put the app in my app launcher: run `<release folder>/guided-review install-app`. It puts the Desktop entry in place and says where (on macOS ~/Applications/Guided Review.app, on Linux a desktop file in ~/.local/share/applications).
8. Run `which -a guided-review`. If ~/.local/bin is not on my PATH, tell me which line to add to my shell profile; do not edit it yourself. If another guided-review comes before ~/.local/bin/guided-review (an old `cargo install` in ~/.cargo/bin, say), tell me: it would run instead of this install and never get its updates.
9. Run `~/.local/bin/guided-review --version` and tell me the installed version and how to start: Guided Review from my app launcher, or `/tour <pr-url>` in Claude Code.
```

After that, the app updates itself: when a newer release is out, an Update button shows in the sidebar, above Settings. One click downloads it, checks it, moves the links to it, refreshes the Desktop entry and restarts the app. Each version keeps its own folder, so the old one is still there.

The Desktop entry starts the app from your app launcher (Spotlight, Raycast and the Dock on macOS; any launcher that reads desktop files on Linux) and shows the PR switcher to pick a PR. It runs the app through your login shell, so `gh`, `claude` and `hc` are found as in a terminal.

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

- macOS on Apple Silicon, or Linux on x86_64 with a Vulkan driver and glibc 2.39 or newer (Ubuntu 24.04 or later). CI runs the gate on macOS and Ubuntu. On Linux the app is used on Arch with Hyprland (Wayland); X11 and other desktops are built but untried.
- A Rust toolchain, stable (edition 2024), only to build from source.
- [`gh`](https://cli.github.com), logged in. The app reads the token from `gh auth token`.
- [Claude Code](https://claude.com/claude-code) (`claude`), which writes the tours.
- [herdr](https://herdr.dev) (`hc` and `herdr-app`), only for the agent features: the PR switcher starting a tour writer, comments reaching the tour's agent, Refresh. Without herdr the app turns those features off and says why; opening and reading an existing tour does not need it.

## Install

### Prebuilt (no Rust needed)

Each release has an archive per OS and CPU: `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`. Unpack it into its own version folder and link the binary and the tour skill into it. The app updates itself only when it is installed this way:

```sh
case "$(uname -sm)" in
  "Darwin arm64")  TARGET=aarch64-apple-darwin ;;
  "Linux x86_64")  TARGET=x86_64-unknown-linux-gnu ;;
  *) echo "No release for $(uname -sm); build from source" ;;
esac
VERSION=$(gh release view --repo ekvedaras/guided-review --json tagName -q .tagName | sed 's/^v//')
ARCHIVE=guided-review-$VERSION-$TARGET.tar.gz
mkdir -p ~/.local/share/guided-review/$VERSION ~/.local/bin ~/.claude/skills
cd ~/.local/share/guided-review/$VERSION
gh release download v$VERSION --repo ekvedaras/guided-review --pattern "$ARCHIVE*"
shasum -a 256 -c $ARCHIVE.sha256
tar xzf $ARCHIVE
RELEASE=$PWD/guided-review-$VERSION-$TARGET
ln -sf "$RELEASE/guided-review" ~/.local/bin/guided-review
ln -sfn "$RELEASE/skills/tour" ~/.claude/skills/tour
"$RELEASE/guided-review" install-app   # the Desktop entry, for your app launcher
```

`~/.local/bin` must be on `PATH`, also for agents the app starts, and come before any other `guided-review` (check with `which -a guided-review`). One found first, such as an old `cargo install` in `~/.cargo/bin`, runs instead of the release install and never gets its updates; the app names it as a problem until it is removed.

`install-app` puts the Desktop entry in place: `~/Applications/Guided Review.app` on macOS; `io.github.ekvedaras.guided-review.desktop` in `~/.local/share/applications` and its icons in `~/.local/share/icons/hicolor` on Linux. The app keeps its config, tours and state in the XDG folders on every OS ([Use](#use)).

macOS only: the binary is not signed or notarized. Files downloaded with `gh` or `curl` carry no quarantine flag, but if you downloaded the archive with a browser, macOS Gatekeeper blocks it until you remove the flag once:

```sh
xattr -d com.apple.quarantine "$RELEASE/guided-review"
```

The CLI embeds the skill, so keep both from the same release. An update the app installs needs no quarantine step.

### From source

Linux only: the build needs the libraries gpui builds against. On Debian or Ubuntu, `scripts/linux-build-packages` installs them; on Arch they are `wayland libxkbcommon-x11 fontconfig vulkan-icd-loader`.

```sh
git clone https://github.com/ekvedaras/guided-review && cd guided-review
cargo install --path . --locked
mkdir -p ~/.claude/skills
ln -sfn "$PWD/skills/tour" ~/.claude/skills/tour   # so Claude Code finds /tour
```

The `guided-review` binary (in `~/.cargo/bin`) must be on `PATH`, also for agents the app starts. The CLI embeds the skill, so keep both from the same checkout.

A build from source never updates itself and has no Desktop entry (`install-app` works only for a release install): start the app with `guided-review` or `guided-review open <pr-url>`. Do not keep it beside a release install, or the app warns that it is in the release install's way.

## Use

In Claude Code:

```
/tour <pr-url>
```

The skill writes the tour with the `guided-review` CLI and opens it. `guided-review` with no command, like the Desktop entry, shows the PR switcher. To open a PR in the app:

```sh
guided-review open <pr-url>
```

A PR with no tour yet gets one written, as when you pick it in the PR switcher: the app shows the writer's progress and opens the tour when it is ready (needs herdr).

In the app, `→` or `J` (the Next button) goes on to the next step not read yet, `N` jumps to the next unread step from anywhere, `M` marks a step read, `Primary+K` jumps to a step, file or action (Settings, Finish review, Restart app here, which `guided-review restart` also does), `G` opens the PR on GitHub, `?` lists every shortcut and `Primary+W` or `Primary+Q` quits the app (Primary is Cmd on macOS and Ctrl on Linux). With summary instructions set in Settings, the tour can also carry a reviewer summary; pictures it shows sit in the tour's own folder.

The config (`config.toml`, `summary.md`) lives in `~/.config/guided-review/`; tours, review state, Asks, evals and the app log in `~/.local/state/guided-review/`; image previews in `~/.cache/guided-review/`. `XDG_CONFIG_HOME`, `XDG_STATE_HOME` and `XDG_CACHE_HOME` move them, and `GUIDED_REVIEW_TOURS_DIR` moves the tours alone (`guided-review tour-path <pr-url>` prints where a tour is). An earlier version's `~/.guided-review/` is moved there on the first start of the release install (of a build from source when there is no release install); a file both places hold stays in both, and the app names it. `guided-review --help` lists the other commands (`outline`, `tour-path`, `show`, `check`, `repin`, `refresh`, `settings`, `restart`, `install-app`, `agent-reply`, `eval`).

## How it works

[GLOSSARY.md](GLOSSARY.md) defines the words (tour, step, piece). [docs/wiki](docs/wiki/index.md) records the decisions and why.

## Contributing

Read [CODING_STANDARDS.md](CODING_STANDARDS.md) first. The [wiki](docs/wiki/index.md) records decisions. `scripts/check` is the gate before every commit.
