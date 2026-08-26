// SuperShot - Screenshot capture pipeline
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Orchestrates the full capture workflow: optional countdown, window hiding,
// XDG Desktop Portal screenshot request, post-processing (edits, crop,
// annotations, watermark, format conversion), file persistence, clipboard copy
// and desktop notification. Provides both GUI-driven and headless (CLI) paths.
//
// Capture strategy
// ----------------
// The XDG Screenshot portal is tried first on every backend, because it is the
// only mechanism that works under a locked-down Wayland compositor and the only
// one available inside Flatpak/Snap confinement. When the portal is missing or
// refuses the request, SuperShot falls back to whichever CLI screenshot tool the
// session actually provides — a different set on Wayland (grim/slurp, spectacle,
// wayshot, hyprshot) than on X11 (gnome-screenshot, xfce4-screenshooter,
// spectacle, scrot, maim, flameshot, ImageMagick).
//
// Save pipeline
// -------------
// Everything after capture runs through the `image` crate on a single owned
// `RgbaImage`, so the whole pipeline is `Send` and executes off the GTK main
// thread. Watermark and vector annotations are rendered by Pango/Cairo into
// small transparent layers that are composited onto that image, which keeps
// text shaping correct for every script while avoiding a full-surface Cairo
// round-trip per save.

use gtk4::prelude::*;
use gtk4::{glib, gio};
use libadwaita as adw;
use libadwaita::prelude::*;
use ashpd::desktop::screenshot::Screenshot;
use chrono::{DateTime, Local};
use gettextrs::gettext;
use std::path::{Path, PathBuf};

use crate::annotate::Annotation;
use crate::config;

// ---------------------------------------------------------------------------
// Display backend detection
// ---------------------------------------------------------------------------

/// The windowing backend GDK actually connected to.
///
/// This is deliberately *not* derived from `WAYLAND_DISPLAY` alone: running
/// with `GDK_BACKEND=x11` inside a Wayland session gives a process that talks
/// X11 through XWayland while `WAYLAND_DISPLAY` is still set. Choosing the
/// fallback tool list from the environment instead of from GDK would offer
/// Wayland-only helpers to a client that cannot use them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayBackend {
    Wayland,
    X11,
    Unknown,
}

/// Determine the active backend from the live GDK display, falling back to
/// environment inspection when no display is open yet (headless startup).
pub fn display_backend() -> DisplayBackend {
    if let Some(display) = gtk4::gdk::Display::default() {
        // The backend is not exposed as an enum by gdk4-rs without pulling in
        // the per-backend crates, but the GType name is stable and cheap.
        let type_name = display.type_().name().to_string();
        if type_name.contains("Wayland") {
            return DisplayBackend::Wayland;
        }
        if type_name.contains("X11") {
            return DisplayBackend::X11;
        }
    }
    backend_from_env()
}

fn backend_from_env() -> DisplayBackend {
    backend_from_values(
        &std::env::var("GDK_BACKEND").unwrap_or_default(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
        &std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
    )
}

/// The environment-inspection rules, with the environment passed in.
///
/// Kept separate from the lookups so it can be exercised directly: mutating
/// process environment variables from a test is racy across the test harness's
/// threads, which is the same class of unsoundness that the `setlocale`
/// advisory covers.
fn backend_from_values(
    gdk_backend: &str,
    wayland_display: Option<&str>,
    x11_display: Option<&str>,
    session_type: &str,
) -> DisplayBackend {
    // An explicit GDK_BACKEND request wins: it is what GDK itself honours.
    let forced = gdk_backend.to_ascii_lowercase();
    if forced.contains("x11") {
        return DisplayBackend::X11;
    }
    if forced.contains("wayland") {
        return DisplayBackend::Wayland;
    }

    if wayland_display.is_some() || session_type == "wayland" {
        return DisplayBackend::Wayland;
    }
    if x11_display.is_some() || session_type == "x11" {
        return DisplayBackend::X11;
    }
    DisplayBackend::Unknown
}

// ---------------------------------------------------------------------------
// Capture modes
// ---------------------------------------------------------------------------

/// What the user asked to capture.
///
/// `Interactive` defers the choice to the portal's own selection UI, which is
/// what GNOME and KDE present. The explicit modes exist because the CLI
/// fallbacks have no such chooser: `grim` captures a whole output, `slurp`
/// selects a region, and each tool needs a different flag per mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CaptureMode {
    /// Let the capture backend present its own area/window/screen chooser.
    #[default]
    Interactive,
    /// Drag-select a rectangular region.
    Area,
    /// Pick a single window.
    Window,
    /// The entire screen.
    Screen,
}

impl CaptureMode {
    /// Map the combo-row index used by the Shot tab.
    pub fn from_index(idx: u32) -> Self {
        match idx {
            1 => CaptureMode::Area,
            2 => CaptureMode::Window,
            3 => CaptureMode::Screen,
            _ => CaptureMode::Interactive,
        }
    }

    fn label(self) -> String {
        match self {
            CaptureMode::Interactive => gettext("Interactive"),
            CaptureMode::Area => gettext("Area"),
            CaptureMode::Window => gettext("Window"),
            CaptureMode::Screen => gettext("Screen"),
        }
    }
}

// ---------------------------------------------------------------------------
// Private temporary files
// ---------------------------------------------------------------------------

/// A private directory holding one capture's temporary file.
///
/// The previous implementation wrote to a predictable `/tmp/supershot_<pid>.png`.
/// On a shared machine any other user can pre-create that path as a symlink and
/// redirect the write. This type instead creates a fresh directory with mode
/// 0700 using `create_dir`, which fails rather than follows if the name already
/// exists, and prefers `XDG_RUNTIME_DIR` (already user-private) when available.
/// The directory and its contents are removed on drop.
pub struct TempCapture {
    dir: PathBuf,
    file: PathBuf,
}

impl TempCapture {
    fn new() -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;

        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);

        // A handful of attempts is plenty: collisions require another process
        // to have claimed the exact same pid+nanosecond pair.
        for attempt in 0..8u32 {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(attempt);
            let dir = base.join(format!("supershot-{}-{}-{}", std::process::id(), nonce, attempt));

            match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => {
                    let file = dir.join("capture.png");
                    return Ok(Self { dir, file });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(format!(
                        "cannot create a private temporary directory in {}: {}",
                        base.display(),
                        e
                    ))
                }
            }
        }
        Err("cannot create a private temporary directory".to_string())
    }

    fn path_str(&self) -> String {
        self.file.to_string_lossy().to_string()
    }

    /// The `file://` URI of the temporary capture, correctly percent-encoded.
    fn uri(&self) -> String {
        gio::File::for_path(&self.file).uri().to_string()
    }
}

impl Drop for TempCapture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ---------------------------------------------------------------------------
// CLI fallback tools
// ---------------------------------------------------------------------------

/// Where a fallback tool writes its output.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sink {
    /// The tool writes to the path given in its arguments.
    File,
    /// The tool writes PNG bytes to stdout.
    Stdout,
}

/// A CLI screenshot helper together with the modes it can serve.
struct FallbackTool {
    /// Executable name, looked up on PATH.
    name: &'static str,
    /// Backends this tool can capture from.
    backends: &'static [DisplayBackend],
    /// Additional executables that must also be present (e.g. `slurp` for area
    /// selection with `grim`).
    companions: &'static [&'static str],
    sink: Sink,
    /// Build the argument vector for a mode, or `None` if unsupported.
    args: fn(CaptureMode, &str) -> Option<Vec<String>>,
}

fn v(parts: &[&str]) -> Option<Vec<String>> {
    Some(parts.iter().map(|s| s.to_string()).collect())
}

