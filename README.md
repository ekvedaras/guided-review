# guided-review

Turn a PR diff into a top-down guided review. An agent writes a tour of a GitHub PR: a summary, then steps from the main flow down to the details, each with the diff pieces and diagrams it talks about. The app shows the tour next to the diff and lets you comment on it.

## Requirements

- macOS (the only platform tested; CI runs on macOS).
- A Rust toolchain, stable (edition 2024), only to build from source.
- [`gh`](https://cli.github.com), logged in. The app reads the token from `gh auth token`.
- [Claude Code](https://claude.com/claude-code) (`claude`), which writes the tours.
- [herdr](https://herdr.dev) (`hc` and `herdr-app`), only for the agent features: the PR switcher starting a tour writer, comments reaching the tour's agent, Refresh. Without herdr the app turns those features off and says why; opening and reading an existing tour does not need it.

## Install

### Prebuilt (no Rust needed)

Download the archive for your Mac from the [latest release](https://github.com/ekvedaras/guided-review/releases/latest): `aarch64-apple-darwin` for Apple Silicon, `x86_64-apple-darwin` for Intel. Then:

```sh
VERSION=0.1.0 TARGET=aarch64-apple-darwin   # the release you downloaded
shasum -a 256 -c guided-review-$VERSION-$TARGET.tar.gz.sha256   # optional
tar xzf guided-review-$VERSION-$TARGET.tar.gz
mkdir -p ~/.local/bin
mv guided-review-$VERSION-$TARGET/guided-review ~/.local/bin/
```

`~/.local/bin` must be on `PATH`, also for agents the app starts.

The binary is not signed or notarized, so macOS Gatekeeper blocks a downloaded copy. Remove the quarantine flag once:

```sh
xattr -d com.apple.quarantine ~/.local/bin/guided-review
```

Link the tour skill from the unpacked folder so Claude Code finds `/tour`. Keep that folder, the link points into it:

```sh
mkdir -p ~/.claude/skills
ln -s "$PWD/guided-review-$VERSION-$TARGET/skills/tour" ~/.claude/skills/tour
```

The CLI embeds the skill, so keep both from the same release.

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

The skill writes the tour with the `guided-review` CLI and opens it. To open an existing tour:

```sh
guided-review open <pr-url>
```

Tours and app state live under `~/.guided-review/`. `guided-review --help` lists the other commands (`outline`, `show`, `check`, `repin`, `refresh`, `eval`).

## How it works

[GLOSSARY.md](GLOSSARY.md) defines the words (tour, step, piece). [docs/wiki](docs/wiki/index.md) records the decisions and why.

## Contributing

Read [CODING_STANDARDS.md](CODING_STANDARDS.md) first. The [wiki](docs/wiki/index.md) records decisions. `scripts/check` is the gate before every commit.
