# Platform

Decisions on how the app learns which operating system it runs on, from the Linux work (spec #67). Plans and tickets live in GitHub issues ([Where work is tracked](delivery.md)); this page keeps the decisions and why.

## Where it came from

Until 2026-11-14 the app was built for macOS only, and what it knew of macOS was spread through it: the release target named only Macs, gpui had no Linux window backend, and the Dock icon and the Option-arrow monitor each carried their own `cfg(target_os)`. A reviewer on Linux (Arch, Hyprland on Wayland) could not run it. The maintainer chose one seam for everything that differs by OS, so a later Windows port adds one adapter instead of hunting checks through the app.

## Decisions, 2026-11-14 (#68)

- **One platform module.** `src/platform.rs` holds every OS fact; `src/platform/macos.rs` and `src/platform/linux.rs` are the adapters. `Os::RUNNING` was made the one place that picks the running OS. The rest of the app asks the module and does not check the OS (`9e1eb8d`).
- **Adapters that are plain data compile on every host**, so a test on a Mac checks the Linux adapter and the other way round. Only code that needs one OS's libraries (objc2: the Dock icon and the Option-arrow monitor, moved to `src/platform/macos/`) sits behind `cfg`. Moving them fixed clippy on Linux too: their pure helpers had been dead code there.
- **The gate enforces it.** `scripts/check` fails on any `target_os` in `src/`, `tests/` or `build.rs` outside `src/platform` (`b1d565c`). A grep, not a lint: no clippy lint knows a path rule, and a grep is one line. `Cargo.toml`'s macOS-only dependency table is not checked; it is where Cargo needs it. `target_arch` is not checked: a CPU is not an OS.
- **A release Target is an OS and a CPU** (`update::Target { os, cpu }`), so its triple and archive name follow from both: `x86_64-unknown-linux-gnu` on Linux. Before, `Target` was `AppleSilicon` or `Intel`, and a Linux build would have fetched a Mac archive.
- **gpui gets its `wayland` and `x11` features** (`490dd7f`). They change nothing on macOS. With them the app opened a native Wayland window on Hyprland (not through XWayland) on 2026-11-14; its window had no app id yet, which the Desktop entry work sets.

## Decisions, 2026-11-14 (#69)

- **The platform names the font families** (`d53ab58`). Each adapter lists its text and code families, best first; `Os::text_font` and `Os::code_font` take the first that is installed and the last one unasked. macOS lists one of each (Helvetica Neue, Menlo), so it never reads the font database for the app's text. Linux lists Adwaita, Noto, Liberation and DejaVu (Sans and Mono), names checked against the installed fonts, not asked of fontconfig: a list is plain data, so both adapters are tested on any host, and the families come out the same on every desktop that has them.
- **Diagrams find the files of the text family** in the font database (`palette::font_files`), all of them, so bold labels are measured in bold too. The path `/System/Library/Fonts/HelveticaNeue.ttc` is gone; it was why every diagram failed on Linux. The system font database is read once (`INSTALLED_FONTS`) and shared by the Linux font check and the diagram drawer.

## Decisions, 2026-11-14 (#70, Primary modifier)

- **The app's shortcuts are held with the Primary modifier** (`viewer::Modifier::Primary`, was `Cmd`): Cmd on macOS, Ctrl on Linux. It binds as gpui's `secondary`, which gpui maps the same way. Before, the app bound gpui `cmd`, which on Linux is Super, so Super+K opened the palette and Hyprland took Super for itself (`b5e9f34`).
- **Super is not used on Linux.** A press with Super is no chord (`app::keymap::chord_on`), so Settings cannot record one. `Os::primary_key` says which key Primary is.
- **`config.toml` writes `primary-`.** `cmd-` saved before still reads as Primary on every OS, and on Linux `ctrl-` reads as Primary too, so a Linux reviewer's natural `ctrl-k` and the default can never bind one key twice. `Chord::written` is the config text, `Chord::keystroke` the gpui text; they differed from here on. A config written by this version reads as "not a key" in older versions.
- **One spelling table per OS** (`Os::key_spelling`): the faces of the modifiers and the joiner between caps. Spelled chords and key caps both read it: `⌘⇧↵` on macOS, `Ctrl+Shift+↵` on Linux. The quit and close warnings spell the default key instead of saying `cmd+q`.
- **Text box keys come from a table per OS** (`Os::text_keys`, `platform::TextEdit`): the macOS text keys as before; on Linux Ctrl moves and deletes by word, Ctrl+Home/End go to the ends of the text, Shift+Home/End select to the line's ends, Ctrl+A/C/X/V. Linux has no delete-to-line-start key, as GTK has none. Keys every OS shares stay in `text_box::bind_keys`.

## Decisions, 2026-11-14 (#72, window chrome)

- **The platform gives the window's title bar options and the button inset** (`Os::titlebar`, `Os::window_buttons_inset`), both from one adapter fact, `WINDOW_BUTTONS`. macOS: the traffic lights 14 px from the corner on a transparent title bar, 80 px kept free left of them, as before. Linux: none, so the title bar is not transparent and no room is kept; the window manager decides the decorations (Hyprland draws none). Before, the title bar code held `TRAFFIC_LIGHTS_WIDTH` and the sidebar and Settings left an empty 80 px gap on Linux.
- **The title bar stays 40 px on every OS.** The app draws it on Linux too, as the strip that drags the window and carries the sidebar heading and the Settings chip.

## Decisions, 2026-11-14 (#74, raising Herdr)

- **The platform says how Herdr comes to the front** after `herdr-app do agent.focus` (`Os::herdr_raise`, `platform::HerdrRaise`): `/usr/bin/open -a Herdr` on macOS; `hyprctl dispatch 'hl.dsp.focus({ window = "class:herdr-app" })'` on Linux, the class Herdr's window has on Hyprland. 2026-11-15: it was `hyprctl dispatch focuswindow class:herdr-app`, which Hyprland 0.56 refuses with exit 7, as its dispatch is now Lua; the error goes to stdout, so `app.log` showed it empty. Only the Lua form is sent: a Hyprland before 0.56 leaves Herdr behind.
- **On Linux the raise is best effort.** Only the desktop raises windows on Wayland and only Hyprland is asked, so a missing `hyprctl`, another desktop or a failed dispatch leaves Herdr behind and showing the agent still succeeds. On macOS a failed `open` still fails it, as before.
- Tests run both adapters' commands through one stand-in (`HcTourAgents::raising_herdr_with(os, program)`).

## Decisions, 2026-11-14 (#71)

- **The Desktop entry is one interface** (`4a541e7`): `Os::desktop_entry(home)` puts the entry in place from a release folder, names the variable that marks a launch from it, gives the launcher to start the app with and the relaunch through the login shell. The App bundle moved to `src/platform/macos/app_bundle.rs` unchanged in what it writes; the Linux form is `src/platform/linux/desktop_file.rs`. Both are plain files, so both are tested on any host.
- **On Linux the release carries only the icons** (`icons/hicolor/{256x256,512x512}/apps/io.github.ekvedaras.guided-review.png`, at the path they take under `~/.local/share`); the desktop file is written at install. Its Exec names the bin link by full path, as desktop files expand no `~`, so no release could carry it ready.
- **The Exec line marks the launch itself**: `env GUIDED_REVIEW_LAUNCHED_APP=<app id> <bin link>`, the Linux stand-in for macOS's `__CFBundleIdentifier`, as Linux launchers name no app they start (GIO's `GIO_LAUNCHED_DESKTOP_FILE` is set by GTK launchers only). The relaunch is the macOS one with bash in place of zsh when `$SHELL` is unset. On the maintainer's machine a bash login shell with a launcher's short `PATH` found `gh` only through mise's shims, which is why the relaunch matters on Linux too.
- **One app id, `io.github.ekvedaras.guided-review`**: the bundle id, the desktop file's name, its `StartupWMClass` and the window's app id (`6064df7`), which Wayland compositors match to the desktop file for the window's icon.
- **`bundle-template` became `desktop-entry-template`** and writes every OS's entry files; the macOS release job packs only the bundle from it.