/// Ordered fallback table.
///
/// Order matters: the desktop-native tool for each environment comes before the
/// generic ones, so a KDE user gets Spectacle's familiar UI rather than scrot's.
const FALLBACK_TOOLS: &[FallbackTool] = &[
    // --- Wayland ---------------------------------------------------------
    // grim captures; slurp provides the interactive region selector. Area mode
    // is executed as a two-step pipeline by `run_grim_area`.
    FallbackTool {
        name: "grim",
        backends: &[DisplayBackend::Wayland],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| match mode {
            // Area is special-cased before this table is consulted.
            CaptureMode::Screen | CaptureMode::Interactive => v(&[out]),
            _ => None,
        },
    },
    FallbackTool {
        name: "hyprshot",
        backends: &[DisplayBackend::Wayland],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| {
            let m = match mode {
                CaptureMode::Area | CaptureMode::Interactive => "region",
                CaptureMode::Window => "window",
                CaptureMode::Screen => "output",
            };
            v(&["-m", m, "--raw", "-o", "-", "-f", out])
        },
    },
    FallbackTool {
        name: "wayshot",
        backends: &[DisplayBackend::Wayland],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| match mode {
            CaptureMode::Screen | CaptureMode::Interactive => v(&["-f", out]),
            _ => None,
        },
    },
    // --- Both backends ----------------------------------------------------
    // Spectacle is KDE's native tool and speaks both protocols.
    FallbackTool {
        name: "spectacle",
        backends: &[DisplayBackend::Wayland, DisplayBackend::X11],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| {
            let flag = match mode {
                CaptureMode::Area => "-r",
                CaptureMode::Window => "-a",
                CaptureMode::Screen => "-f",
                CaptureMode::Interactive => "-r",
            };
            v(&[flag, "-b", "-n", "-o", out])
        },
    },
    // gnome-screenshot works on X11 and, under a GNOME session, on Wayland too
    // because it drives gnome-shell over D-Bus rather than the display server.
    FallbackTool {
        name: "gnome-screenshot",
        backends: &[DisplayBackend::Wayland, DisplayBackend::X11],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| match mode {
            CaptureMode::Area | CaptureMode::Interactive => v(&["-a", "-f", out]),
            CaptureMode::Window => v(&["-w", "-f", out]),
            CaptureMode::Screen => v(&["-f", out]),
        },
    },
    FallbackTool {
        name: "flameshot",
        backends: &[DisplayBackend::Wayland, DisplayBackend::X11],
        companions: &[],
        sink: Sink::Stdout,
        args: |mode, _out| match mode {
            CaptureMode::Area | CaptureMode::Interactive => v(&["gui", "--raw"]),
            CaptureMode::Screen => v(&["full", "--raw"]),
            CaptureMode::Window => None,
        },
    },
    // --- X11 only ---------------------------------------------------------
    FallbackTool {
        name: "xfce4-screenshooter",
        backends: &[DisplayBackend::X11],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| {
            let flag = match mode {
                CaptureMode::Area | CaptureMode::Interactive => "-r",
                CaptureMode::Window => "-w",
                CaptureMode::Screen => "-f",
            };
            v(&[flag, "-s", out])
        },
    },
    FallbackTool {
        name: "scrot",
        backends: &[DisplayBackend::X11],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| match mode {
            CaptureMode::Area | CaptureMode::Interactive => v(&["-s", out]),
            CaptureMode::Window => v(&["-u", out]),
            CaptureMode::Screen => v(&[out]),
        },
    },
    FallbackTool {
        name: "maim",
        backends: &[DisplayBackend::X11],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| match mode {
            CaptureMode::Area | CaptureMode::Interactive => v(&["-s", out]),
            CaptureMode::Window => v(&["-st", "9999999", out]),
            CaptureMode::Screen => v(&[out]),
        },
    },
    // ImageMagick's `import` is the universal last resort on X11.
    FallbackTool {
        name: "import",
        backends: &[DisplayBackend::X11],
        companions: &[],
        sink: Sink::File,
        args: |mode, out| match mode {
            CaptureMode::Screen => v(&["-window", "root", out]),
            _ => v(&[out]),
        },
    },
];

/// Look up an executable on PATH without spawning it.
fn has_binary(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        // is_file() follows symlinks, which is what we want for /usr/bin entries
        // that point into /usr/lib or a Nix/Guix store.
        candidate.is_file()
    })
}

/// `grim` has no region selector of its own; `slurp` supplies one and prints a
/// geometry string that grim consumes via `-g`.
fn run_grim_area(out: &str) -> Result<(), String> {
    let selection = std::process::Command::new("slurp")
        .output()
        .map_err(|e| format!("slurp failed to start: {}", e))?;

    if !selection.status.success() {
        // A non-zero exit is how slurp reports that the user pressed Escape.
        return Err(CANCELLED.to_string());
    }
    let geometry = String::from_utf8_lossy(&selection.stdout).trim().to_string();
    if geometry.is_empty() {
        return Err(CANCELLED.to_string());
    }

    let status = std::process::Command::new("grim")
        .args(["-g", &geometry, out])
        .status()
        .map_err(|e| format!("grim failed to start: {}", e))?;

    if status.success() && Path::new(out).exists() {
        Ok(())
    } else {
        Err("grim did not produce an image".to_string())
    }
}

/// Names of the CLI fallback tools usable on the current backend.
///
/// Surfaced in the About dialog's troubleshooting section: on a desktop where
/// the portal is missing, whether any fallback exists is the single most
/// useful fact in a bug report.
pub fn available_fallback_tools() -> Vec<String> {
    let backend = display_backend();
    let mut found = Vec::new();

    if backend == DisplayBackend::Wayland && has_binary("grim") && has_binary("slurp") {
        found.push("grim+slurp".to_string());
    }

    for tool in FALLBACK_TOOLS {
        if !tool.backends.contains(&backend) {
            continue;
        }
        if found.iter().any(|f| f.starts_with(tool.name)) {
            continue;
        }
        if has_binary(tool.name) && tool.companions.iter().all(|c| has_binary(c)) {
            found.push(tool.name.to_string());
        }
    }

    found
}

/// Whether an XDG Desktop Portal implementation is currently on the session bus.
///
/// Checked by asking the bus for the name's owner rather than by issuing a
/// screenshot request, so calling this has no user-visible effect.
pub fn portal_available() -> bool {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };

    let reply = connection.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&("org.freedesktop.portal.Desktop",).to_variant()),
        Some(glib::VariantTy::new("(b)").expect("literal signature is valid")),
        gio::DBusCallFlags::NONE,
        1000,
        gio::Cancellable::NONE,
    );

    reply
        .ok()
        .and_then(|v| v.child_value(0).get::<bool>())
        .unwrap_or(false)
}

