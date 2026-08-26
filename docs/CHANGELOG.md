# Changelog

All notable changes to SuperShot are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Version numbering adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [1.3.0] -- 2026-08-26

Annotation, redaction, and a compatibility audit across Linux desktop
environments and distributions.

### Added

- **Annotation toolbar**: arrow, rectangle, ellipse, highlighter, free draw,
  text labels and numbered step markers, with an eight-colour palette and an
  adjustable thickness that scales with the capture's resolution.
- **Redaction**: pixelate and blackout tools that overwrite the underlying
  pixels in the image buffer instead of drawing over them, so nothing sensitive
  survives in the saved file. Blur is deliberately not offered as a redaction
  tool: it attenuates information rather than removing it, and is frequently
  reversible for text.
- **Undo and redo** across annotations, crops and adjustments, with `Ctrl+Z`,
  `Ctrl+Shift+Z` and `Ctrl+Y`.
- **Clipboard-only export**: `Ctrl+C` in the preview renders the annotated
  image straight to the clipboard without writing a file.
- **Explicit capture modes** — ask, area, window, whole screen — in the Shot
  tab and via `--mode` on the command line. Required for the CLI fallbacks,
  which have no chooser of their own.
- **Configurable JPEG quality** (1-100), in Settings and via `--quality`.
- **About dialog**, reachable from a new primary menu in the main window,
  showing the running version plus a troubleshooting section carrying the
  session details a bug report needs. The main window is 100 px taller so the
  dialog opens inside it rather than as a clipped bottom sheet.
- **`--doctor`** prints those same diagnostics: display server, desktop,
  packaging channel, catalog directory, portal availability, and which
  fallback screenshot tools are installed.
- **Keyboard shortcuts window**, and `--output`, `--format` and `--watermark`
  command-line flags.
- **arm64 packages** and an **AppImage** build for both architectures.
- **Desktop entry actions** for area, window and full-screen capture, reachable
  from the launcher's context menu.
- **`--edit FILE`** opens the annotation editor on an image already on disk,
  without capturing anything.
- **Test suite** (43 tests) covering the geometry and save-pipeline logic that
  cannot be exercised without a display: annotation coordinate handling across
  crops, rotations and flips; redaction actually destroying pixels; the
  Cairo/`image` conversions and their premultiplied, stride-padded layout; and
  save-path resolution and fallback. Run in CI.
- `scripts/install.sh`, one installer shared by every packaging channel;
  `scripts/build-deb.sh`, `scripts/build-appimage.sh`,
  `scripts/gen-cargo-sources.py` and `scripts/update-po.sh`.

### Fixed

- **Cropping then saving wrote the uncropped image.** `apply_crop()` baked the
  crop into an in-memory pixbuf and reset the edit state, while `do_save()`
  independently re-read the original file from disk and re-applied the
  now-empty edit state. The crop and every adjustment made before it were
  silently discarded. The preview window now owns the working image and saves
  from it.
- **None of the 14 translations were ever active.** No build step compiled
  `po/*.po`, no package installed the catalogs, and `LOCALEDIR` was hard-coded
  to `/usr/share/locale` — wrong for Flatpak, Snap, AppImage and any non-`/usr`
  prefix. Catalogs are now compiled by `build.rs`, installed by every channel,
  and located at runtime from the executable's own prefix.
- **Most of the interface was not translatable.** The catalog covered 17
  strings; the entire preview window, every dialog and the file chooser were
  hard-coded English. It now covers 122 strings, all translated in all 14
  languages. The adjustment sliders also dispatched on their English label
  text, which would have broken the moment those labels were translated.
- **Watermark and annotation text now uses Pango** instead of Cairo's "toy"
  text API, which performs no shaping and no font fallback. Japanese, Chinese,
  Korean, Devanagari and right-to-left text previously rendered as empty boxes.
- **Wayland sessions without a Screenshot portal had no recovery path.** The
  CLI fallback was X11-only. `grim`+`slurp`, `spectacle`, `wayshot` and
  `hyprshot` are now tried on Wayland, and the tool table is ordered so each
  desktop gets its native tool first.
- **The display backend is read from GDK**, not from `WAYLAND_DISPLAY`. Running
  with `GDK_BACKEND=x11` inside a Wayland session gave a process talking X11
  through XWayland while the environment still advertised Wayland, which
  disabled the X11 fallbacks it could actually have used.
- **The "install gnome-screenshot with apt" error message** was wrong on
  Fedora, Arch, openSUSE, Alpine and NixOS, and wrong everywhere inside
  Flatpak or Snap, where host binaries are unreachable regardless of what the
  user installs. The message now names the tools that suit the running
  backend, detects the distribution from `/etc/os-release`, and tells sandboxed
  builds to install a portal backend instead.
