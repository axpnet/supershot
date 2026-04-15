# SuperShot

<p align="center">
  <img src="data/icons/hicolor/scalable/apps/com.github.axpnet.SuperShot.svg" width="128" height="128" alt="SuperShot icon" />
</p>

<p align="center">
  <strong>A native GTK4/Libadwaita screenshot tool for GNOME desktops, written in Rust.</strong>
</p>

<p align="center">
  <a href="https://github.com/axpnet/supershot/releases"><img src="https://img.shields.io/github/v/release/axpnet/supershot" alt="Release" /></a>
  <a href="https://snapcraft.io/supershot"><img src="https://img.shields.io/badge/snap-supershot-blue?logo=snapcraft" alt="Snap" /></a>
  <img src="https://img.shields.io/github/license/axpnet/supershot" alt="License" />
  <img src="https://img.shields.io/badge/rust-edition%202021-orange?logo=rust" alt="Rust" />
</p>

---

SuperShot provides a minimal, focused interface for taking screenshots with a
configurable delay timer. It delegates capture to the GNOME Screenshot portal,
which handles area selection, window capture, and full-screen modes natively.
Screenshots are automatically saved and copied to the clipboard.

## Features

### Capture
- **Configurable delay** -- None, 3, 5, or 10 seconds with a visual countdown
  displayed inside the capture button.
- **Format selection** -- Save as PNG (lossless) or JPEG (quality 90).
- **Preview and crop** -- Optional post-capture preview window with
  drag-to-select crop region, click inside selection to apply, and undo crop
  to retry. The watermark is rendered as a live overlay in the preview, showing
  exactly how the final image will appear.
- **Image editor** -- Full editing sidebar with rotate, flip, brightness,
  contrast, blur, sharpen, grayscale, and invert controls. All edits are
  applied live in the preview before saving.
- **Duplicate prevention** -- The portal's original file is automatically
  removed after processing, preventing duplicate screenshots in the system
  default folder.
- **Portal-first capture** -- Uses the XDG Desktop Portal on both Wayland and
  X11 (with GNOME portal backend), falling back to CLI tools only when the
  portal is unavailable.

### Watermark
- **Customizable watermark** -- Optional text overlay rendered via Cairo with
  configurable date format, custom text, corner position, and color.
- **5 date format presets** -- `YYYY-MM-DD HH:MM:SS`, `DD/MM/YYYY HH:MM`,
  `Mon DD, YYYY HH:MM`, `YYYY-MM-DD` (date only), `HH:MM:SS` (time only).
- **Custom text** -- Prepend a brand name or label to the date stamp
  (e.g., `MyBrand | 2026-02-16 14:30:25`).
- **4 corner positions** -- Bottom-right, bottom-left, top-right, top-left.
- **Color selection** -- White text with dark shadow (default) or black text
  with light shadow, for optimal legibility on any background.

### Output
- **Custom save directory** -- Choose where screenshots are saved via folder
  picker. Defaults to `~/Pictures/Screenshots/`.
- **Open screenshots folder** -- One-click access to the save directory in the
  file manager.
- **Automatic clipboard copy** -- The captured image is copied to the system
  clipboard immediately after saving.
- **Desktop notifications** -- Posts a GNOME notification with the file path
  after each successful capture. Clicking the notification opens the screenshot
  in the default image viewer.

### General
- **Tabbed interface** -- Clean two-tab layout: Shot (capture controls) and
  Settings (watermark, format, output configuration).
- **Settings persistence** -- All preferences stored via GSettings and restored
  on launch.
- **CLI headless mode** -- Capture screenshots from the command line without
  displaying the GUI, suitable for scripting and automation.
- **14 languages** -- Translations via GNU gettext for the most widely spoken
  languages.

## Requirements

### Runtime

| Dependency | Minimum version |
|---|---|
| GTK 4 | 4.14 |
| Libadwaita | 1.5 |
| xdg-desktop-portal-gnome | -- |
| glib-compile-schemas | -- |

