# Contributing to SuperShot

Thank you for your interest in contributing to SuperShot. This document covers
the project structure, development workflow, and guidelines for code and
translation contributions.

---

## Table of contents

- [Development environment](#development-environment)
- [Project structure](#project-structure)
- [Building and running](#building-and-running)
- [Code contributions](#code-contributions)
- [Translation contributions](#translation-contributions)
- [Packaging](#packaging)
- [Submitting changes](#submitting-changes)

---

## Development environment

### Prerequisites

| Tool | Purpose |
|---|---|
| Rust toolchain (stable, edition 2021) | Compiler and cargo |
| `libgtk-4-dev` | GTK 4 C headers |
| `libadwaita-1-dev` | Libadwaita C headers |
| `pkg-config` | Dependency resolution |
| `gettext` | Translation toolchain (optional, for `.po` editing) |
| `glib-compile-schemas` | GSettings schema compiler (part of `libglib2.0-bin`) |

On Ubuntu/Debian:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config gettext libglib2.0-bin
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Editor setup

Any editor with Rust Language Server (rust-analyzer) support works well.
The project has no workspace-specific IDE configuration to keep `.vscode/`
and `.idea/` out of the repository.

---

## Project structure

```
SuperShot/
  Cargo.toml             Package manifest and dependency declarations
  Cargo.lock             Pinned dependency versions
  build.rs               Build script: auto-installs GSettings schema for development
  src/
    main.rs              Entry point, CLI argument parsing (clap), dispatch
    app.rs               AdwApplication subclass, application lifecycle
    config.rs            Constants: APP_ID, gettext domain, locale path
    i18n.rs              GNU gettext initialization
    window.rs            AdwApplicationWindow subclass, composite template, GSettings
    capture.rs           Backend detection, portal and CLI capture, save pipeline
    preview.rs           Annotation and editing window
    annotate.rs          Annotation display list, Cairo rendering, redaction
    editing.rs           Non-destructive edit state and pixel operations
  data/
    com.github.axpnet.SuperShot.desktop       Desktop launcher
    com.github.axpnet.SuperShot.metainfo.xml  AppStream metadata
    com.github.axpnet.SuperShot.gschema.xml   GSettings schema
    icons/hicolor/scalable/apps/              SVG application icon
    supershot.1                               Manual page
  scripts/
    install.sh            Shared installer used by every packaging channel
    build-deb.sh          Assembles the .deb from a release build
    build-appimage.sh     Assembles the AppImage
    update-po.sh          Regenerates the template and merges the catalogs
    gen-cargo-sources.py  Regenerates cargo-sources.json for Flatpak
  po/
    supershot.pot         Translation template (source strings)
    LINGUAS               List of supported locale codes
    POTFILES.in           List of source files containing translatable strings
    *.po                  Per-language translation catalogs
  pkg-deb/
    DEBIAN/control.in     Debian control template (version and deps are derived)
    copyright             Machine-readable copyright file
  docs/
    CHANGELOG.md          Release history
    CONTRIBUTING.md       This file
```

---

## Building and running

### Development build

```sh
cargo run
```

The `build.rs` script automatically copies the GSettings schema to
`~/.local/share/glib-2.0/schemas/` and compiles it, so no manual
installation is required during development.

### Release build

```sh
cargo build --release
strip target/release/supershot
```

### Lint checks

The project maintains a zero-warning, zero-clippy-diagnostic policy:

```sh
cargo check 2>&1 | grep warning     # Must return nothing
cargo clippy 2>&1 | grep warning    # Must return nothing
```

All contributions must pass both checks before submission.

---

## Code contributions

### Guidelines

1. **Rust edition 2021.** Follow standard Rust idioms and formatting
   (`cargo fmt` with default settings).

2. **Zero warnings.** The project enforces zero compiler warnings and zero
   Clippy diagnostics. Run `cargo clippy` before submitting.

3. **Encapsulation.** Do not access `imp` module fields from outside their
   parent module. Use public methods on the wrapper type instead
   (e.g., `window.set_capture_sensitive()` rather than
   `window.imp().capture_btn.set_sensitive()`).

4. **Error handling.** GUI-facing errors must be shown via `adw::AlertDialog`.
   Diagnostic output goes to `stderr` via `eprintln!`. Never use `println!`
   in a GUI application. Never use `.unwrap()` or `.expect()` on fallible
   operations that can fail at runtime.

5. **No zombies.** When spawning child screenshot tools, use `.status()` or
   `.output()` rather than `.spawn()`, so the child is waited for and reaped.

6. **Gettext.** All user-visible strings must be translatable. In the XML
   template, add `translatable="yes"`. In Rust code, wrap strings with
   `gettextrs::gettext()` **at the literal call site** — `gettext(some_variable)`
   compiles but cannot be extracted, so the string silently stays English.

7. **No blocking the main loop.** Rendering and encoding run through
   `tokio::task::spawn_blocking`. A capture can be 4K; anything proportional to
   pixel count belongs on a worker thread.

8. **Tests for geometry.** Coordinate handling (crops, rotations, flips,
   redaction bounds) is covered by unit tests, because it cannot be verified
   without a display. Add tests when you touch it: `cargo test`.

9. **Minimal changes.** Keep pull requests focused. One feature or fix per PR.

### Architecture notes

- The application uses GLib Object Subclassing (`glib::wrapper!`,
  `#[glib::object_subclass]`) for `SuperShotApp` and `SuperShotWindow`.
- The main window's UI is an inline composite template (a raw XML string in
  `window.rs`), which avoids bundling `.ui` resources. `scripts/update-po.sh`
  extracts it by writing the template to a temporary file and running
  `xgettext --language=Glade` over it, so its `translatable="yes"` strings do
  reach the catalogs.
- Capture tries the XDG Desktop Portal on every backend, then falls back to a
  table of CLI tools filtered by the backend GDK actually connected to. The
  backend is read from GDK rather than from `WAYLAND_DISPLAY`, because
  `GDK_BACKEND=x11` in a Wayland session yields an X11 client whose environment
  still advertises Wayland.
- Portal requests are deliberately sent **without** a `WindowIdentifier`.
  Parenting one exports the window through `xdg_foreign`, which requires a live
  toplevel role; SuperShot hides its window before capturing, and exporting the
  resulting roleless surface is a protocol violation that kills the process.
- The save pipeline operates on one owned `image::RgbaImage` — edits, crop,
  redactions, vector annotations, watermark, encode — so it is `Send` and runs
  off the main thread. Text is laid out by Pango into transparent Cairo layers
  that are composited onto that image, which keeps shaping and font fallback
  correct for every script.
- The preview window owns the working image. Saving renders from it rather than
  re-reading the capture file, which is what makes an applied crop and the
  adjustments layered on it both survive to disk.
- Installation-dependent paths are resolved at runtime from the executable's
  own prefix (see `config::localedir`), so one binary works from `/usr`,
  `/app` (Flatpak), `$SNAP` or an AppImage mountpoint.

---

## Translation contributions

SuperShot uses [GNU gettext](https://www.gnu.org/software/gettext/) for
internationalization. Translations are stored as `.po` files in the `po/`
directory.

### Supported languages

The current set of supported languages is listed in `po/LINGUAS`. Each
language has a corresponding `.po` file (e.g., `po/it.po` for Italian).

### Adding a new language

1. **Register the language.** Add the ISO 639-1 code to `po/LINGUAS`, one per
   line, alphabetically sorted. Use `<LANG>_<COUNTRY>` for regional variants
   (e.g. `pt_BR` for Brazilian Portuguese).

2. **Generate the catalog.**

   ```sh
   ./scripts/update-po.sh
   ```

   The script regenerates `po/supershot.pot` from the sources and creates
   `po/<LANG>.po` for any language in `LINGUAS` that does not have one yet. It
   also merges new strings into every existing catalog and prints a per-language
   completion count.

3. **Translate every `msgstr`.** Each `msgid` is an English source string; fill
   in the corresponding `msgstr`. Never modify a `msgid`.

   Leave `translator-credits` for your own name — it is the one entry expected
   to differ per translator, and the About dialog omits the credits section
   when it is untranslated.

4. **Preserve the placeholders.** SuperShot uses named placeholders rather than
   printf specifiers, so they can be reordered freely and a mistake cannot
   crash the formatter:

   | Placeholder | Substituted with |
   |---|---|
   | `__PATH__` | Path of the saved screenshot |
   | `__N__` | A count (seconds, annotations) |
   | `__W__`, `__H__` | Width and height in pixels |
   | `__MODE__`, `__BACKEND__` | Capture mode, display server |
   | `__TOOLS__`, `__CMD__` | Suggested tools, example install command |
   | `__DETAIL__` | An underlying error message |

   Copy them verbatim, including the double underscores.

5. **Test it.** No installation is needed — `build.rs` compiles `po/*.po` into
   the target directory and the binary finds them there:

   ```sh
   LANGUAGE=<LANG> cargo run
   ```

   To test against an installed layout instead, point the binary at any
   catalog directory:

   ```sh
   SUPERSHOT_LOCALEDIR=/path/to/locale LANGUAGE=<LANG> ./target/release/supershot
   ```

6. **Submit a pull request** with the new `.po` file and the updated `LINGUAS`.

### Updating an existing translation

```sh
./scripts/update-po.sh
```

This regenerates the template and merges it into every catalog. Entries marked
`#, fuzzy` were guessed from a similar string and need review; remove the marker
once you have checked them.

CI rejects a pull request when `po/supershot.pot` is out of date, when a catalog
has untranslated entries, or when fuzzy markers remain — so run the script and
commit its output alongside any change to a user-visible string.

### Translation string reference

| Source | String type |
|---|---|
| `src/window.rs` (XML template) | Main window labels, rows, combo items |
| `src/preview.rs` | Annotation toolbar, status hints, dialogs |
| `src/capture.rs` | Notifications, error dialogs, fallback guidance |
| `src/app.rs` | Menu entries, About dialog, shortcuts window |
| `data/*.desktop` | Launcher name, comment, keywords, actions |
| `data/*.metainfo.xml` | AppStream summary |

The `.desktop` and AppStream files carry their translations inline as
`Key[lang]=` entries and `xml:lang` attributes rather than through the gettext
catalog; edit them directly.

### Tools

- [Poedit](https://poedit.net/) -- graphical `.po` editor (recommended)
- [GNOME Translation Editor](https://wiki.gnome.org/Apps/Gtranslator) --
  GNOME-native alternative
- Any text editor -- `.po` files are plain text with a simple format

---

## Packaging

### .deb package

The `pkg-deb/` directory contains the Debian packaging structure. To build:

```sh
cargo build --release
strip target/release/supershot
mkdir -p pkg-deb/usr/bin
cp target/release/supershot pkg-deb/usr/bin/

# Copy data files to pkg-deb/usr/share/ (must mirror data/ exactly)
cp data/com.github.axpnet.SuperShot.desktop    pkg-deb/usr/share/applications/
cp data/com.github.axpnet.SuperShot.gschema.xml pkg-deb/usr/share/glib-2.0/schemas/
cp data/com.github.axpnet.SuperShot.metainfo.xml pkg-deb/usr/share/metainfo/
cp data/icons/hicolor/scalable/apps/com.github.axpnet.SuperShot.svg \
   pkg-deb/usr/share/icons/hicolor/scalable/apps/

dpkg-deb --build pkg-deb supershot_<VERSION>_amd64.deb
```

**Important:** The files in `pkg-deb/usr/share/` must always be exact copies
of the canonical files in `data/`. Never edit the `pkg-deb/` copies directly.

### Flatpak

A Flatpak manifest is planned for Flathub submission. Contributions to the
`com.github.axpnet.SuperShot.yml` manifest are welcome.

### Snap / AppImage

Snap and AppImage packaging is under consideration. If you have experience
with either format, contributions are especially welcome.

---

## Submitting changes

1. Fork the repository on GitHub.
2. Create a feature branch: `git checkout -b feature/your-feature`
3. Make your changes, ensuring `cargo clippy` and `cargo check` pass cleanly.
4. Commit with a clear message describing the change.
5. Open a pull request against `main`.

For translation-only contributions, the PR should include:
- The new or updated `.po` file(s) in `po/`
- An updated `po/LINGUAS` if adding a new language
- No other file changes

---

## Questions

Open an issue on [GitHub](https://github.com/axpnet/supershot/issues) or
contact the maintainer at [axp@pm.me](mailto:axp@pm.me).
