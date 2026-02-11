# SuperShot

A native GTK4/Libadwaita screenshot tool for GNOME desktops, written in Rust.

SuperShot provides a minimal, focused interface for taking screenshots with a
configurable delay timer. It delegates capture to the GNOME Screenshot portal,
which handles area selection, window capture, and full-screen modes natively.
Screenshots are automatically saved and copied to the clipboard.

## Features

- **Configurable delay** -- None, 3, 5, or 10 seconds with a visual countdown
  overlay displayed in the application window.
- **Automatic clipboard copy** -- The captured image is copied to the system
  clipboard immediately after saving.
- **Desktop notifications** -- Posts a GNOME notification with the file path
  after each successful capture.
- **Settings persistence** -- The delay preference is stored via GSettings and
  restored on subsequent launches.
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

### From .deb package

```sh
sudo dpkg -i supershot_1.0.0_amd64.deb
```

The package installs the binary to `/usr/bin/supershot`, the GSettings schema,
the `.desktop` launcher, the AppStream metadata, and an application icon.
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

Opens the main application window. Set the delay if desired, then press the
capture button. The GNOME Screenshot portal opens, allowing you to select an
area, window, or full screen. The screenshot is saved to
`~/Pictures/Screenshots/` and copied to the clipboard.

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

SuperShot is structured as a standard GLib/GTK4 application:

```
src/
  main.rs      Entry point, CLI parsing (clap), dispatch
  app.rs       AdwApplication subclass, lifecycle management
  config.rs    Application constants (APP_ID, gettext domain)
  i18n.rs      GNU gettext initialization
  window.rs    AdwApplicationWindow subclass, UI template, GSettings bindings
  capture.rs   Screenshot pipeline: countdown, portal, save, clipboard, notify
```

Screenshot capture is performed through the XDG Desktop Portal (`ashpd` crate),
which communicates with the compositor via D-Bus. The portal's interactive UI
handles area, window, and full-screen selection natively.

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

axpnet -- [axp@pm.me](mailto:axp@pm.me)

Repository: [https://github.com/axpnet/supershot](https://github.com/axpnet/supershot)