### Build

| Tool | Version |
|---|---|
| Rust toolchain | 1.70+ (edition 2021) |
| pkg-config | -- |
| GTK 4 development headers | `libgtk-4-dev` |
| Libadwaita development headers | `libadwaita-1-dev` |

On Ubuntu/Debian:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config
```

## Building from source

```sh
git clone https://github.com/axpnet/supershot.git
cd supershot/SuperShot
cargo build --release
```

The release binary is produced at `target/release/supershot`.

During development builds, the `build.rs` script automatically installs the
GSettings XML schema into `~/.local/share/glib-2.0/schemas/` and compiles it,
so `cargo run` works without a system-wide installation step.

## Installation

### From Snap Store (recommended)

```sh
sudo snap install supershot
```

[![Get it from the Snap Store](https://snapcraft.io/static/images/badges/en/snap-store-black.svg)](https://snapcraft.io/supershot)

### From .deb package

Download the latest `.deb` from the
[Releases](https://github.com/axpnet/supershot/releases) page:

```sh
sudo dpkg -i supershot_1.2.0_amd64.deb
```

The package installs the binary to `/usr/bin/supershot`, the GSettings schema,
the `.desktop` launcher, the AppStream metadata, and the application icon.
Post-installation scripts compile the GSettings schema and update the icon
cache automatically.

### Manual installation

```sh
sudo install -Dm755 target/release/supershot /usr/bin/supershot
sudo install -Dm644 data/com.github.axpnet.SuperShot.desktop /usr/share/applications/
sudo install -Dm644 data/com.github.axpnet.SuperShot.gschema.xml /usr/share/glib-2.0/schemas/
sudo install -Dm644 data/com.github.axpnet.SuperShot.metainfo.xml /usr/share/metainfo/
sudo install -Dm644 data/icons/hicolor/scalable/apps/com.github.axpnet.SuperShot.svg /usr/share/icons/hicolor/scalable/apps/
sudo glib-compile-schemas /usr/share/glib-2.0/schemas/
```

## Usage

### GUI mode

```sh
supershot
```

Opens the main application window with two tabs:

- **Shot** -- Configure delay and preview toggle, then press the circular
  capture button. The GNOME Screenshot portal opens, allowing you to select
  an area, window, or full screen.
- **Settings** -- Configure watermark (enable, date format, custom text,
  position, color), output format (PNG/JPEG), and save directory.

### CLI headless mode

```sh
supershot --now --delay 5
```

Opens the portal screenshot UI after a 5-second delay without displaying the
SuperShot window.

| Flag | Description | Default |
|---|---|---|
| `--delay` | Seconds before capture | `0` |
| `--now` | Headless mode (no GUI) | `false` |

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

New translations are welcome. See [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md)
for the translation workflow.

## Architecture

SuperShot is structured as a standard GLib/GTK4 application using the
object-subclassing pattern with inline composite templates:

```
src/
  main.rs      Entry point, CLI argument parsing (clap), dispatch
  app.rs       AdwApplication subclass, lifecycle and action management
  config.rs    Application constants (APP_ID, gettext domain)
  i18n.rs      GNU gettext initialization
  window.rs    Main window with tabbed UI (AdwViewStack), GSettings bindings
  capture.rs   Capture pipeline: countdown, portal, watermark, crop, save, clipboard, notify
  preview.rs   Preview/crop window with Cairo rendering and live watermark overlay
  editing.rs   Non-destructive image editing (rotate, flip, brightness, contrast, blur, sharpen, grayscale, invert)
```

Screenshot capture is performed through the XDG Desktop Portal (`ashpd` crate),
which communicates with the compositor via D-Bus. The portal is tried first on
both Wayland and X11; CLI tools (gnome-screenshot, scrot, etc.) are used as
fallback only when the portal is unavailable on X11. Post-capture processing
(editing, watermark, crop, format conversion) uses the `image` crate, Cairo,
and GdkPixbuf.

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
