# Changelog

All notable changes to SuperShot are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Version numbering adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [1.0.0] -- 2026-02-11

First public release.

### Added

- **Capture modes**: Selection (area), full screen, and window capture via the
  XDG Desktop Portal screenshot interface (`ashpd` 0.12).
- **Configurable delay**: None, 3, 5, and 10-second delay options with a visual
  countdown overlay displayed in the main window.
- **Clipboard integration**: Optional one-click copy of the captured image to
  the system clipboard via the GDK Texture/Clipboard API.
- **Shutter sound**: Audible camera shutter feedback using `canberra-gtk-play`
  (GNOME sound event system) with a `paplay` fallback for systems without
  libcanberra. Executes asynchronously in a dedicated thread.
- **Desktop notifications**: GNOME notification posted after each successful
  capture, displaying the saved file path.
- **Settings persistence**: All user preferences (capture mode, delay, clipboard
  toggle, sound toggle) stored via GSettings with bidirectional widget bindings.
  Schema auto-installed to `~/.local/share/glib-2.0/schemas/` during development
  builds by `build.rs`.
- **CLI headless mode**: `--now` flag for scriptable capture without GUI.
  Supports `--mode`, `--delay`, and `--clipboard` arguments. CLI mode validates
  capture modes via `ValueEnum` at parse time.
- **Internationalization infrastructure**: GNU gettext initialization via
  `gettext-rs`. English UI strings as source language. Translation-ready `po/`
  directory structure.
- **Distribution files**: `.desktop` launcher, AppStream `metainfo.xml`,
  GSettings XML schema, SVG application icon, and `.deb` packaging support.
- **Error handling**: User-visible error dialogs via `AdwAlertDialog` for
  portal failures, file save errors, and missing directories. Diagnostic output
  retained on `stderr` for troubleshooting.

### Technical details

- **UI framework**: GTK4 0.10 + Libadwaita 0.8 (v1\_5 feature gate).
  AdwApplicationWindow subclass with inline composite template.
- **Screenshot backend**: `ashpd` 0.12 (XDG Desktop Portal).
  `interactive(true)` for selection/window modes, `interactive(false)` for
  full screen.
- **File output**: Saved to `~/Pictures/Screenshots/Screenshot_YYYY-MM-DD_HH-MM-SS.fff.png`
  with millisecond precision to prevent same-second overwrites.
  Async GIO file copy to avoid blocking the main loop.
- **Window hiding**: 200 ms settling delay after `set_visible(false)` to ensure
  the compositor fully unmaps the window before portal capture.
- **Build**: Rust edition 2021. Zero compiler warnings, zero Clippy diagnostics.
  Release binary ~3.9 MB (stripped).

### Dependencies

| Crate | Version | Purpose |
|---|---|---|
| gtk4 | 0.10 | UI toolkit |
| libadwaita | 0.8 | Adwaita design system |
| ashpd | 0.12 | XDG Desktop Portal client |
| chrono | 0.4 | Timestamp generation |
| dirs | 6.0 | XDG directory resolution |
| gettext-rs | 0.7 | GNU gettext bindings |
| clap | 4 | CLI argument parsing |

---

[Unreleased]: https://github.com/axpnet/supershot/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/axpnet/supershot/releases/tag/v1.0.0
