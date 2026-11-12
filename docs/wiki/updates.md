# Updates

2026-11-11. The app looks for a newer release and installs it with one click, then restarts into it. Code: `src/update.rs`, `src/update/release_install.rs`, `src/app/updater.rs`.

## Only a release install updates itself

The app offers an update only when it runs as a release install: the binary that `~/.local/bin/guided-review` links to, inside `~/.local/share/guided-review/<version>/`. That is the layout the README's install prompt makes, and the manual install in the README makes it too.

A binary built from source (`cargo install`) never sees an update. Its version can be ahead of the latest release, and swapping it for a release would throw away the build the maintainer is trying. A binary moved into `~/.local/bin` by hand has no link to move, so it is not updated either.

## A guided-review in the release install's way

2026-11-12. With a release installed, another `guided-review` can keep its updates from the reviewer: the app runs it instead (`InTheWay::Runs`), or it comes before `~/.local/bin` on `PATH` (`InTheWay::FirstOnPath`), so a terminal and the tour skill run it. Each review that opens looks this up again and lists it as a problem (`Problem::ReleaseInstallInTheWay`) naming the binary to remove, so removing it and opening a review clears it. With no release installed nothing is in the way, so a source-only setup sees nothing.

It shows on purpose while the maintainer's own source build in `~/.cargo/bin` is in the way: that is the same situation a reviewer with an old `cargo install` is in. A `guided-review` on `PATH` that links to the release install is not in its way. A `PATH` without any `guided-review` is not looked at.

## An update moves links; it never writes over the running binary

The new release unpacks into its own folder beside the old one, then `~/.local/bin/guided-review` and `~/.claude/skills/tour` are pointed at it. Each link is replaced by renaming a new link over it, so there is no moment with no link. The running binary is not touched, so the app keeps working until it restarts, and the old version's folder stays for a manual way back.

The skill link moves only when it already links into the release folders. A real folder there is the reviewer's own, so it is left alone and a toast says so.

Rejected: writing over the binary in place. On macOS, writing into a signed binary the kernel has open can kill the process, and a failed write leaves no working binary.

## Checks

The first check is 30 seconds after the app starts, then every 6 hours. A check reads `releases/latest` of the app's repository. GitHub leaves drafts and pre-releases out of it, so a release is offered only once it is published from the draft the release workflow makes. A failed check only writes to the app's log.

The archive is checked against its `.sha256` file before anything on disk changes. Downloads send no token: release files are public and GitHub serves them from another host.

## Restart

Once installed, the app restarts the way Restart app here does, but runs the new release's own binary by its full path in the version folder. The restart guard still holds: with an Ask or hand-off in flight, the first restart only warns, and the button then reads Restart for the version, to restart anyway.

2026-11-12: the restart ran the bin link until then. It now runs the exact binary the install unpacked, never the bin link or a `guided-review` looked up on `PATH`, since by the time the restart runs (it may wait on the restart guard) either may lead to another binary.