/// A copyable summary of everything that determines how SuperShot behaves on
/// this machine.
///
/// Shown in the About dialog's troubleshooting section and printed by
/// `supershot --doctor`. For a tool that has to work across every Linux
/// desktop, this is the difference between a reproducible bug report and a
/// guess about which portal backend the reporter had installed.
pub fn session_report() -> String {
    let env = |key: &str| std::env::var(key).unwrap_or_else(|_| "(unset)".to_string());

    let backend = match display_backend() {
        DisplayBackend::Wayland => "Wayland",
        DisplayBackend::X11 => "X11",
        DisplayBackend::Unknown => "unknown",
    };

    let tools = available_fallback_tools();
    let tool_list = if tools.is_empty() {
        "(none)".to_string()
    } else {
        tools.join(", ")
    };

    format!(
        "SuperShot {version}\n\
         Packaging: {channel}\n\
         GTK: {gtk_major}.{gtk_minor}.{gtk_micro}\n\
         Libadwaita: {adw_major}.{adw_minor}.{adw_micro}\n\
         \n\
         Display server: {backend}\n\
         XDG_SESSION_TYPE: {session_type}\n\
         XDG_CURRENT_DESKTOP: {desktop}\n\
         GDK_BACKEND: {gdk_backend}\n\
         \n\
         Locale: {lang}\n\
         Catalogs: {catalogs}\n\
         \n\
         Screenshot portal: {portal}\n\
         Fallback tools found: {tool_list}\n",
        version = config::VERSION,
        channel = config::channel(),
        gtk_major = gtk4::major_version(),
        gtk_minor = gtk4::minor_version(),
        gtk_micro = gtk4::micro_version(),
        adw_major = adw::major_version(),
        adw_minor = adw::minor_version(),
        adw_micro = adw::micro_version(),
        backend = backend,
        session_type = env("XDG_SESSION_TYPE"),
        desktop = env("XDG_CURRENT_DESKTOP"),
        gdk_backend = env("GDK_BACKEND"),
        lang = env("LANG"),
        catalogs = config::localedir().display(),
        portal = if portal_available() { "available" } else { "not detected" },
        tool_list = tool_list,
    )
}

/// Sentinel error meaning "the user cancelled", which must not be reported as a
/// failure or trigger another fallback attempt.
const CANCELLED: &str = "__supershot_cancelled__";

/// Attempt a capture with the first suitable CLI tool available on the session.
///
/// Returns the path that was written, or an error. `CANCELLED` is returned when
/// the user dismissed a tool's selection UI.
fn cli_screenshot_sync(
    out_path: &str,
    backend: DisplayBackend,
    mode: CaptureMode,
) -> Result<(), String> {
    // grim + slurp is the canonical area-capture pair on wlroots compositors,
    // where the Screenshot portal is frequently absent or non-interactive.
    if backend == DisplayBackend::Wayland
        && matches!(mode, CaptureMode::Area | CaptureMode::Interactive)
        && has_binary("grim")
        && has_binary("slurp")
    {
        match run_grim_area(out_path) {
            Ok(()) => return Ok(()),
            Err(e) if e == CANCELLED => return Err(e),
            Err(_) => { /* fall through to the generic table */ }
        }
    }

    let mut attempted = false;

    for tool in FALLBACK_TOOLS {
        if !tool.backends.contains(&backend) {
            continue;
        }
        let Some(args) = (tool.args)(mode, out_path) else {
            continue;
        };
        if !has_binary(tool.name) || !tool.companions.iter().all(|c| has_binary(c)) {
            continue;
        }

        attempted = true;

        match tool.sink {
            Sink::Stdout => {
                if let Ok(output) = std::process::Command::new(tool.name).args(&args).output() {
                    if output.status.success()
                        && !output.stdout.is_empty()
                        && std::fs::write(out_path, &output.stdout).is_ok()
                    {
                        return Ok(());
                    }
                }
            }
            Sink::File => {
                if let Ok(status) = std::process::Command::new(tool.name).args(&args).status() {
                    if status.success() && Path::new(out_path).exists() {
                        return Ok(());
                    }
                }
            }
        }
    }

    Err(no_tool_message(backend, mode, attempted))
}

/// Build an actionable, distribution-appropriate error message.
///
/// The old message hard-coded `sudo apt install gnome-screenshot`, which is
/// wrong advice on Fedora, Arch, openSUSE and every immutable distribution, and
/// is wrong advice *everywhere* inside Flatpak or Snap where host binaries are
/// unreachable no matter what the user installs.
fn no_tool_message(backend: DisplayBackend, mode: CaptureMode, attempted: bool) -> String {
    if config::is_sandboxed() {
        let channel = if config::is_flatpak() { "Flatpak" } else { "Snap" };
        return format!(
            "{}\n\n{}",
            gettext("The desktop screenshot portal is not responding."),
            gettext(
                "SuperShot is running inside a __CHANNEL__ sandbox, so it cannot use \
                 command-line screenshot tools installed on the host. Install an XDG desktop \
                 portal backend for your desktop environment (xdg-desktop-portal-gnome, \
                 xdg-desktop-portal-kde, xdg-desktop-portal-wlr or xdg-desktop-portal-gtk) \
                 and restart your session."
            )
            .replace("__CHANNEL__", channel)
        );
    }

    let suggestions: Vec<&str> = match backend {
        DisplayBackend::Wayland => vec!["grim + slurp", "spectacle", "gnome-screenshot"],
        DisplayBackend::X11 => vec!["gnome-screenshot", "spectacle", "scrot", "maim"],
        DisplayBackend::Unknown => vec!["grim + slurp", "gnome-screenshot", "scrot"],
    };

    let lead = if attempted {
        gettext("Every available screenshot tool failed to produce an image.")
    } else {
        gettext("No screenshot tool is available for this session.")
    };

    let mut msg = format!(
        "{}\n\n{}",
        lead,
        gettext("Mode: __MODE__ · Display server: __BACKEND__")
            .replace("__MODE__", &mode.label())
            .replace(
                "__BACKEND__",
                match backend {
                    DisplayBackend::Wayland => "Wayland",
                    DisplayBackend::X11 => "X11",
                    DisplayBackend::Unknown => "unknown",
                }
            )
    );

    msg.push_str("\n\n");
    msg.push_str(&gettext("Install a desktop portal backend, or one of: __TOOLS__")
        .replace("__TOOLS__", &suggestions.join(", ")));

    if let Some(cmd) = install_hint(&suggestions) {
        msg.push_str("\n\n");
        msg.push_str(&cmd);
    }

    msg
}

/// Derive a package-manager command line from /etc/os-release.
///
/// Package *names* differ per distribution far more than command syntax does,
/// so only the invocation is suggested; the tool names above are upstream
/// binary names, which most distributions reuse verbatim.
fn install_hint(tools: &[&str]) -> Option<String> {
    let release = std::fs::read_to_string("/etc/os-release").ok()?;

    let field = |key: &str| -> Option<String> {
        release.lines().find_map(|line| {
            let rest = line.strip_prefix(key)?.strip_prefix('=')?;
            Some(rest.trim_matches(['"', '\'']).to_ascii_lowercase())
        })
    };

    let ids = format!(
        "{} {}",
        field("ID").unwrap_or_default(),
        field("ID_LIKE").unwrap_or_default()
    );

    // First tool only: suggesting a single concrete command is more useful than
    // a menu, and the ordering above already puts the best fit first.
    let pkg = tools.first()?.split(" + ").next()?;

    let cmd = if ids.contains("arch") {
        format!("sudo pacman -S {}", pkg)
    } else if ids.contains("fedora") || ids.contains("rhel") || ids.contains("centos") {
        format!("sudo dnf install {}", pkg)
    } else if ids.contains("suse") {
        format!("sudo zypper install {}", pkg)
    } else if ids.contains("alpine") {
        format!("sudo apk add {}", pkg)
    } else if ids.contains("debian") || ids.contains("ubuntu") {
        format!("sudo apt install {}", pkg)
    } else if ids.contains("nixos") {
        format!("nix-env -iA nixpkgs.{}", pkg)
    } else {
        return None;
    };

    Some(gettext("For example: __CMD__").replace("__CMD__", &cmd))
}

// ---------------------------------------------------------------------------
// Capture options
// ---------------------------------------------------------------------------

