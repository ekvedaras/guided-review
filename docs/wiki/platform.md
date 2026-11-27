# Platform

How the app learns which operating system it runs on. From the Linux work (spec #67, 2026-11-14). Plans and tickets live in GitHub issues ([Where work is tracked](delivery.md)); this page keeps the decisions and why.

Until 2026-11-14 the app was built for macOS only, and what it knew of macOS was spread through it: the release target named only Macs, gpui had no Linux window backend, and the Dock icon and the Option-arrow monitor each carried their own `cfg(target_os)`. The maintainer chose one seam for everything that differs by OS, so a later Windows port adds one adapter instead of hunting checks through the app.

## One platform module

- `src/platform.rs` holds every OS fact; `src/platform/macos.rs` and `src/platform/linux.rs` are the adapters. Each exports one `ADAPTER`, a plain `Adapter` value with every fact of its OS (triple, fonts, Primary key, key spelling, text keys, window buttons, app menu, Herdr raise, Desktop entry constructor). `Os::RUNNING` is the one place that picks the OS, `Os::adapter` the one match on it, and the `Os` methods read it. A Windows port adds one `ADAPTER` and one match arm. The rest of the app asks the module and never checks the OS.
- **Adapters that are plain data compile on every host**, so a test on a Mac checks the Linux adapter and the other way round. Only code that needs one OS's libraries (objc2: the Dock icon and the Option-arrow monitor, in `src/platform/macos/`) sits behind `cfg`.
- **The gate enforces it.** `scripts/check` fails on any `target_os` in `src/`, `tests/` or `build.rs` outside `src/platform`. A grep, not a lint: no clippy lint knows a path rule, and a grep is one line. `Cargo.toml`'s macOS-only dependency table is not checked; it is where Cargo needs it. `target_arch` is not checked: a CPU is not an OS.
- **No bool parameters in the platform** (CODING_STANDARDS): `platform::start` takes a `Focus` (`Taken` or `LeftAlone`), and `HerdrRaise` says what a failed raise does with `RaiseFailed`.
- **A release Target is an OS and a CPU** (`update::Target { os, cpu }`), so its triple and archive name follow from both: `x86_64-unknown-linux-gnu` on Linux. Before, `Target` was `AppleSilicon` or `Intel`, and a Linux build would have fetched a Mac archive.
- **gpui gets its `wayland` and `x11` features** (`Cargo.toml`). They change nothing on macOS. With them the app opens a native Wayland window on Hyprland, not through XWayland.

## Fonts

- Each adapter names its text and code families. `Os::font` (by `FontRole`) takes them from a `FontChoice`: macOS's Helvetica Neue and Menlo come with the OS and are never looked up. Linux lists Adwaita, Noto, Liberation and DejaVu (Sans and Mono), best first, and takes the first that is installed. When none is, it takes the font database's default sans-serif or monospace family (fontconfig's, read by fontdb), so diagrams find a file whenever that default is installed.
- The lists are plain data checked against the installed fonts, not asked of fontconfig, so both adapters are tested on any host and the families come out the same on every desktop that has them.
- Diagrams find all files of the text family in the font database (`palette::font_files`), so bold labels are measured in bold too. The font database is read once (`INSTALLED_FONTS`) and shared by the Linux font check and the diagram drawer. A hard-coded `/System/Library/Fonts/HelveticaNeue.ttc` made every diagram fail on Linux.

## Shortcuts

- **The app's shortcuts are held with the Primary modifier** (`keys::Modifier::Primary`): Cmd on macOS, Ctrl on Linux. It binds as gpui's `secondary`, which gpui maps the same way. The app had bound gpui `cmd`, which on Linux is Super, so Super+K opened the palette and Hyprland took Super for itself.
- **Super is not used on Linux.** A press with Super is no chord (`app::keymap::chord_on`), so Settings cannot record one. `Os::primary_key` says which key Primary is.
- **`config.toml` writes `primary-`.** `cmd-` saved before still reads as Primary on every OS. On Linux `ctrl-` reads as Primary too, so a Linux reviewer's natural `ctrl-k` and the default can never bind one key twice. `Chord::written` is the config text, `Chord::keystroke` the gpui text. An older version reads a config written with `primary-` as "not a key".
- **One spelling table per OS** (`Os::key_spelling`): the faces of the modifiers and the joiner between caps, used for spelled chords and key caps: `⌘⇧↵` on macOS, `Ctrl+Shift+↵` on Linux. The quit and close warnings spell the default key instead of saying `cmd+q`.
- **Text box keys come from a table per OS** (`Os::text_keys`, `platform::TextEdit`): the macOS keys as before; on Linux Ctrl moves and deletes by word, Ctrl+Home/End go to the ends of the text, Shift+Home/End select to the line's ends, Ctrl+A/C/X/V. Linux has no delete-to-line-start key, as GTK has none. Keys every OS shares stay in `text_box::bind_keys`.

## Window chrome and app start

- **The platform gives the window's title bar options and the button inset** (`Os::titlebar`, `Os::window_buttons_inset`), both from the adapter's `window_buttons`. macOS: the traffic lights on a transparent title bar, with room kept left of them. Linux: none, so the title bar is not transparent and no room is kept; the window manager decides the decorations (Hyprland draws none). The app draws its own title bar on Linux too, as the strip that drags the window and carries the sidebar heading and the Settings chip.
- **One platform call at app start** (`platform::start(focus, icon, quit, cx)`) gives the OS what it gets from the app: on macOS the Dock icon (or, when the app must not take the focus, no Dock icon and no turn at the front), the Option-arrow monitor and the app menu. Linux gets none of them. The app start still decides whether the app takes the focus and owns the Quit action.
- **The app menu is plain data per OS** (`Os::app_menus(quit)`): macOS one `guided-review` menu with Quit; Linux none, as Linux desktops have no app menu bar. Quit on Linux is the Quit key only, bound from the keymap as on every OS.

## Raising Herdr

The platform says how Herdr comes to the front after `herdr-app do agent.focus` (`Os::herdr_raise`, `HerdrRaise`): `/usr/bin/open -a Herdr` on macOS; `hyprctl dispatch 'hl.dsp.focus({ window = "class:herdr-app" })'` on Linux, the class Herdr's window has on Hyprland. Hyprland 0.56 made its dispatch Lua and refuses the old `focuswindow class:herdr-app` form with exit 7 (fixed 2026-11-15; the error goes to stdout, so `app.log` showed it empty). Only the Lua form is sent: a Hyprland before 0.56 leaves Herdr behind.

On Linux the raise is best effort (`RaiseFailed::LeavesHerdrBehind`). Only the desktop raises windows on Wayland and only Hyprland is asked, so a missing `hyprctl`, another desktop or a failed dispatch leaves Herdr behind, is logged to stderr (`app.log` for the app), and showing the agent still succeeds. On macOS a failed `open` still fails it (`FailsShowing`).

## Desktop entry

- **The Desktop entry is one trait** (`platform::DesktopEntry`, `src/platform/desktop_entry.rs`), implemented by `AppBundle` (`src/platform/macos/app_bundle.rs`) and `DesktopFile` (`src/platform/linux/desktop_file.rs`) and picked once, by the adapter's `desktop_entry` constructor. It puts the entry in place from a release folder, names the variable that marks a launch from it, and gives the relaunch through the login shell (the trait's one shared method). Both are plain files, so both are tested on any host. The bin link is the release install's (`ReleaseInstall`) and is passed to the entry, which does not name `~/.local/bin/guided-review` itself.
- **On Linux the release carries only the icons** (`icons/hicolor/{256x256,512x512}/apps/io.github.ekvedaras.guided-review.png`, at the path they take under `~/.local/share`); the desktop file is written at install. Its Exec names the bin link by full path, as desktop files expand no `~`, so no release could carry it ready.
- **The Exec line marks the launch itself**: `env GUIDED_REVIEW_LAUNCHED_APP=<app id> <bin link>`, the Linux stand-in for macOS's `__CFBundleIdentifier`, as Linux launchers name no app they start (GIO's `GIO_LAUNCHED_DESKTOP_FILE` is set by GTK launchers only). The relaunch is the macOS one with bash in place of zsh when `$SHELL` is unset. On the maintainer's machine a bash login shell with a launcher's short `PATH` found `gh` only through mise's shims, which is why the relaunch matters on Linux too.
- **One app id, `io.github.ekvedaras.guided-review`**: the bundle id, the desktop file's name, its `StartupWMClass` and the window's app id, which Wayland compositors match to the desktop file for the window's icon.
- **`guided-review desktop-entry-template`** writes every OS's entry files; the macOS release job packs only the bundle from it. It replaced `bundle-template`.

