# Changelog

All notable changes to SuperShot are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Version numbering adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [1.2.1] -- 2026-05-01

### Changed

- **Updated dependencies**: Bumped Rust crates to their latest compatible versions.
- **Security maintenance**: Added `liblcms2-2` to snap stage packages to resolve security vulnerabilities in the build environment.
- **Fixed deprecations**: Migrated from `CssProvider::load_from_data` to `load_from_string` for GTK 4.12+ compatibility.

### Dependencies

| Crate | Version | Purpose |
|---|---|---|
| gtk4 | 0.11 | UI toolkit |
| libadwaita | 0.9 | Adwaita design system |
| ashpd | 0.13 | XDG Desktop Portal client |
| cairo-rs | 0.22 | Watermark rendering |
| gdk-pixbuf | 0.22 | Image processing |

---

## [1.2.0] -- 2026-04-16

### Added

- **Image editor**: Full editing sidebar with rotate, flip, brightness,
  contrast, blur, sharpen, grayscale, and invert controls. All transforms
  applied live via the `image` crate with non-destructive `EditState`.
- **Apply Crop**: Button in sidebar and click-to-crop inside the selected area
  to confirm the crop and preview the result before saving.
- **Undo Crop**: Restores the pre-crop state (original image and all edit
  values) so the user can redo the selection.
- **Portal-first capture**: The XDG Desktop Portal is now tried first on both
  Wayland and X11. CLI tools (gnome-screenshot, scrot, maim, flameshot, etc.)
  are used as fallback only when the portal is unavailable on X11.

### Fixed

- **Stale screenshot on discard**: The portal's temp file is now deleted in
  `do_discard()`, preventing the previous screenshot from reappearing on the
  next capture.

### Dependencies

| Crate | Version | Purpose |
|---|---|---|
| image | 0.25 | Pixel-level image editing transforms |

---

## [1.1.0] -- 2026-02-16

### Added

- **Tabbed interface**: Two-tab layout (Shot / Settings) using AdwViewStack and
  AdwViewSwitcher with rounded tab buttons matching the content block radius.
- **Format selection**: Choose between PNG and JPEG output with quality 90 for
  JPEG. Persisted via GSettings.
- **Customizable watermark**: Fully configurable text overlay rendered via Cairo.
  - 5 date format presets (ISO, European, English, date-only, time-only).
  - Optional custom text prefix with pipe separator (e.g., `Brand | 2026-02-16`).
  - 4 corner positions (bottom-right, bottom-left, top-right, top-left).
  - Color selection: white text with dark shadow or black text with light shadow.
  - Responsive font size that scales with image dimensions (2% of height,
    clamped to 14--36 px).
- **Preview/crop window**: Optional post-capture preview with drag-to-crop
  selection. Crop region highlighted with dimmed overlay and dashed border.
  Save or discard from the preview header bar. Live watermark overlay in the
  preview shows the exact final appearance before saving.
- **Custom save directory**: Choose a save location via GTK FileDialog, persisted
  in GSettings. Defaults to ~/Pictures/Screenshots/.
- **Open screenshots folder**: One-click button to open the save directory in the
  system file manager.
- **Clickable notifications**: Clicking a capture notification opens the
  screenshot in the default image viewer via `gio::AppInfo::launch_default_for_uri`.
- **Notification history**: Each capture produces a unique notification ID,
  preserving all notifications in the GNOME notification center.
- **Duplicate prevention**: The XDG portal's original file is automatically
  deleted after SuperShot processes and saves its own copy, preventing duplicate
  screenshots in the system default folder.
- **Branded filenames**: Output files use the pattern
  `Screenshot_YYYY-MM-DD_HH-MM-SS.fff_supershot.ext` for unambiguous
  identification.
- **Keyboard shortcut**: `Ctrl+Enter` accelerator to trigger capture from the
  main window. Shortcut hint displayed below the capture button.

### Changed

- **Countdown display**: Moved from a separate label to inside the capture
  button, eliminating window resize during countdown.
- **Post-processing pipeline**: New `process_and_save()` replaces the old
  `save_screenshot_to_disk()`, supporting crop, watermark, and format conversion.

### Dependencies

| Crate | Version | Purpose |
|---|---|---|
| cairo-rs | 0.21 | Watermark text rendering (PNG feature) |
| gdk-pixbuf | 0.21 | Format conversion and crop |

---

## [1.0.0] -- 2026-02-11

First public release.

### Added

- **Capture modes**: Selection (area), full screen, and window capture via the
  XDG Desktop Portal screenshot interface (`ashpd` 0.12).
- **Configurable delay**: None, 3, 5, and 10-second delay options with a visual
  countdown overlay displayed in the main window.
- **Clipboard integration**: Automatic copy of the captured image to the system
  clipboard via the GDK Texture/Clipboard API.
- **Desktop notifications**: GNOME notification posted after each successful
  capture, displaying the saved file path.
- **Settings persistence**: Delay preference stored via GSettings with
  bidirectional widget binding. Schema auto-installed to
  `~/.local/share/glib-2.0/schemas/` during development builds by `build.rs`.
- **CLI headless mode**: `--now` flag for scriptable capture without GUI.
  Supports `--delay` argument.
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

[Unreleased]: https://github.com/axpnet/supershot/compare/v1.2.1...HEAD
[1.2.1]: https://github.com/axpnet/supershot/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/axpnet/supershot/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/axpnet/supershot/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/axpnet/supershot/releases/tag/v1.0.0
