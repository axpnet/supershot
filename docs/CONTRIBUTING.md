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
    capture.rs           Screenshot pipeline: countdown, hide, portal, save, notify
    sound.rs             Shutter sound: canberra-gtk-play with paplay fallback
  data/
    com.github.axpnet.SuperShot.desktop       Desktop launcher
    com.github.axpnet.SuperShot.metainfo.xml  AppStream metadata
    com.github.axpnet.SuperShot.gschema.xml   GSettings schema
    icons/hicolor/scalable/apps/              SVG application icon
  po/
    supershot.pot         Translation template (source strings)
    LINGUAS               List of supported locale codes
    POTFILES.in           List of source files containing translatable strings
    *.po                  Per-language translation catalogs
  pkg-deb/
    DEBIAN/               Debian packaging control files
    usr/share/            Staged data files for .deb
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

5. **No zombies.** When spawning child processes (e.g., in `sound.rs`), use
   `.status()` instead of `.spawn()` to wait for the child and reap it.

6. **Gettext.** All user-visible strings must be translatable. In XML
   templates, add `translatable="yes"`. In Rust code, wrap strings with
   `gettextrs::gettext()`.

7. **Minimal changes.** Keep pull requests focused. One feature or fix per PR.

### Architecture notes

- The application uses GLib Object Subclassing (`glib::wrapper!`,
  `#[glib::object_subclass]`) for `SuperShotApp` and `SuperShotWindow`.
- The UI is defined as an inline composite template (raw XML string in
  `window.rs`). This avoids the need for a build system to bundle `.ui`
  resource files, at the cost of not being directly extractable by `xgettext`.
- Screenshot capture uses the XDG Desktop Portal via the `ashpd` crate.
  The `interactive` flag controls whether the portal shows its own selection
  UI (`true` for selection/window modes) or captures immediately (`false`
  for full screen).
- Sound playback runs in a dedicated thread to avoid blocking the GTK main
  loop. The thread is intentionally detached (fire-and-forget).

---

## Translation contributions

SuperShot uses [GNU gettext](https://www.gnu.org/software/gettext/) for
internationalization. Translations are stored as `.po` files in the `po/`
directory.

### Supported languages

The current set of supported languages is listed in `po/LINGUAS`. Each
language has a corresponding `.po` file (e.g., `po/it.po` for Italian).

### Adding a new language

1. **Copy the template:**

   ```sh
   cp po/supershot.pot po/<LANG>.po
   ```

   Replace `<LANG>` with the ISO 639-1 language code (e.g., `sv` for Swedish,
   `nl` for Dutch). Use `<LANG>_<COUNTRY>` for regional variants (e.g.,
   `pt_BR` for Brazilian Portuguese).

2. **Edit the header.** Update these fields in the new `.po` file:

   ```
   "Language: <LANG>\n"
   "Language-Team: <Language Name>\n"
   "Last-Translator: Your Name <your@email>\n"
   ```

3. **Translate all `msgstr` entries.** Each `msgid` is an English source
   string; fill in the corresponding `msgstr` with the translation. Do not
   modify `msgid` lines.

   For strings containing `%s` or `%u` placeholders, preserve the placeholders
   in the translation. They are substituted at runtime with the file path or
   countdown number respectively.

4. **Register the language.** Add the language code to `po/LINGUAS`
   (one code per line, alphabetically sorted).

5. **Test the translation.** To preview your translation without installing
   system-wide:

   ```sh
   # Compile the .po to .mo
   msgfmt po/<LANG>.po -o /usr/share/locale/<LANG>/LC_MESSAGES/supershot.mo

   # Or for local testing:
   mkdir -p ~/.local/share/locale/<LANG>/LC_MESSAGES/
   msgfmt po/<LANG>.po -o ~/.local/share/locale/<LANG>/LC_MESSAGES/supershot.mo

   # Run with the target locale
   LANGUAGE=<LANG> cargo run
   ```

6. **Submit a pull request** with the new `.po` file and the updated `LINGUAS`.

### Updating an existing translation

If the source strings change (new features, reworded UI text), the `.pot`
template will be regenerated. To update your translation:

```sh
# Merge new strings into your .po file
msgmerge --update po/<LANG>.po po/supershot.pot

# Edit po/<LANG>.po to translate any new or fuzzy entries
# (entries marked #, fuzzy need review)
```

### Translation string reference

The translatable strings are sourced from:

| Source | String type |
|---|---|
| `src/window.rs` (XML template) | UI labels, button text, combo box items |
| `src/window.rs` (Rust code) | Countdown overlay text |
| `src/capture.rs` | Notification titles/bodies, error dialog headings |
| `data/*.desktop` | Application description and generic name |
| `data/*.metainfo.xml` | AppStream summary |

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
