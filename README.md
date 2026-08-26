# SuperShot

<p align="center">
  <img src="data/icons/hicolor/scalable/apps/com.github.axpnet.SuperShot.svg" width="128" height="128" alt="SuperShot icon" />
</p>

<p align="center">
  <strong>A native GTK4/Libadwaita screenshot tool for Linux desktops, written in Rust.</strong>
</p>

<p align="center">
  <a href="https://github.com/axpnet/supershot/releases"><img src="https://img.shields.io/github/v/release/axpnet/supershot" alt="Release" /></a>
  <a href="https://snapcraft.io/supershot"><img src="https://img.shields.io/badge/snap-supershot-blue?logo=snapcraft" alt="Snap" /></a>
  <img src="https://img.shields.io/github/license/axpnet/supershot" alt="License" />
  <img src="https://img.shields.io/badge/rust-edition%202021-orange?logo=rust" alt="Rust" />
</p>

---

SuperShot provides a focused interface for taking screenshots, marking them up,
and sharing them. Capture goes through the XDG Desktop Portal on both Wayland
and X11, so it works on GNOME, KDE Plasma, and any compositor that ships a
portal backend; where none is available it falls back to the command-line
screenshot tools the session already has. Screenshots are saved and copied to
the clipboard automatically.

## Features

### Annotate
- **Annotation tools** -- arrow, rectangle, ellipse, highlighter, free draw,
  text labels, and numbered step markers for walking a reader through a UI.
  Eight-colour palette and adjustable thickness that scales with the capture's
  resolution.
- **Redaction** -- pixelate and blackout tools that **overwrite the underlying
  pixels** rather than drawing over them, so nothing sensitive survives in the
  saved file. Blur is deliberately not offered here: it attenuates information
  rather than removing it, and is frequently reversible for text.
- **Undo and redo** across annotations, crops, and adjustments.
- **Copy without saving** -- `Ctrl+C` in the preview renders the annotated
  image straight to the clipboard, ready to paste into a chat or issue tracker.

### Capture
- **Capture modes** -- ask, area, window, or the whole screen. Selectable in the
  interface and on the command line.
- **Configurable delay** -- none, 3, 5, or 10 seconds, with a visual countdown
  shown inside the capture button.
- **Portal-first, everywhere** -- the XDG Desktop Portal is tried on every
  backend. When no portal answers, SuperShot uses whichever tool the session
  provides: `grim`+`slurp`, `spectacle`, `wayshot` or `hyprshot` on Wayland;
  `gnome-screenshot`, `xfce4-screenshooter`, `spectacle`, `scrot`, `maim`,
  `flameshot` or ImageMagick's `import` on X11.
- **Duplicate prevention** -- the portal's own copy is removed after
  processing, so no duplicate appears in the system screenshot folder.

### Edit
- **Crop** -- drag a selection with a live dimension badge, then click inside it
  or press Apply Crop.
- **Image editor** -- rotate, flip, brightness, contrast, blur, sharpen,
  grayscale, and invert, applied live in the preview before saving.
- **Watermark** -- optional text overlay rendered with Pango, with 5 date format
  presets, custom text, 4 corner positions, and white or black with a
  contrasting shadow. Rendered live in the preview exactly as it will be saved,
  timestamp included.

### Output
- **PNG or JPEG**, with configurable JPEG quality.
- **Custom save directory** via folder picker, with one-click access to it.
- **Automatic clipboard copy** after each capture.
- **Desktop notifications** with actions to open the image or reveal it in the
  file manager.

### General
- **Settings persistence** via GSettings.
- **CLI headless mode** for scripting and keyboard shortcuts.
- **Session diagnostics** -- `supershot --doctor` reports the display server,
  desktop, packaging channel, portal availability, and which fallback tools are
  installed. The same information is in the About dialog.
- **14 languages** via GNU gettext.

## Requirements

### Runtime

| Dependency | Minimum version | Notes |
|---|---|---|
| GTK 4 | 4.14 | |
| Libadwaita | 1.5 | |
| An XDG Desktop Portal backend | -- | `xdg-desktop-portal-gnome`, `-kde`, `-wlr` or `-gtk` |

Without a portal backend, SuperShot falls back to any of `grim`+`slurp`,
`spectacle`, `wayshot`, `hyprshot`, `gnome-screenshot`, `xfce4-screenshooter`,
`scrot`, `maim`, `flameshot`, or ImageMagick's `import`. Run
`supershot --doctor` to see what the running session offers.

### Build