/// Aggregates user-configurable capture settings.
///
/// Constructed from widget state in `window.rs` and threaded through the
/// capture pipeline so each stage can consult active settings without
/// reaching back into window state.
#[derive(Clone, Debug, Default)]
pub struct CaptureOptions {
    /// Output format: 0 = PNG, 1 = JPEG.
    pub format_idx: u32,
    /// JPEG encoder quality, 1-100. Ignored for PNG output.
    pub jpeg_quality: u8,
    /// Whether to overlay a date/time watermark.
    pub watermark: bool,
    /// Watermark date format preset index (0-4).
    pub watermark_format: u32,
    /// Optional custom text prepended to the watermark date (e.g. brand name).
    pub watermark_text: String,
    /// Watermark corner: 0 = bottom-right, 1 = bottom-left, 2 = top-right, 3 = top-left.
    pub watermark_position: u32,
    /// Watermark text color: 0 = white (dark shadow), 1 = black (light shadow).
    pub watermark_color: u32,
    /// Custom save directory, or None for the default ~/Pictures/Screenshots/.
    pub save_dir: Option<PathBuf>,
    /// Whether to show the preview/annotation window before saving.
    pub show_preview: bool,
    /// What to capture.
    pub mode: CaptureMode,
    /// Instant the screenshot was taken.
    ///
    /// Captured once and reused for every watermark render, so the timestamp
    /// shown in the preview is byte-for-byte the timestamp written to the file.
    /// Previously each render called `Local::now()` independently, which made
    /// the `HH:MM:SS` preset display one time and save another.
    pub captured_at: Option<DateTime<Local>>,
}

impl CaptureOptions {
    /// Effective JPEG quality, guarding against an unset or out-of-range value.
    pub fn quality(&self) -> u8 {
        if self.jpeg_quality == 0 {
            90
        } else {
            self.jpeg_quality.clamp(1, 100)
        }
    }

    fn timestamp(&self) -> DateTime<Local> {
        self.captured_at.unwrap_or_else(Local::now)
    }
}

/// Date format presets for the watermark.
const WATERMARK_FORMATS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S",   // 0: 2026-02-16 14:30:25
    "%d/%m/%Y %H:%M",      // 1: 16/02/2026 14:30
    "%b %d, %Y %H:%M",     // 2: Feb 16, 2026 14:30
    "%Y-%m-%d",            // 3: 2026-02-16
    "%H:%M:%S",            // 4: 14:30:25
];

// ---------------------------------------------------------------------------
// GUI capture entry points
// ---------------------------------------------------------------------------

/// Entry point for GUI-mode capture.
///
/// Disables the capture button immediately to prevent concurrent requests,
/// then either starts a visual countdown or hides the window and invokes
/// the portal after a short compositor settling period.
pub fn start_capture(
    window: &crate::window::SuperShotWindow,
    delay_seconds: u32,
    options: CaptureOptions,
) {
    let window = window.clone();

    // Guard against concurrent captures for all delay values.
    window.set_capture_sensitive(false);

    // Prevent GTK from auto-quitting when the window is hidden.
    // GtkApplication terminates when no visible windows remain. The RAII
    // ApplicationHoldGuard keeps the app alive; it is moved through the closure
    // chain and automatically calls g_application_release() when dropped at
    // the end of the capture flow.
    let hold = gio::Application::default().map(|app| app.hold());

    if delay_seconds > 0 {
        start_countdown(window, delay_seconds, hold, options);
    } else {
        gtk4::prelude::WidgetExt::set_visible(&window, false);
        glib::timeout_add_local_once(SETTLE_DELAY, move || {
            perform_screenshot(window, hold, options);
        });
    }
}

/// Time given to the compositor to finish unmapping our window before the
/// screenshot is taken, so SuperShot does not appear in its own capture.
const SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// Drive the visual countdown timer.
fn start_countdown(
    window: crate::window::SuperShotWindow,
    seconds: u32,
    hold: Option<gio::ApplicationHoldGuard>,
    options: CaptureOptions,
) {
    let remaining = std::rc::Rc::new(std::cell::Cell::new(seconds));
    // Wrap the hold guard in Rc<RefCell> so it can be moved out of the periodic closure.
    let hold = std::rc::Rc::new(std::cell::RefCell::new(hold));
    let options = std::rc::Rc::new(options);
    window.show_countdown(seconds);

    glib::timeout_add_seconds_local(1, move || {
        let r = remaining.get().saturating_sub(1);
        remaining.set(r);

        if r == 0 {
            window.hide_countdown();
            gtk4::prelude::WidgetExt::set_visible(&window, false);

            let window_clone = window.clone();
            let hold_inner = hold.borrow_mut().take();
            let opts = (*options).clone();
            glib::timeout_add_local_once(SETTLE_DELAY, move || {
                perform_screenshot(window_clone, hold_inner, opts);
            });
            glib::ControlFlow::Break
        } else {
            window.show_countdown(r);
            glib::ControlFlow::Continue
        }
    });
}

/// Restore the main window and re-arm the capture button.
fn restore_window(window: &crate::window::SuperShotWindow) {
    window.set_capture_sensitive(true);
    window.set_busy(false);
    gtk4::prelude::WidgetExt::set_visible(window, true);
    gtk4::prelude::GtkWindowExt::present(window);
}

/// Present a modal error dialog using AdwAlertDialog.
/// The dialog is non-blocking; the GLib main loop continues while it is shown.
pub(crate) fn show_error_dialog(window: &crate::window::SuperShotWindow, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("ok", &gettext("OK"));
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    dialog.present(Some(window));
}

/// Execute the screenshot request in GUI mode: portal first, CLI fallback after.
fn perform_screenshot(
    window: crate::window::SuperShotWindow,
    hold: Option<gio::ApplicationHoldGuard>,
    options: CaptureOptions,
) {
    glib::spawn_future_local(async move {
        let backend = display_backend();

        match request_portal_screenshot(options.mode).await {
            PortalOutcome::Captured(uri) => {
                handle_screenshot_result(&window, uri, None, options, hold);
                return;
            }
            PortalOutcome::Cancelled => {
                // Deliberate dismissal: no fallback, no error.
            }
            PortalOutcome::Unavailable => {
                // No portal implementation answered. Try the session's own tools
                // on every backend — wlroots compositors in particular ship
                // grim/slurp far more reliably than a Screenshot portal.
                match cli_capture(backend, options.mode).await {
                    Ok(Some((uri, temp))) => {
                        handle_screenshot_result(&window, uri, Some(temp), options, hold);
                        return;
                    }
                    Ok(None) => { /* cancelled in the CLI tool */ }
                    Err(e) => {
                        show_error_dialog(&window, &gettext("Screenshot Failed"), &e);
                    }
                }
            }
            PortalOutcome::Failed(e) => {
                show_error_dialog(
                    &window,
                    &gettext("Portal Error"),
                    &format!("{}\n\n{}", gettext("The screenshot portal returned an error."), e),
                );
            }
        }

        restore_window(&window);
        drop(hold);
    });
}

/// Outcome of a portal screenshot request.
enum PortalOutcome {
    Captured(String),
    /// The user dismissed the portal's selection UI.
    Cancelled,
    /// No portal implementation is reachable — fall back to CLI tools.
    Unavailable,
    /// A portal answered but reported a failure.
    Failed(String),
}