## Decisions, 2026-11-14 (#73, app start hooks)

- **One platform call at app start** (`platform::start(takes_focus, quit, cx)`) gives the OS what it gets from the app: on macOS the Dock icon (or, when the app must not take the focus, no Dock icon and no turn at the front), the Option-arrow monitor and the app menu. Linux gets none of them. The app start no longer names the Dock or macOS; it still decides whether the app takes the focus and owns the Quit action and its handler.
- **The app menu is plain data per OS** (`Os::app_menus(quit)`): macOS one `guided-review` menu with Quit, which macOS shows beside its key; Linux none, as Linux desktops have no app menu bar. Quit on Linux is the Quit key only, bound from the keymap as on every OS.

## Decisions, 2026-11-14 (#76, dev scripts)

- **The snapshot scripts stop early off macOS** with a message saying why, and exit 1. `scripts/snapshot` needs gpui's `render_to_image`, which this gpui rev has only on macOS. `scripts/snapshot-compare` is now a shell wrapper around `snapshot-compare.swift`, since a Swift file cannot check the OS before `import AppKit` fails. They check `uname`, not the platform module: they are shell, outside the app.
- **`scripts/check` takes rustup's cargo first** (`~/.cargo/bin`), before whatever cargo is on PATH: Arch's `rust` package puts an older cargo in `/usr/bin` that cannot build the pinned toolchain's code. The `stable-aarch64-apple-darwin` path is gone; with no rustup proxies, any `stable-*` toolchain under `~/.rustup` is the last resort.