| Tool | Version |
|---|---|
| Rust toolchain | 1.92+ (edition 2021) |
| pkg-config | -- |
| GTK 4 development headers | `libgtk-4-dev` |
| Libadwaita development headers | `libadwaita-1-dev` |
| GNU gettext | for `msgfmt`; without it the build is English-only |

On Debian and Ubuntu:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libglib2.0-dev pkg-config gettext
```

On Fedora:

```sh
sudo dnf install gtk4-devel libadwaita-devel glib2-devel pkgconf-pkg-config gettext
```

On Arch Linux:

```sh
sudo pacman -S gtk4 libadwaita glib2 pkgconf gettext
```

## Building from source

```sh
git clone https://github.com/axpnet/supershot.git
cd supershot
cargo build --release
```

The release binary is produced at `target/release/supershot`.

During development builds `build.rs` installs the GSettings schema into
`~/.local/share/glib-2.0/schemas/` and compiles it, so `cargo run` works without
a system-wide installation step. Release builds do not touch `$HOME`; set
`SUPERSHOT_NO_DEV_SCHEMA=1` to disable it for debug builds too.

## Installation

### From Snap Store

```sh
sudo snap install supershot
```

[![Get it from the Snap Store](https://snapcraft.io/static/images/badges/en/snap-store-black.svg)](https://snapcraft.io/supershot)

### From .deb package

Download the latest `.deb` for your architecture (`amd64` or `arm64`) from the
[Releases](https://github.com/axpnet/supershot/releases) page:

```sh
sudo apt install ./supershot_1.3.0_amd64.deb
```

Using `apt` rather than `dpkg -i` lets it resolve the runtime dependencies.

### AppImage

For distributions whose GTK 4 or libadwaita is older than SuperShot needs, and
for immutable systems:

```sh
chmod +x SuperShot-1.3.0-x86_64.AppImage
./SuperShot-1.3.0-x86_64.AppImage
```

### From source

```sh
cargo build --release
sudo ./scripts/install.sh /
sudo glib-compile-schemas /usr/share/glib-2.0/schemas/
sudo update-desktop-database /usr/share/applications/
```

`scripts/install.sh DESTDIR [PREFIX]` is the same installer every packaging
channel uses, so a manual install gets exactly what a package would: binary,
desktop entry, GSettings schema, AppStream metadata, icons, man page, and all
14 translation catalogs.

## Usage

### GUI mode

```sh
supershot
```

Opens the main window with two tabs:

- **Shot** -- choose what to capture (ask, area, window, whole screen), the
  delay, and whether to open the preview. Press the circular button, or
  `Ctrl+Enter`.
- **Settings** -- watermark (enable, date format, custom text, position,
  colour), output format and JPEG quality, and the save directory.

With **Preview** enabled, the capture opens in the annotation window. The same
window can be opened on an image already on disk:

```sh
supershot --edit ~/Pictures/Screenshots/shot.png
```

Tools live in a sidebar that collapses into an overlay on narrow windows:

| Tool | What it does |
|---|---|
| Crop | Drag a region, then click inside it or press Apply Crop |
| Arrow, Box, Circle | Drag to draw |
| Marker | Translucent highlight over a region |
| Pen | Freehand drawing |
| Text | Type in the toolbar field, then click to place the label |
| Step | Click to drop the next numbered marker |
| Pixelate, Redact | Drag over sensitive data -- the pixels are destroyed, not covered |

| Shortcut | Action |
|---|---|
| `Ctrl+S` | Save |
| `Ctrl+C` | Copy the annotated image to the clipboard without saving |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo |
| `Escape` | Cancel the pending selection |

### CLI headless mode

```sh
supershot --now --mode area --delay 3
```

Captures without showing the interface and prints the saved path to standard
output, which makes it suitable for a desktop keyboard shortcut or a script.

| Flag | Description | Default |
|---|---|---|
| `-d`, `--delay` | Seconds before capture | `0` |
| `--now` | Headless mode, no GUI | `false` |
| `-m`, `--mode` | `interactive`, `area`, `window`, `screen` | `interactive` |
| `-f`, `--format` | `png`, `jpeg` | `png` |
| `--quality` | JPEG quality, 1-100 | `90` |
| `-o`, `--output` | Save directory | configured location |
| `--watermark` | Overlay the configured watermark | `false` |
| `-e`, `--edit` | Annotate an existing image instead of capturing | |
| `--doctor` | Print session diagnostics and exit | |

### Troubleshooting

```sh
supershot --doctor
```

Reports the display server GDK actually connected to, the desktop, the
packaging channel, where translation catalogs were found, whether a screenshot
portal is on the bus, and which fallback tools are installed. Include its output
in bug reports.

## Translations

SuperShot ships with 14 language translations via GNU gettext. The interface
defaults to English; translations are activated automatically when the system
locale matches a supported language.

| Language | Code | Status |
|---|---|---|
| English | `en` | Source language |
| Italian | `it` | Complete |
| French | `fr` | Complete |
| German | `de` | Complete |
| Spanish | `es` | Complete |
| Portuguese | `pt` | Complete |
| Russian | `ru` | Complete |
| Polish | `pl` | Complete |
| Turkish | `tr` | Complete |
| Japanese | `ja` | Complete |
| Korean | `ko` | Complete |
| Chinese (Simplified) | `zh_CN` | Complete |
| Chinese (Traditional) | `zh_TW` | Complete |
| Hindi | `hi` | Complete |
| Indonesian | `id` | Complete |

All 122 interface strings are translated in every language listed above.

To add or update a translation, run `scripts/update-po.sh` to refresh
`po/supershot.pot` and merge it into every catalog, then edit `po/<lang>.po`.
The script extracts both the `gettext()` calls in the Rust sources and the
`translatable="yes"` attributes in the GtkBuilder template. CI fails if the
template is out of date. See [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md).

## Architecture

SuperShot is a standard GLib/GTK4 application using the object-subclassing
pattern with an inline composite template:

```
src/
  main.rs      Entry point, CLI parsing (clap), dispatch
  app.rs       AdwApplication subclass: lifecycle, actions, About, shortcuts
  config.rs    Identifiers, and runtime resolution of installation-dependent paths
  i18n.rs      GNU gettext initialization
  window.rs    Main window: tabbed UI (AdwViewStack), GSettings bindings
  capture.rs   Backend detection, portal and CLI capture, save pipeline, watermark
  preview.rs   Annotation and editing window
  annotate.rs  Annotation display list, Cairo rendering, redaction
  editing.rs   Non-destructive edit state and pixel operations