- **URIs are built through GIO** rather than by concatenating `"file://"` with
  a path. The old form broke on any directory containing a space, `#`, `%` or a
  non-ASCII character — routine for a localized Pictures folder.
- **A missing xdg-user-dirs configuration no longer discards the capture.**
  `dirs::picture_dir()` returns `None` on minimal installs and bare window
  managers, which aborted the save after the screenshot had already been taken.
  The save directory now falls back through the configured path, the XDG
  pictures directory, `$HOME/Pictures` and finally a temporary directory, and
  is probed for write access rather than assumed writable.
- **Temporary captures use a private 0700 directory** created with `create_dir`
  (which fails rather than follows an existing symlink), preferring
  `XDG_RUNTIME_DIR`. The previous predictable `/tmp/supershot_<pid>.png` was
  open to a symlink attack on a shared machine.
- **Rendering and encoding run off the GTK main thread.** The previous code
  encoded synchronously immediately after destroying the preview window,
  leaving the user with no window and a blocked main loop. Live editing now
  works on a bounded-size copy with debounced recomputation, replacing a
  per-pixel `put_pixel` loop over an `unsafe` pixbuf borrow that ran at full
  resolution on every slider tick.
- **Headless mode** now copies to the clipboard and posts a notification, as
  the documentation had always claimed, and no longer creates a stray main
  window behind the capture.
- **The watermark timestamp** shown in the preview is the one written to the
  file. `Local::now()` was evaluated separately per render, so the `HH:MM:SS`
  preset displayed one time and saved another.
- **Annotations follow the image.** They are carried through a crop applied at
  save time, and through rotations and flips — including the case where a
  single active flip conjugates the rotation and reverses its apparent
  direction on screen.
- **The application had no way to show its own version.** Added an About
  dialog, reachable from a new primary menu, alongside a keyboard shortcuts
  window.
- **The preview window left a wide empty band beside the image.** The
  adjustments panel sat in a `GtkRevealer` whose child was a `GtkScrolledWindow`;
  a scrolled window expands horizontally by default and a revealer propagates
  its child's expand flag, so although the collapsed revealer *measured* zero it
  was *allocated* half of all spare width — 788 px in a 1629 px window — pushing
  the canvas to the right of an empty gap.
- **The preview window forced itself to be 1629 px wide.** Ten tools, eight
  colour swatches, a thickness slider and a text field in one top toolbar set
  that as the window's minimum width, so the window was always far wider than
  the capture inside it.
- **`build.rs` no longer writes into `$HOME` for release builds**, which made
  builds non-reproducible and polluted the synthetic home directories used by
  distribution, Flatpak and Snap build environments.
- **Flatpak manifest**: built from tag `v1.1.0` while the project was at 1.2.4;
  targeted the obsolete GNOME 47 runtime; declared no `--filesystem`, so the
  sandboxed app could not write the screenshot it had just taken; and held a
  `--talk-name` for `org.freedesktop.portal.Screenshot`, which is an interface,
  not a bus name, and therefore matched nothing.
- **Debian package**: the control file declared `1.2.3` while `Cargo.toml` said
  `1.2.4`, and its dependency list named only GTK and libadwaita, omitting
  cairo, pango, gdk-pixbuf and glibc. Version and architecture are now derived,
  dependencies come from `dpkg-shlibdeps`, and the package passes `lintian`
  with no errors or warnings — it also gained a manual page, a
  machine-readable copyright file and a changelog.
- **Desktop entry**: added `DBusActivatable`, `StartupWMClass`,
  `X-GNOME-UsesNotifications`, translated `GenericName`, `Comment` and
  `Keywords` in all 14 languages, and per-mode launcher actions.
- **Snap**: added the `removable-media` plug for save directories on external
  drives, dropped the unused `audio-playback` permission, and declared arm64.

### Changed

- **The preview toolbar became a sidebar.** Tools, colours, thickness, the
  label field, undo/redo and the adjustments section now sit in a vertically
  scrolling `AdwOverlaySplitView` sidebar that collapses into an overlay below
  640 px. Tool labels ellipsize so the widest of fourteen translations cannot
  set the sidebar's width, zoom moved to the status bar, and the header carries
  only actions. The window's minimum width went from 1629 px to 365 px, and it
  now adapts to its content rather than the other way round.
- Portal requests are documented as deliberately unparented. Parenting one
  means exporting the window through `xdg_foreign`, which requires a live
  toplevel role; SuperShot hides its window before every capture so it does not
  appear in its own screenshot, and exporting the resulting roleless surface is
  a protocol violation that terminates the process. With the window hidden
  there is also no parent for the dialog to sit above, and portals identify the
  caller from its D-Bus credentials rather than from a window handle.