## Decisions, 2026-11-14 (#75): app folders

- **XDG on every OS** (`a3cce78`). `platform::app_folders(home, env)` gives the config, state, cache and runtime folders from the XDG variables (an empty or relative one is ignored, as the XDG spec says) and the XDG defaults under home, also on macOS, so a Windows adapter is one more layout there. The rest of the app gets an `AppFolders` value, never a bare home: `home()` stays on it for `~/` in settings, the skills folder, the release install and the Desktop entry, whose folders do not change. Inside the state folder the old names stay (`tours/`, `state/`, `evals/`), so the move is a rename per entry.
- **Tests never read the reviewer's environment for folders.** In-process tests build `support::folders_in(home)` (XDG defaults, no variables); the CLI runner and `scripts/snapshot` clear the four XDG variables beside their throwaway HOME.
- **The move runs once per process, from `main`** (`AppFolders::settle`), never from `AppFolders::running()`: a library call made in a test must not move the developer's own `~/.guided-review`.
- **Never half and half.** The move holds the old folder's `app.lock` (waits up to 1 s): while an app of an earlier version runs, nothing moves and that start keeps using `~/.guided-review` whole. Each of config.toml, summary.md, tours, state, evals and app.log is renamed; across disks it is copied to `<name>.partial`, compared, renamed into place, and only then the original deleted. A `.moving` mark in the old folder lets a start after a crash finish the move instead of taking half-filled XDG folders as done. A step that fails puts every entry back and the start uses the old folder. The cache is not moved: it fills again.
- **The XDG folders win once they hold files.** An older binary that writes `~/.guided-review` afterwards (say a source build in the way) is not merged: each review opened shows the problem `OldFolderWrittenAgain`, which names the folder. Files the reviewer put in `~/.guided-review` themselves stay there and are no problem.
- **`open` tries the move again after quitting an app of an earlier build** (`app_link::start_and_hand`), so the CLI and the app it starts agree on the socket.
- **Agent permission rules name the real tours and evals folders** as `//<abs>/**`, not `~/.guided-review/...`; the /tour skill already finds tours through `guided-review tour-path`.

## Decisions, 2026-11-14 (#75 review): a move that loses nothing

A review of #75 found ways the move could hide or split a reviewer's tours, read ticks or Asks. Losing or splitting them is the failure to avoid; these supersede the bullets above where they differ.