```

**Capture.** The XDG Desktop Portal (via `ashpd`) is tried first on every
backend, parented to the SuperShot window so its dialog cannot open behind or
unfocused. When no portal answers, a table of CLI tools is consulted, filtered
by the backend GDK actually connected to and by which capture mode each tool
supports -- read from GDK rather than from `WAYLAND_DISPLAY`, because
`GDK_BACKEND=x11` inside a Wayland session produces an X11 client whose
environment still advertises Wayland.

**Save pipeline.** Everything after capture operates on a single owned
`image::RgbaImage`, in this order: edits, crop, redactions (which overwrite
pixels), vector annotations, watermark, encode. The whole pipeline is `Send`
and runs on a worker thread. Watermark and annotation text are laid out by
Pango and rendered into transparent Cairo layers that are composited onto the
image, so shaping and font fallback work for every script.

**Preview.** Live editing runs against a copy of the image bounded to 1600 px
on its longest edge, with debounced recomputation; saving re-runs the same
edits at full resolution. The preview window owns the working image, which is
what makes a crop and the adjustments layered on it both survive to disk.

**Tests.** `cargo test` covers the parts that cannot be checked by clicking
through the interface: annotation coordinates across crops, rotations and
flips; redaction destroying pixels rather than covering them; the Cairo and
`image` conversions, including Cairo's premultiplied alpha and padded row
stride; and save-directory resolution and fallback.

**Paths.** Locale catalogs are located at runtime from the executable's own
prefix, with Flatpak, Snap and `SUPERSHOT_LOCALEDIR` taking precedence, so one
binary works from `/usr`, `/app`, `$SNAP` or an AppImage mountpoint.

## Contributing

Contributions are welcome. Please open an issue to discuss proposed changes
before submitting a pull request. See [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md)
for the development workflow and translation guide.

## Support

If you find SuperShot useful, consider supporting its development:

- **GitHub Sponsors**: [github.com/sponsors/axpnet](https://github.com/sponsors/axpnet)
- **Buy Me a Coffee**: [buymeacoffee.com/axpnet](https://buymeacoffee.com/axpnet)

## License

This project is licensed under the GNU General Public License v3.0 or later.
See the [LICENSE](LICENSE) file for details.

## Author

axpnet -- [github.com/axpnet](https://github.com/axpnet)

Repository: [https://github.com/axpnet/supershot](https://github.com/axpnet/supershot)