/// Issue the XDG Screenshot portal request.
///
/// Deliberately sent without a `WindowIdentifier`.
///
/// Parenting a portal request means exporting the calling window through the
/// `xdg_foreign` protocol, and that requires a surface with a live toplevel
/// role. SuperShot hides its window before every capture so it does not appear
/// in its own screenshot, which destroys the `xdg_toplevel` — asking the
/// compositor to export the surface afterwards is a protocol violation
/// ("exported surface had an invalid role") that terminates the process.
///
/// There is also nothing to gain from it here: with the window hidden there is
/// no parent for the portal dialog to sit above, and portals identify the
/// requesting application from the D-Bus connection's credentials rather than
/// from the window handle.
async fn request_portal_screenshot(mode: CaptureMode) -> PortalOutcome {
    let request = Screenshot::request();

    // The portal exposes a single interactive flag rather than discrete modes.
    // Interactive lets its own chooser handle area/window/screen; the explicit
    // Screen mode skips the chooser entirely, which is what a scripted
    // full-screen capture wants. Area and Window have no portal equivalent, so
    // they go through the chooser too and are honoured exactly by the CLI
    // fallbacks.
    let interactive = mode != CaptureMode::Screen;

    match request.interactive(interactive).send().await {
        Ok(response) => match response.response() {
            Ok(screenshot) => PortalOutcome::Captured(screenshot.uri().as_str().to_string()),
            Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => {
                PortalOutcome::Cancelled
            }
            Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Other)) => {
                // Backends disagree on which of these two they emit for a
                // dismissed dialog; treating both as cancellation avoids
                // showing an error box for a deliberate Escape keypress.
                PortalOutcome::Cancelled
            }
            Err(e) => PortalOutcome::Failed(e.to_string()),
        },
        // A transport-level error means no portal service answered on the bus.
        Err(_) => PortalOutcome::Unavailable,
    }
}

/// Run the CLI fallback on a worker thread.
///
/// Returns `Ok(None)` when the user cancelled. The `TempCapture` guard is handed
/// back to the caller so the private directory outlives the read of its file.
async fn cli_capture(
    backend: DisplayBackend,
    mode: CaptureMode,
) -> Result<Option<(String, TempCapture)>, String> {
    let temp = TempCapture::new()?;
    let out = temp.path_str();

    let result = tokio::task::spawn_blocking(move || cli_screenshot_sync(&out, backend, mode))
        .await
        .map_err(|e| format!("screenshot task failed: {}", e))?;

    match result {
        Ok(()) => {
            let uri = temp.uri();
            Ok(Some((uri, temp)))
        }
        Err(e) if e == CANCELLED => Ok(None),
        Err(e) => Err(e),
    }
}

/// Common handler for a successful capture (portal and CLI paths alike).
fn handle_screenshot_result(
    window: &crate::window::SuperShotWindow,
    uri_string: String,
    temp: Option<TempCapture>,
    mut options: CaptureOptions,
    hold: Option<gio::ApplicationHoldGuard>,
) {
    // Freeze the capture instant once, here, so preview and file agree.
    options.captured_at = Some(Local::now());

    if options.show_preview {
        crate::preview::PreviewWindow::present_for(
            uri_string,
            temp,
            options,
            window.clone(),
            hold,
        );
        return;
    }

    window.set_busy(true);
    let window = window.clone();

    save_async(
        SaveSource::File(uri_string),
        options,
        crate::editing::EditState::default(),
        Vec::new(),
        None,
        move |result| {
            match result {
                Ok(dest_path) => {
                    if let Err(e) = copy_to_clipboard(&window, &dest_path) {
                        eprintln!("Clipboard error: {}", e);
                    }
                    send_notification(&dest_path);
                }
                Err(e) => {
                    show_error_dialog(
                        &window,
                        &gettext("Save Error"),
                        &format!("{}\n\n{}", gettext("Failed to save the screenshot."), e),
                    );
                }
            }
            restore_window(&window);
            drop(hold);
            drop(temp);
        },
    );
}

// ---------------------------------------------------------------------------
// Headless (CLI) capture
// ---------------------------------------------------------------------------

/// Entry point for headless capture, invoked by `--now`.
pub fn start_headless(delay_seconds: u32, options: CaptureOptions) {
    // Headless mode has no visible windows; the hold guard prevents auto-quit.
    let hold = gio::Application::default().map(|app| app.hold());

    if delay_seconds > 0 {
        eprintln!("{}", gettext("Waiting __N__ seconds…").replace("__N__", &delay_seconds.to_string()));
        glib::timeout_add_seconds_local_once(delay_seconds, move || {
            let _h = hold;
            perform_headless_screenshot(options);
        });
    } else {
        let _h = hold;
        perform_headless_screenshot(options);
    }
}

fn perform_headless_screenshot(mut options: CaptureOptions) {
    glib::spawn_future_local(async move {
        let backend = display_backend();

        let captured = match request_portal_screenshot(options.mode).await {
            PortalOutcome::Captured(uri) => Some((uri, None)),
            PortalOutcome::Cancelled => None,
            PortalOutcome::Unavailable => match cli_capture(backend, options.mode).await {
                Ok(Some((uri, temp))) => Some((uri, Some(temp))),
                Ok(None) => None,
                Err(e) => {
                    eprintln!("supershot: {}", e);
                    None
                }
            },
            PortalOutcome::Failed(e) => {
                eprintln!("supershot: portal error: {}", e);
                None
            }
        };

        if let Some((uri, temp)) = captured {
            options.captured_at = Some(Local::now());
            let opts = options.clone();
            let result = tokio::task::spawn_blocking(move || {
                render_and_save(
                    SaveSource::File(uri),
                    &opts,
                    &crate::editing::EditState::default(),
                    &[],
                    None,
                )
            })
            .await;

            match result {
                Ok(Ok(dest_path)) => {
                    // Parity with GUI mode: a scripted capture is far more
                    // useful when the image also lands on the clipboard and the
                    // user is told about it. The previous headless path did
                    // neither, contradicting the documented behaviour.
                    if let Err(e) = copy_path_to_clipboard(&dest_path) {
                        eprintln!("supershot: clipboard unavailable: {}", e);
                    }
                    send_notification(&dest_path);
                    println!("{}", dest_path.display());
                }
                Ok(Err(e)) => eprintln!("supershot: {}", e),
                Err(e) => eprintln!("supershot: save task failed: {}", e),
            }
            drop(temp);
        }

        if let Some(app) = gio::Application::default() {
            app.quit();
        }
    });
}

// ---------------------------------------------------------------------------
// Notifications and clipboard
// ---------------------------------------------------------------------------

/// Send a desktop notification indicating a successful capture.
pub(crate) fn send_notification(dest_path: &Path) {
    let Some(app) = gio::Application::default() else {
        return;
    };
    let path_str = dest_path.to_string_lossy().to_string();

    let notification = gio::Notification::new(&gettext("Screenshot captured"));
    notification.set_body(Some(
        &gettext("Saved to __PATH__").replace("__PATH__", &path_str),
    ));
    notification.set_default_action_and_target_value(
        "app.open-screenshot",
        Some(&path_str.to_variant()),
    );
    notification.add_button_with_target_value(
        &gettext("Show in Folder"),
        "app.open-folder",
        Some(&path_str.to_variant()),
    );

    let notif_id = format!("screenshot-{}", Local::now().format("%H%M%S%3f"));
    app.send_notification(Some(&notif_id), &notification);
}

/// Copy a saved screenshot to the system clipboard via a GDK texture.
pub(crate) fn copy_to_clipboard(
    window: &crate::window::SuperShotWindow,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let texture = gtk4::gdk::Texture::from_file(&gio::File::for_path(path))?;
    gtk4::prelude::WidgetExt::clipboard(window).set_texture(&texture);
    Ok(())
}

/// Clipboard copy without a window, for headless mode.
fn copy_path_to_clipboard(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let display = gtk4::gdk::Display::default().ok_or("no display connection")?;
    let texture = gtk4::gdk::Texture::from_file(&gio::File::for_path(path))?;
    display.clipboard().set_texture(&texture);
    Ok(())
}