- **Only the release install moves `~/.guided-review`, or any binary on a machine with none** (`Mover`, `ReleaseInstall::folder_mover`). The maintainer runs source builds (`cargo run`, `scripts/snapshot`'s `tour-path` with the real HOME) beside the release install; a move by one of them left the release on a folder no longer used. Another binary never moves: it reads the XDG folders once they hold what the app writes to its state folder (tours, state, evals, the log; a config alone may be written by hand), and the old folder before. Source-only users have no release install, so their first start moves.
- **Each file moves on its own; what the XDG folders already hold is never written over** (supersedes "The XDG folders win once they hold files"). A file the XDG folders lack is moved, even when they already hold others (a hand-written config, a tour a newer build wrote), so a reviewer's old tours are never ignored whole. A file in both stays in both, unless the two are the same, when the old one is deleted (a copy across disks put in place before a crash). The `.moving` mark is gone: whatever is left is moved by the next start, so there is nothing to mark; one left by an earlier build is tidied away.
- **The move never fails a command** (supersedes "A step that fails puts every entry back"). A step that fails puts back exactly what this start moved and the start uses the old folder. What cannot be put back stays where it is, with a warning, and the next start tries again. Before, a failed put-back left the mark and made every later command fail.
- **One problem, `FilesNotRead`, names each file left in the folders not in use beside the file read instead** (supersedes `OldFolderWrittenAgain`, whose "written again by an older guided-review" was wrong when the XDG folders were filled some other way). It covers a file in both after a move, what an older binary wrote since, and what a failed move could not put back. Nothing is merged: the reviewer keeps what they need by hand.
- **Starts at once all end in the XDG folders.** One that finds the old folder gone when it opens the lock takes it as moved; one whose opening of the lock made the folder again tidies it away, so no stray `app.lock` raises a problem.
- **The move points config values at the moved files** (`summary_instructions = "~/.guided-review/summary.md"` becomes `"~/.config/guided-review/summary.md"`), written as before (under `~/` when it was, same quotes), with comments and the rest kept. Values naming the reviewer's own files in `~/.guided-review` stay.
- **The socket lives in the state folder beside the lock; there is no runtime folder** (supersedes the runtime folder in "XDG on every OS"). `XDG_RUNTIME_DIR` differs between processes (an agent started through herdr, ssh), so a CLI looked for the socket elsewhere than the running app. Paths stay under macOS's 103-byte socket limit for temp homes.

## Decisions, 2026-11-14 (#77): Linux release and CI

- **CI runs the gate on `ubuntu-latest` beside `macos-latest`**, a matrix of the one `test` job. No branch rule names the check, so its rename to `test (<os>)` blocks nothing.
- **`scripts/linux-build-packages` is the one list of apt packages** CI, the release and the README's build from source use. Taken from what the binary links (`ldd`: libxcb, libxkbcommon, libxkbcommon-x11) and what gpui's crates build or load (fontconfig, wayland-client, the Vulkan loader), checked against `script/linux` of the pinned zed rev. Not taken from that script: ALSA, libgit2, OpenSSL, sqlite, clang and the rest Zed itself needs. DejaVu is installed so the fonts' last choice is there.
- **The Linux archive is built on `ubuntu-24.04`**, so it needs glibc 2.39 or newer. Its job runs `scripts/check` itself, as `verify` gates only the macOS run, and packs the binary, skills, LICENSE, README and the `icons/` of the macOS-built Desktop entry template; the checksum is `sha256sum`'s line, which reads like `shasum -a 256`'s. aarch64 Linux is not built.
- **The updater's Linux path is tested with GitHub's answer for v0.9.0** with a Linux archive added (`tests/fixtures/github/latest_release_with_linux.json`): find the archive, download, install, desktop entry placed.

## Decisions, 2026-11-14 (review of the Linux work)

- **One adapter value per OS.** Each adapter module exports one `ADAPTER`, a plain `Adapter` holding every fact of its OS (triple, fonts, Primary key, key spelling, text keys, window buttons, app menu, Herdr raise), and `Os::adapter` is the one match on the OS; the `Os` methods read it. Before, each fact was its own constant, nine `Os` methods each matched on the OS, and Linux's missing app menu was an inline `Vec::new()` in `Os::app_menus`. A Windows port adds one `ADAPTER` and one match arm.
- **The Desktop entry is a trait** (`platform::DesktopEntry`), implemented by `AppBundle` and `DesktopFile` and picked once, by the adapter's `desktop_entry` constructor. Before, `DesktopEntry` wrapped a private enum of the two that six methods matched on. The relaunch through the login shell is the trait's one shared method.
- **The bin link is the release install's** (`ReleaseInstall`) and passed to the entry, which no longer names `~/.local/bin/guided-review` itself; the launch and `app_link` get the entry from `ReleaseInstall`. The bundle's folder and the `~/Applications` that messages name are one constant.
- **No bool parameters in the platform** (CODING_STANDARDS): `platform::start` takes a `Focus` (`Taken` or `LeftAlone`), and `HerdrRaise` says what a failed raise does with `RaiseFailed` (`FailsShowing` on macOS, `LeavesHerdrBehind` on Linux), where they had `takes_focus: bool` and `best_effort: bool`.
- **A family comes with the OS or is the first installed** (`FontChoice`). macOS's Helvetica Neue and Menlo come with it and are never looked up, as before. When none of Linux's families is installed, the app takes the font database's default sans-serif or monospace family (fontconfig's, read by fontdb) instead of DejaVu unasked, so diagrams find a file whenever that default is installed.
- **A Herdr raise that fails on Linux is logged** to stderr, which is `app.log` for the app, and showing the agent still succeeds. Before, it left no trace.

## Known open

- `scripts/check` on Linux needs rustup: the pinned toolchain (`rust-toolchain.toml`) uses library features newer than Arch's `rust` package had on 2026-11-14.
- Snapshots on Linux wait for a gpui with `render_to_image` there.