- The save pipeline is a single `image::RgbaImage` flow (edits, crop,
  redactions, vector annotations, watermark, encode), removing the GdkPixbuf
  and Cairo round-trips and the `unsafe` block that came with them.
- Preview edits are computed against a copy bounded to 1600 px on its longest
  edge, with 120 ms debouncing; saves always re-run at full resolution.
- The compositor settling delay before a capture is 250 ms, up from 200 ms.

### Dependencies

- `gettext-rs` 0.7 → 0.8, resolving **RUSTSEC-2026-0244** (`setlocale` accessing
  the environment without synchronization). `setlocale` is now `unsafe` and is
  called exactly once, before any thread exists.
- `cargo update` across the tree, resolving **RUSTSEC-2026-0221**
  (`event-listener` allowing `!Send` tags to cross thread boundaries).
- `image` now builds with `default-features = false` and only `png`, `jpeg` and
  `rayon`. This drops the AVIF and OpenEXR codecs, which SuperShot never used,
  taking the compiled dependency graph from 266 crates to 194 and removing
  **RUSTSEC-2024-0436** (`paste`, unmaintained) from the build entirely. The
  advisory survives only as a feature-agnostic entry in `Cargo.lock`, recorded
  with its justification in `.cargo/audit.toml`.
- Added `pango` and `pangocairo` for text layout.
- `ashpd` builds with `default-features = false` and only `screenshot` and
  `tokio`. The `gtk4_wayland` and `gtk4_x11` features existed solely to
  construct a `WindowIdentifier`, which SuperShot cannot use (see above).
- `rust-version = "1.83"` declared, replacing the README's inaccurate claim of
  1.70.

### Audit

- `cargo audit`: 0 vulnerabilities, 0 unmaintained crates in the build graph.
- `cargo clippy`: clean.
- `lintian`: clean (no errors, no warnings).
- `desktop-file-validate` and `appstreamcli validate`: clean.
- All 14 catalogs: 122/122 strings translated, verified loading at runtime from
  an installed prefix.

---

## [1.2.3] -- 2026-05-26

### Security

- **Snap rebuild**: Snap rebuilt to incorporate Ubuntu Security Notice
  USN-8269-1 (`libavahi-client3`, `libavahi-common-data`, `libavahi-common3`) in the runtime stage.

---

## [1.2.2] -- 2026-05-06

### Security

- **Snap rebuild**: Snap revisions r3-r6 rebuilt to incorporate Ubuntu
  Security Notices USN-8227-1 (`libcurl3t64-gnutls`) and USN-8233-1
  (`libnghttp2-14`) in the runtime stage.

### Fixed

- **`pixbuf_to_dynamic` bounds safety**: The pixbuf-to-`DynamicImage`
  conversion now validates the pixel buffer length against the expected
  `rowstride * height` size and rejects malformed inputs (zero or negative
  dimensions, fewer than three channels) instead of risking out-of-bounds
  indexing on the unsafe `pixels()` slice.
- **Crop rectangle clamping**: The "apply crop" path now clamps the crop
  origin to the valid pixbuf rect before calling `new_subpixbuf`, removing
  a panic path on extreme crop coordinates.
- **Watermark text length cap**: Custom watermark text is truncated to 256
  characters at persist time to avoid pathological Cairo rendering and
  unbounded GSettings strings.
- **Save directory canonicalization**: Paths chosen via the folder picker
  are canonicalized before being persisted to GSettings, normalizing
  symlinks and relative components.
- **Numerical correctness**: Rotation arithmetic uses `rem_euclid` (correct
  for negative input); zoom percentage is rounded instead of truncated.

### Changed

- **Patch dependency bumps**: `cargo update` to refresh transitive crates
  (`tokio` 1.52.1 → 1.52.2, `zvariant` 5.10.1 → 5.11.0, `profiling` 1.0.17
  → 1.0.18). All direct dependencies remain at the latest stable releases
  (gtk4 0.11.3, libadwaita 0.9.1, ashpd 0.13.10, image 0.25.10, clap 4.6.1).

### Audit

- `cargo audit`: 0 RUSTSEC vulnerabilities. One unmaintained-crate warning
  (`paste` 1.0.15) propagated transitively through `image → ravif → rav1e`;
  no known security impact, tracked upstream.
- `cargo clippy --release -- -D warnings`: clean. Removed manual `Default`
  impl on `CaptureOptions`, collapsed nested-if in flameshot capture path,
  switched manual min/max watermark font clamp to `f64::clamp`.

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

[Unreleased]: https://github.com/axpnet/supershot/compare/v1.2.3...HEAD
[1.2.3]: https://github.com/axpnet/supershot/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/axpnet/supershot/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/axpnet/supershot/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/axpnet/supershot/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/axpnet/supershot/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/axpnet/supershot/releases/tag/v1.0.0