// ---------------------------------------------------------------------------
// Save pipeline
// ---------------------------------------------------------------------------

/// Resolve, create and validate the directory screenshots are written to.
///
/// Falls back through the configured directory, the XDG pictures directory and
/// `$HOME/Pictures` in turn. `dirs::picture_dir()` returns `None` whenever
/// xdg-user-dirs is not configured — routine on minimal installs and bare
/// window managers — which used to abort the save and discard a capture the
/// user had already taken.
pub fn resolve_save_dir(options: &CaptureOptions) -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(dir) = &options.save_dir {
        candidates.push(dir.clone());
    }
    if let Some(pictures) = dirs::picture_dir() {
        candidates.push(pictures.join("Screenshots"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Pictures").join("Screenshots"));
    }
    // Last resort: never lose a capture the user already took.
    candidates.push(std::env::temp_dir().join("supershot"));

    let mut last_error = String::new();
    for dir in candidates {
        match std::fs::create_dir_all(&dir) {
            Ok(()) => {
                if is_writable(&dir) {
                    return Ok(dir);
                }
                last_error = format!("{} is not writable", dir.display());
            }
            Err(e) => last_error = format!("{}: {}", dir.display(), e),
        }
    }

    Err(gettext("No writable save directory could be found (__DETAIL__)")
        .replace("__DETAIL__", &last_error))
}

/// Probe write access by creating and removing a file, rather than inspecting
/// permission bits, so ACLs, read-only mounts and full filesystems are all
/// caught by the same check.
fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".supershot-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Build the destination path for a capture.
fn destination_path(options: &CaptureOptions) -> Result<PathBuf, String> {
    let save_dir = resolve_save_dir(options)?;
    let ext = if options.format_idx == 1 { "jpg" } else { "png" };
    let filename = format!(
        "Screenshot_{}_supershot.{}",
        options.timestamp().format("%Y-%m-%d_%H-%M-%S%.3f"),
        ext
    );
    Ok(save_dir.join(filename))
}

/// Where the save pipeline gets its pixels.
///
/// The preview window hands over the image it is actually showing, which is the
/// only way a baked-in crop and the adjustments layered on it can both survive
/// to disk. Captures saved without a preview stream straight from the file the
/// portal or CLI tool produced.
pub enum SaveSource {
    /// A capture file, identified by `file://` URI or path.
    File(String),
    /// An in-memory image, plus the original capture file to clean up.
    Image {
        image: image::RgbaImage,
        original: Option<PathBuf>,
    },
}

impl SaveSource {
    /// Path of the file this capture came from, if any, for duplicate cleanup.
    fn origin(&self) -> Option<PathBuf> {
        match self {
            SaveSource::File(uri) => crate::editing::source_path(uri),
            SaveSource::Image { original, .. } => original.clone(),
        }
    }
}

/// Render the finished image: edits, crop, annotations, watermark.
///
/// Order is significant. Geometry and tone come first; then the crop the user
/// drew against that corrected image; then redactions, which have to overwrite
/// real pixels; then vector annotations; and the watermark last, so it is never
/// obscured by a mark placed over it.
pub fn render_image(
    source: SaveSource,
    options: &CaptureOptions,
    edits: &crate::editing::EditState,
    annotations: &[Annotation],
    crop: Option<(i32, i32, i32, i32)>,
) -> Result<image::RgbaImage, String> {
    let base = match source {
        SaveSource::File(ref uri) => crate::editing::load(uri)
            .map_err(|e| format!("{}: {}", gettext("Cannot read the capture"), e))?,
        SaveSource::Image { image, .. } => image,
    };

    let mut img = crate::editing::apply_edits_rgba(&base, edits);

    // Annotations are stored against the edited, uncropped frame. A crop that
    // has not been baked into the source yet moves the origin, so the marks
    // have to move with it or they land in the wrong place in the saved file.
    let mut annotations = annotations.to_vec();

    if let Some((x, y, w, h)) = crop {
        img = crate::editing::crop_rgba(img, x, y, w, h);
        for ann in &mut annotations {
            crate::annotate::translate(ann, -(x as f64), -(y as f64));
        }
    }

    crate::annotate::render_onto(&mut img, &annotations);

    if options.watermark {
        apply_watermark(&mut img, options);
    }

    Ok(img)
}

/// The complete render-and-save pipeline. Runs entirely off the main thread.
pub fn render_and_save(
    source: SaveSource,
    options: &CaptureOptions,
    edits: &crate::editing::EditState,
    annotations: &[Annotation],
    crop: Option<(i32, i32, i32, i32)>,
) -> Result<PathBuf, String> {
    let origin = source.origin();
    let img = render_image(source, options, edits, annotations, crop)?;

    let dest_path = destination_path(options)?;
    encode(&img, &dest_path, options)?;

    // Remove the portal's own copy to prevent a duplicate appearing in the
    // system screenshot folder. Guard against deleting what we just wrote.
    if let Some(origin) = origin {
        if origin != dest_path {
            let _ = std::fs::remove_file(&origin);
        }
    }

    Ok(dest_path)
}

/// Encode the finished image in the configured format.
fn encode(
    img: &image::RgbaImage,
    dest_path: &Path,
    options: &CaptureOptions,
) -> Result<(), String> {
    let file = std::fs::File::create(dest_path)
        .map_err(|e| format!("{}: {}", dest_path.display(), e))?;
    let mut writer = std::io::BufWriter::new(file);

    let result = if options.format_idx == 1 {
        // JPEG has no alpha channel; flattening onto white matches what every
        // other screenshot tool produces for a transparent capture.
        let rgb = flatten_to_rgb(img);
        let encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, options.quality());
        rgb.write_with_encoder(encoder)
    } else {
        let encoder = image::codecs::png::PngEncoder::new(&mut writer);
        img.write_with_encoder(encoder)
    };

    result.map_err(|e| format!("{}: {}", gettext("Encoding failed"), e))?;

    // Surface a full disk here rather than leaving a truncated file behind.
    use std::io::Write;
    writer
        .flush()
        .map_err(|e| format!("{}: {}", gettext("Encoding failed"), e))?;

    Ok(())
}

/// Composite RGBA over an opaque white background.
fn flatten_to_rgb(img: &image::RgbaImage) -> image::RgbImage {
    let mut out = image::RgbImage::new(img.width(), img.height());
    for (x, y, px) in img.enumerate_pixels() {
        let a = px[3] as u32;
        let blend = |c: u8| -> u8 { ((c as u32 * a + 255 * (255 - a)) / 255) as u8 };
        out.put_pixel(x, y, image::Rgb([blend(px[0]), blend(px[1]), blend(px[2])]));
    }
    out
}

/// Run the save pipeline on a worker thread and deliver the result on the main
/// thread.
///
/// Encoding a 4K screenshot — especially one carrying a blur adjustment — takes
/// long enough to freeze the UI noticeably if done inline, which is what the
/// previous synchronous implementation did immediately after destroying the
/// preview window, leaving the user with no window and no feedback.
pub fn save_async<F>(
    source: SaveSource,
    options: CaptureOptions,
    edits: crate::editing::EditState,
    annotations: Vec<Annotation>,
    crop: Option<(i32, i32, i32, i32)>,
    on_done: F,
) where
    F: FnOnce(Result<PathBuf, String>) + 'static,
{
    glib::spawn_future_local(async move {
        let result = tokio::task::spawn_blocking(move || {
            render_and_save(source, &options, &edits, &annotations, crop)
        })
        .await;

        on_done(match result {
            Ok(inner) => inner,
            Err(e) => Err(format!("{}: {}", gettext("Save task failed"), e)),
        });
    });
}