## App folders: XDG on every OS

2026-11-14 (#75). `platform::app_folders(home, env)` gives the config, state and cache folders from the XDG variables (an empty or relative one is ignored, as the XDG spec says) and the XDG defaults under home, also on macOS, so a Windows adapter is one more layout there. The rest of the app gets an `AppFolders` value (`src/app_folders.rs`), never a bare home: `home()` stays on it for `~/` in settings, the skills folder, the release install and the Desktop entry, whose folders do not change. Inside the state folder the old names stay (`tours/`, `state/`, `evals/`), so the move is a rename per entry.

- **Tests never read the reviewer's environment for folders.** In-process tests build `support::folders_in(home)` (XDG defaults, no variables); the CLI runner and `scripts/snapshot` clear the XDG variables beside their throwaway HOME.
- **The socket lives in the state folder beside the lock; there is no runtime folder.** `XDG_RUNTIME_DIR` differs between processes (an agent started through herdr, ssh), so a CLI looked for the socket elsewhere than the running app. Paths stay under macOS's 103-byte socket limit for temp homes.
- **Agent permission rules name the real tours and evals folders** as `//<abs>/**`, not `~/.guided-review/...`; the /tour skill finds tours through `guided-review tour-path`.

**The move from `~/.guided-review` loses nothing.** Losing or splitting a reviewer's tours, read ticks or Asks is the failure to avoid.

- **The move runs once per process, from `main`** (`AppFolders::settle`), never from `AppFolders::running()`: a library call made in a test must not move the developer's own `~/.guided-review`. `open` tries it again after quitting an app of an earlier build (`settle_again`, called from `app_link`), so the CLI and the app it starts agree on the socket.
- **Only the release install moves it, or any binary on a machine with none** (`Mover`). The maintainer runs source builds beside the release install; a move by one of them left the release on a folder no longer used. Another binary never moves: it reads the XDG folders once they hold what the app writes to its state folder (tours, state, evals, the log; a config alone may be written by hand), and the old folder before. Source-only users have no release install, so their first start moves.
- **The move holds the old folder's `app.lock`** (waits up to 1 s). While an app of an earlier version runs, nothing moves and that start keeps using `~/.guided-review` whole. Starts at once all end in the XDG folders: one that finds the old folder gone when it opens the lock takes it as moved; one whose opening of the lock made the folder again tidies it away, so no stray `app.lock` raises a problem.
- **Each file moves on its own; what the XDG folders already hold is never written over.** A file the XDG folders lack is moved, even when they already hold others. A file in both stays in both, unless the two are the same, when the old one is deleted. Across disks a file is copied to `<name>.partial`, compared, renamed into place, and only then the original deleted. Whatever is left is moved by the next start, so there is no mark; a `.moving` mark left by an earlier build is tidied away. The cache is not moved: it fills again.
- **The move never fails a command.** A step that fails puts back exactly what this start moved and the start uses the old folder. What cannot be put back stays where it is, with a warning, and the next start tries again.
- **One problem, `FilesNotRead`, names each file left in the folders not in use beside the file read instead.** It covers a file in both after a move, what an older binary wrote since, and what a failed move could not put back. Nothing is merged: the reviewer keeps what they need by hand. Files the reviewer put in `~/.guided-review` themselves are no problem.
- **The move points config values at the moved files** (`summary_instructions = "~/.guided-review/summary.md"` becomes `"~/.config/guided-review/summary.md"`), written as before (under `~/` when it was, same quotes), with comments and the rest kept. Values naming the reviewer's own files in `~/.guided-review` stay.

## Dev scripts, CI and release

- **The snapshot scripts stop early off macOS** with a message saying why, and exit 1. `scripts/snapshot` needs gpui's `render_to_image`, which this gpui rev has only on macOS. `scripts/snapshot-compare` is a shell wrapper around `snapshot-compare.swift`, since a Swift file cannot check the OS before `import AppKit` fails. They check `uname`, not the platform module: they are shell, outside the app.
- **`scripts/check` takes rustup's cargo first** (`~/.cargo/bin`), before whatever cargo is on PATH: Arch's `rust` package puts an older cargo in `/usr/bin` that cannot build the pinned toolchain's code. With no rustup proxies, any `stable-*` toolchain under `~/.rustup` is the last resort.
- **CI runs the gate on `ubuntu-latest` beside `macos-latest`**, a matrix of the one `test` job. No branch rule names the check, so its name `test (<os>)` blocks nothing.
- **`scripts/linux-build-packages` is the one list of apt packages** CI, the release and the README's build from source use. Taken from what the binary links (`ldd`: libxcb, libxkbcommon, libxkbcommon-x11) and what gpui's crates build or load (fontconfig, wayland-client, the Vulkan loader), checked against `script/linux` of the pinned zed rev. Not taken from that script: ALSA, libgit2, OpenSSL, sqlite, clang and the rest Zed itself needs. DejaVu is installed so the fonts' last choice is there.
- **The Linux archive is built on `ubuntu-24.04`**, so it needs glibc 2.39 or newer. Its job runs `scripts/check` itself, as `verify` gates only the macOS run, and packs the binary, skills, LICENSE, README and the `icons/` of the macOS-built Desktop entry template. The checksum is `sha256sum`'s line, which reads like `shasum -a 256`'s. aarch64 Linux is not built.
- **The updater's Linux path is tested with GitHub's answer for v0.9.0** with a Linux archive added (`tests/fixtures/github/latest_release_with_linux.json`): find the archive, download, install, desktop entry placed.

## Known open

- `scripts/check` on Linux needs rustup: the pinned toolchain (`rust-toolchain.toml`) uses library features newer than Arch's `rust` package had on 2026-11-14.
- Snapshots on Linux wait for a gpui with `render_to_image` there.
