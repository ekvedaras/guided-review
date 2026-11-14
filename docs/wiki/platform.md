# Platform

Decisions on how the app learns which operating system it runs on, from the Linux work (spec #67). Plans and tickets live in GitHub issues ([Where work is tracked](delivery.md)); this page keeps the decisions and why.

## Where it came from

Until 2026-11-14 the app was built for macOS only, and what it knew of macOS was spread through it: the release target named only Macs, gpui had no Linux window backend, and the Dock icon and the Option-arrow monitor each carried their own `cfg(target_os)`. A reviewer on Linux (Arch, Hyprland on Wayland) could not run it. The maintainer chose one seam for everything that differs by OS, so a later Windows port adds one adapter instead of hunting checks through the app.

## Decisions, 2026-11-14 (#68)

- **One platform module.** `src/platform.rs` holds every OS fact; `src/platform/macos.rs` and `src/platform/linux.rs` are the adapters. `Os::RUNNING` was made the one place that picks the running OS. The rest of the app asks the module and does not check the OS (`a167d8a`).
- **Adapters that are plain data compile on every host**, so a test on a Mac checks the Linux adapter and the other way round. Only code that needs one OS's libraries (objc2: the Dock icon and the Option-arrow monitor, moved to `src/platform/macos/`) sits behind `cfg`. Moving them fixed clippy on Linux too: their pure helpers had been dead code there.
- **The gate enforces it.** `scripts/check` fails on any `target_os` in `src/`, `tests/` or `build.rs` outside `src/platform` (`f6b14e4`). A grep, not a lint: no clippy lint knows a path rule, and a grep is one line. `Cargo.toml`'s macOS-only dependency table is not checked; it is where Cargo needs it. `target_arch` is not checked: a CPU is not an OS.
- **A release Target is an OS and a CPU** (`update::Target { os, cpu }`), so its triple and archive name follow from both: `x86_64-unknown-linux-gnu` on Linux. Before, `Target` was `AppleSilicon` or `Intel`, and a Linux build would have fetched a Mac archive.
- **gpui gets its `wayland` and `x11` features** (`7a0f87a`). They change nothing on macOS. With them the app opened a native Wayland window on Hyprland (not through XWayland) on 2026-11-14; its window had no app id yet, which the Desktop entry work sets.

## Decisions, 2026-11-14 (#69)

- **The platform names the font families** (`083c89c`). Each adapter lists its text and code families, best first; `Os::text_font` and `Os::code_font` take the first that is installed and the last one unasked. macOS lists one of each (Helvetica Neue, Menlo), so it never reads the font database for the app's text. Linux lists Adwaita, Noto, Liberation and DejaVu (Sans and Mono), names checked against the installed fonts, not asked of fontconfig: a list is plain data, so both adapters are tested on any host, and the families come out the same on every desktop that has them.
- **Diagrams find the files of the text family** in the font database (`palette::font_files`), all of them, so bold labels are measured in bold too. The path `/System/Library/Fonts/HelveticaNeue.ttc` is gone; it was why every diagram failed on Linux. The system font database is read once (`INSTALLED_FONTS`) and shared by the Linux font check and the diagram drawer.

## Known open

- The Dock icon, background and word-key calls are three platform functions the app start calls in turn; one start call is a later ticket.
- The Primary modifier, the Desktop entry, window chrome, raising Herdr, app folders, the Linux release and CI, and the macOS-only dev scripts are each their own ticket under #67.
- `scripts/check` on Linux needs rustup: the pinned toolchain (`rust-toolchain.toml`) uses library features newer than Arch's `rust` package had on 2026-11-14.