/// Render without writing to disk, for clipboard-only export.
pub fn render_async<F>(
    source: SaveSource,
    options: CaptureOptions,
    edits: crate::editing::EditState,
    annotations: Vec<Annotation>,
    crop: Option<(i32, i32, i32, i32)>,
    on_done: F,
) where
    F: FnOnce(Result<image::RgbaImage, String>) + 'static,
{
    glib::spawn_future_local(async move {
        let result = tokio::task::spawn_blocking(move || {
            render_image(source, &options, &edits, &annotations, crop)
        })
        .await;

        on_done(match result {
            Ok(inner) => inner,
            Err(e) => Err(format!("{}: {}", gettext("Render task failed"), e)),
        });
    });
}

/// Put an in-memory image on the clipboard without a file round-trip.
pub fn copy_image_to_clipboard(
    widget: &gtk4::Widget,
    img: &image::RgbaImage,
) -> Result<(), Box<dyn std::error::Error>> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    if w == 0 || h == 0 {
        return Err("empty image".into());
    }
    let bytes = glib::Bytes::from(img.as_raw().as_slice());
    let texture = gtk4::gdk::MemoryTexture::new(
        w,
        h,
        gtk4::gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        (w as usize) * 4,
    );
    gtk4::prelude::WidgetExt::clipboard(widget).set_texture(&texture);
    Ok(())
}

// ---------------------------------------------------------------------------
// Watermark
// ---------------------------------------------------------------------------

/// Build the watermark string for the given options.
pub fn watermark_text(options: &CaptureOptions) -> String {
    let fmt = WATERMARK_FORMATS
        .get(options.watermark_format as usize)
        .unwrap_or(&WATERMARK_FORMATS[0]);
    let date_str = options.timestamp().format(fmt).to_string();

    if options.watermark_text.is_empty() {
        date_str
    } else {
        format!("{} | {}", options.watermark_text, date_str)
    }
}

/// Font size for a given image height, matching preview and save exactly.
fn watermark_font_size(height: f64) -> f64 {
    (height * 0.02).clamp(14.0, 36.0)
}

/// Watermark colours as (shadow_luminance, shadow_alpha, text_luminance, text_alpha).
fn watermark_colors(options: &CaptureOptions) -> (f64, f64, f64, f64) {
    if options.watermark_color == 1 {
        (1.0, 0.45, 0.0, 0.75) // black text, light shadow
    } else {
        (0.0, 0.55, 1.0, 0.85) // white text, dark shadow
    }
}

/// Draw the watermark onto a Cairo context whose user space is image pixels.
///
/// Shared by the preview canvas and the save pipeline's offscreen layer, so
/// what the user sees is what gets written.
///
/// Text is laid out by Pango rather than Cairo's "toy" API. The toy API does no
/// shaping and no font fallback, so a watermark containing Japanese, Chinese,
/// Korean, Hindi or Arabic text rendered as empty boxes — on an application
/// that ships in fourteen languages and invites the user to type their own
/// brand name into it.
pub fn draw_watermark_overlay(
    ctx: &cairo::Context,
    width: f64,
    height: f64,
    options: &CaptureOptions,
) {
    let text = watermark_text(options);
    if text.is_empty() {
        return;
    }

    let layout = pangocairo::functions::create_layout(ctx);
    let mut desc = pango::FontDescription::from_string("Sans");
    desc.set_absolute_size(watermark_font_size(height) * pango::SCALE as f64);
    layout.set_font_description(Some(&desc));
    layout.set_text(&text);

    let (tw, th) = layout.pixel_size();
    let (tw, th) = (tw as f64, th as f64);
    let margin = 10.0;

    // Pango positions from the top-left of the layout box, so every corner is a
    // straightforward inset — no baseline arithmetic, and correct for scripts
    // with tall ascenders or descenders.
    let (x, y) = match options.watermark_position {
        1 => (margin, height - th - margin),                  // bottom-left
        2 => (width - tw - margin, margin),                   // top-right
        3 => (margin, margin),                                // top-left
        _ => (width - tw - margin, height - th - margin),     // bottom-right
    };

    let (shadow_l, shadow_a, text_l, text_a) = watermark_colors(options);

    ctx.set_source_rgba(shadow_l, shadow_l, shadow_l, shadow_a);
    ctx.move_to(x + 1.0, y + 1.0);
    pangocairo::functions::show_layout(ctx, &layout);

    ctx.set_source_rgba(text_l, text_l, text_l, text_a);
    ctx.move_to(x, y);
    pangocairo::functions::show_layout(ctx, &layout);
}

