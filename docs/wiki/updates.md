# Updates

2026-11-11. The app looks for a newer release and installs it with one click, then restarts into it. Code: `src/update.rs`, `src/update/release_install.rs`, `src/app/updater.rs`.

## Only a release install updates itself

The app offers an update only when it runs as a release install: the binary that `~/.local/bin/guided-review` links to, inside `~/.local/share/guided-review/<version>/`. That is the layout the README's install prompt makes, and the manual install in the README makes it too.

A binary built from source (`cargo install`) never sees an update. Its version can be ahead of the latest release, and swapping it for a release would throw away the build the maintainer is trying. A binary moved into `~/.local/bin` by hand has no link to move, so it is not updated either.

Which build runs is one value, `Build` (`src/build.rs`): `Release`, `Local` or `Test` (the snapshot build). `update::running_build()` looks it up once (2026-11-18, #143). The `DEV` badge, the Dock icon (`AppIcon::of(build, look)`) and window titles (`Build::marked_title`) all read it. Before, the Dock icon and titles asked `cfg!(debug_assertions)`, which a `cargo install` release build fails, so a trial build looked like the release in the Dock. Only `Local` wears the Dock badge: the snapshot build stays out of the Dock.

## A guided-review in the release install's way

2026-11-12. With a release install running, another `guided-review` can come before `~/.local/bin` on `PATH`, so a terminal and the tour skill run it and the reviewer's updates never reach them (`InTheWay::FirstOnPath`). Each review that opens looks this up again and lists it as a problem (`Problem::ReleaseInstallInTheWay`) naming the binary to remove. A `guided-review` on `PATH` that links to the release install is not in its way. A `PATH` without any `guided-review` is not looked at.

A local build that runs instead of the release install is not a problem since 2026-11-18 (38719b80): its `DEV` badge says so, and it was the maintainer's own normal setup.

## An update moves links; it never writes over the running binary

The new release unpacks into its own folder beside the old one, then `~/.local/bin/guided-review` and `~/.claude/skills/tour` are pointed at it. Each link is replaced by renaming a new link over it, so there is no moment with no link. The running binary is not touched, so the app keeps working until it restarts, and the old version's folder stays for a manual way back.

The skill link moves only when it already links into the release folders. A real folder there is the reviewer's own, so it is left alone and a toast says so.

Rejected: writing over the binary in place. On macOS, writing into a signed binary the kernel has open can kill the process, and a failed write leaves no working binary.

## Guided Review.app

From 0.7.0 (2026-11-12) a release archive carries `Guided Review.app` beside the binary: its Info.plist and icon files, no executable. The release workflow makes them once (`guided-review desktop-entry-template`, then `actool`) and packs the same copy into both macOS archives. Code: `src/platform/macos/app_bundle.rs`.

**The icon** (2026-11-16) is an Icon Composer icon (`assets/AppIcon.icon`). The release workflow compiles it with `actool` into `Assets.car` and a fallback `AppIcon.icns`, and the Info.plist names it with `CFBundleIconName`. macOS draws it in the reviewer's icon style (System Settings, Appearance, Icon & widget style), which the maintainer wants over following light or dark mode. `actool` reads `.icon` only from Xcode 26, so the `verify` job runs on `macos-26`.

A running app paints its own Dock picture only when Launch Services names no bundle with `CFBundleIconName` for it (`NSRunningApplication`, not `NSBundle::mainBundle`, which a Finder launch puts outside the bundle; probed 2026-11-16). Then it follows light or dark mode, as a `cargo install` binary does. That picture is the icon as Icon Composer draws it, light and dark, exported by `scripts/app-icon-pictures` (Icon Composer's `ictool`; Xcode's own `ictool` has no export) into `assets/app-icon-{light,dark}.png` and placed on macOS's 824-of-1024 icon grid. A flat picture drawn from the same layers had other corners and no glass shine or shadow, so it did not look like the bundle's icon. A test fails until the pictures are exported again after `AppIcon.icon` changes, by a digest the script writes beside them.

The trail's small dots are circles, not a dashed stroke: Icon Composer's glass shine follows a stroke's whole path, gaps too, and drew white lines between the dots on the light look.

**Placement.** `~/Applications/Guided Review.app` is a real folder, as Spotlight does not index a linked one. Its `Contents/MacOS/guided-review` is a link to `~/.local/bin/guided-review`, so the bundle always runs the release the bin link names. Its files are put in place one at a time by renaming a new file over the old one, as the links are. A file already the same is left alone, and a change touches the bundle folder so Launch Services reads the Info.plist again.

Three things put it in place: an update (before the bin link moves), `guided-review install-app` (the README's install), and the start of a release install whose bundle is missing or another version's. The last covers 0.6.0: its updater unpacks the whole archive with `tar::Archive::unpack` (checked in `v0.6.0:src/update/release_install.rs`), so the new version's folder holds the `.app`, but it moves only the bin and skill links. An updater that knows fewer bundle files (0.10.0 knows no `Assets.car`) leaves the new file out. The new release's first start puts it in place, as every start puts in place any bundle file that is missing or different.

**Launch from Finder or the Dock.** launchd gives a short `PATH`, without `gh`, `claude` or `hc`. So the app runs the bundle's executable again as `$SHELL -l -i -c 'exec "$0"' <bundle executable>` (zsh when `SHELL` is unset), marked with `GUIDED_REVIEW_IN_LOGIN_SHELL` so it does not loop. The bundle's path keeps the Dock naming the app Guided Review. A launch of the app is told by `__CFBundleIdentifier`, which Launch Services sets to the bundle id, not by the binary's path. The bundle's executable is a link to the bin link, and macOS follows it before it runs it, so the binary sees its real path outside the bundle (found 2026-11-13 with 0.7.0: the app opened from Finder had the short `PATH` and said `gh` was not logged in). With no command it shows the PR switcher, alone in the window when no PR is open (`AppWindow::SwitcherOnly`). When the CLI starts the app and the bundle's binary is the same file, it starts `serve` through the bundle path for the same Dock name. The Linux counterpart is in [Platform](platform.md).

## Checks

The first check is 30 seconds after the app starts, then every 6 hours (`src/app/updater.rs`). Check for updates is also a palette action. A check reads `releases/latest` of the app's repository. GitHub leaves drafts and pre-releases out of it, so a release is offered only once it is published from the draft the release workflow makes. A failed check only writes to the app's log.

The archive is checked against its `.sha256` file before anything on disk changes. Downloads send no token: release files are public and GitHub serves them from another host.

## Restart

Once installed, the app restarts the way Restart app does, but runs the new release's own binary by its full path in the version folder (2026-11-12). It never runs the bin link or a `guided-review` looked up on `PATH`, since by the time the restart runs (it may wait on the restart guard) either may lead to another binary. The restart guard still holds: with an Ask or hand-off in flight, the first restart only warns, and the button then reads Restart for the version, to restart anyway.

Since 2026-11-16 the restart passes the app's own build (`restart --replacing <build>`), the only way a `restart` may quit an app of another build ([agent runs](agent-runs.md)). An app from before this passes none, so its update to the first release with it cannot restart; the reviewer starts the app once by hand.