/// Composite the watermark onto an owned image during the save pipeline.
fn apply_watermark(img: &mut image::RgbaImage, options: &CaptureOptions) {
    let (w, h) = (img.width(), img.height());
    let layer = crate::annotate::render_layer(w, h, |ctx| {
        draw_watermark_overlay(ctx, w as f64, h as f64, options);
    });

    if let Some(layer) = layer {
        image::imageops::overlay(img, &layer, 0, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::{Annotation, Shape};
    use crate::editing::EditState;

    fn image(w: u32, h: u32) -> image::RgbaImage {
        image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]))
    }

    fn source(img: image::RgbaImage) -> SaveSource {
        SaveSource::Image { image: img, original: None }
    }

    fn options() -> CaptureOptions {
        CaptureOptions {
            captured_at: Some(
                chrono::TimeZone::with_ymd_and_hms(&Local, 2026, 8, 26, 12, 0, 0)
                    .single()
                    .expect("a valid local timestamp"),
            ),
            ..CaptureOptions::default()
        }
    }

    /// The regression this release exists for.
    ///
    /// Saving used to re-read the original capture from disk and re-apply an
    /// edit state that "Apply Crop" had just reset, so the file on disk was the
    /// full uncropped image. Rendering from the image the preview owns is what
    /// makes the crop survive.
    #[test]
    fn a_baked_crop_survives_to_the_rendered_image() {
        // What the preview holds after "Apply Crop": an already-cropped image
        // and an empty edit state, with no pending crop rectangle.
        let cropped = image(200, 100);

        let out = render_image(
            source(cropped),
            &options(),
            &EditState::default(),
            &[],
            None,
        )
        .expect("render must succeed");

        assert_eq!((out.width(), out.height()), (200, 100));
    }

    #[test]
    fn a_pending_crop_is_applied_at_render_time() {
        let out = render_image(
            source(image(400, 300)),
            &options(),
            &EditState::default(),
            &[],
            Some((50, 40, 100, 80)),
        )
        .expect("render must succeed");

        assert_eq!((out.width(), out.height()), (100, 80));
    }

    /// Annotations are stored against the uncropped frame, so a crop applied at
    /// render time has to move them with it.
    #[test]
    fn annotations_follow_a_pending_crop() {
        let mut marked = Annotation {
            shape: Shape::Blackout { rect: (100.0, 100.0, 20.0, 20.0) },
            color: (0.0, 0.0, 0.0),
            stroke: 4.0,
        };
        marked.color = (1.0, 0.0, 0.0);

        let out = render_image(
            source(image(400, 300)),
            &options(),
            &EditState::default(),
            std::slice::from_ref(&marked),
            // Crop starting at (90, 90): the mark should land at (10, 10).
            Some((90, 90, 200, 150)),
        )
        .expect("render must succeed");

        let red = image::Rgba([255, 0, 0, 255]);
        assert_eq!(*out.get_pixel(15, 15), red, "the mark did not move with the crop");
        assert_ne!(*out.get_pixel(150, 120), red, "the mark was painted at its old position");
    }

    /// Geometry is applied before the crop, so a crop rectangle drawn on a
    /// rotated preview is interpreted in the rotated frame.
    #[test]
    fn rotation_is_applied_before_the_crop() {
        let edits = EditState { rotation: 90, ..EditState::default() };

        let out = render_image(
            source(image(400, 200)),
            &options(),
            &edits,
            &[],
            None,
        )
        .expect("render must succeed");

        // A quarter turn swaps the axes.
        assert_eq!((out.width(), out.height()), (200, 400));
    }

    #[test]
    fn a_crop_outside_the_image_does_not_produce_an_empty_render() {
        let out = render_image(
            source(image(100, 100)),
            &options(),
            &EditState::default(),
            &[],
            Some((500, 500, 50, 50)),
        )
        .expect("render must succeed");

        assert!(out.width() > 0 && out.height() > 0);
    }

    /// The preview and the saved file must show the same instant. The timestamp
    /// used to be read from the clock on each render, so the HH:MM:SS preset
    /// displayed one time and wrote another.
    #[test]
    fn the_watermark_timestamp_is_frozen_at_capture() {
        let opts = options();
        let first = watermark_text(&opts);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = watermark_text(&opts);
        assert_eq!(first, second);
        assert!(first.contains("2026-08-26"), "unexpected watermark text: {first}");
    }

    #[test]
    fn watermark_text_prefixes_the_custom_label() {
        let opts = CaptureOptions {
            watermark_text: "Acme".to_string(),
            watermark_format: 3, // date only
            ..options()
        };
        assert_eq!(watermark_text(&opts), "Acme | 2026-08-26");
    }

    #[test]
    fn an_out_of_range_watermark_format_falls_back_to_the_first_preset() {
        let opts = CaptureOptions { watermark_format: 99, ..options() };
        assert_eq!(watermark_text(&opts), "2026-08-26 12:00:00");
    }

    #[test]
    fn the_watermark_is_composited_onto_the_image() {
        let opts = CaptureOptions { watermark: true, ..options() };
        let plain = render_image(source(image(400, 200)), &options(), &EditState::default(), &[], None)
            .expect("render must succeed");
        let marked = render_image(source(image(400, 200)), &opts, &EditState::default(), &[], None)
            .expect("render must succeed");

        assert_ne!(plain.as_raw(), marked.as_raw(), "the watermark was not drawn");
    }

    #[test]
    fn jpeg_quality_is_clamped_to_a_valid_range() {
        assert_eq!(CaptureOptions { jpeg_quality: 0, ..options() }.quality(), 90);
        assert_eq!(CaptureOptions { jpeg_quality: 50, ..options() }.quality(), 50);
        assert_eq!(CaptureOptions { jpeg_quality: 200, ..options() }.quality(), 100);
    }

    #[test]
    fn capture_mode_maps_every_combo_index() {
        assert_eq!(CaptureMode::from_index(0), CaptureMode::Interactive);
        assert_eq!(CaptureMode::from_index(1), CaptureMode::Area);
        assert_eq!(CaptureMode::from_index(2), CaptureMode::Window);
        assert_eq!(CaptureMode::from_index(3), CaptureMode::Screen);
        // Out-of-range values from a corrupted GSettings key must not panic.
        assert_eq!(CaptureMode::from_index(99), CaptureMode::Interactive);
    }

    /// The temporary capture directory must be private and must disappear with
    /// its guard, so a discarded screenshot leaves nothing readable behind.
    #[test]
    fn temp_captures_are_private_and_cleaned_up() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempCapture::new().expect("a temporary directory must be creatable");
        let dir = temp.dir.clone();

        assert!(dir.is_dir());
        let mode = std::fs::metadata(&dir).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "temporary directory is not private");

        std::fs::write(temp.path_str(), b"x").expect("the capture path must be writable");

        drop(temp);
        assert!(!dir.exists(), "the temporary directory outlived its guard");
    }

    #[test]
    fn temp_capture_uris_are_percent_encoded() {
        let temp = TempCapture::new().expect("a temporary directory must be creatable");
        let uri = temp.uri();
        assert!(uri.starts_with("file://"), "unexpected URI: {uri}");
        assert!(!uri.contains(' '), "URI was not encoded: {uri}");
    }

    /// A save directory that cannot be written to must not lose the capture.
    #[test]
    fn the_save_directory_falls_back_when_the_configured_one_is_unusable() {
        let opts = CaptureOptions {
            // A path under /proc can never be created.
            save_dir: Some(PathBuf::from("/proc/supershot-cannot-exist")),
            ..options()
        };

        let dir = resolve_save_dir(&opts).expect("a fallback must always be found");
        assert!(dir.is_dir());
        assert!(is_writable(&dir));
    }

    #[test]
    fn the_configured_save_directory_wins_when_it_is_usable() {
        let base = TempCapture::new().expect("a temporary directory must be creatable");
        let target = base.dir.join("shots");

        let opts = CaptureOptions { save_dir: Some(target.clone()), ..options() };
        assert_eq!(resolve_save_dir(&opts).expect("resolvable"), target);
    }

    #[test]
    fn the_destination_filename_carries_the_capture_timestamp_and_extension() {
        let base = TempCapture::new().expect("a temporary directory must be creatable");

        for (format_idx, ext) in [(0u32, "png"), (1, "jpg")] {
            let opts = CaptureOptions {
                save_dir: Some(base.dir.clone()),
                format_idx,
                ..options()
            };
            let path = destination_path(&opts).expect("a destination must be derivable");
            let name = path.file_name().unwrap().to_string_lossy().to_string();

            assert!(name.starts_with("Screenshot_2026-08-26_12-00-00"), "unexpected name: {name}");
            assert!(name.ends_with(&format!("_supershot.{ext}")), "unexpected name: {name}");
        }
    }

    #[test]
    fn jpeg_output_is_flattened_over_white() {
        let transparent = image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 0]));
        let flat = flatten_to_rgb(&transparent);
        assert_eq!(*flat.get_pixel(0, 0), image::Rgb([255, 255, 255]));

        let opaque = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
        let flat = flatten_to_rgb(&opaque);
        assert_eq!(*flat.get_pixel(0, 0), image::Rgb([10, 20, 30]));
    }

    #[test]
    fn encoding_writes_a_readable_file_in_both_formats() {
        let temp = TempCapture::new().expect("a temporary directory must be creatable");

        for (format_idx, name) in [(0u32, "out.png"), (1, "out.jpg")] {
            let dest = temp.dir.join(name);
            let opts = CaptureOptions { format_idx, ..options() };
            encode(&image(32, 32), &dest, &opts).expect("encoding must succeed");

            let decoded = image::open(&dest).expect("the encoded file must be readable");
            assert_eq!((decoded.width(), decoded.height()), (32, 32));
        }
    }

    #[test]
    fn the_backend_is_taken_from_gdk_backend_when_the_environment_disagrees() {
        // Verified through the pure-environment path; the GDK path needs a
        // display connection and is exercised by `supershot --doctor`.
        assert_eq!(
            super::backend_from_values("x11", Some("wayland-0"), None, "wayland"),
            DisplayBackend::X11,
            "an explicit GDK_BACKEND request must win over WAYLAND_DISPLAY"
        );
        assert_eq!(
            super::backend_from_values("", Some("wayland-0"), None, "wayland"),
            DisplayBackend::Wayland
        );
        assert_eq!(
            super::backend_from_values("", None, Some(":0"), "x11"),
            DisplayBackend::X11
        );
        assert_eq!(
            super::backend_from_values("", None, None, ""),
            DisplayBackend::Unknown
        );
    }
}
