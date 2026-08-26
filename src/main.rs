// SuperShot - Screenshot tool for GNOME and other Linux desktops
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Application entry point. Parses CLI arguments and dispatches
// to either headless capture mode or the full GTK4 GUI.

mod annotate;
mod app;
mod capture;
mod config;
mod editing;
mod i18n;
mod preview;
mod window;

use app::SuperShotApp;
use capture::{CaptureMode, CaptureOptions};
use clap::{Parser, ValueEnum};
use gtk4::prelude::*;

/// What a headless capture should target.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, ValueEnum)]
enum CliMode {
    /// Let the capture backend present its own chooser.
    #[default]
    Interactive,
    /// Drag-select a region.
    Area,
    /// Pick a window.
    Window,
    /// Capture the whole screen without any prompt.
    Screen,
}

impl From<CliMode> for CaptureMode {
    fn from(m: CliMode) -> Self {
        match m {
            CliMode::Interactive => CaptureMode::Interactive,
            CliMode::Area => CaptureMode::Area,
            CliMode::Window => CaptureMode::Window,
            CliMode::Screen => CaptureMode::Screen,
        }
    }
}

/// Command-line interface definition.
///
/// Two execution modes:
/// - GUI (default): the main window with interactive controls.
/// - Headless (`--now`): capture and exit, for scripting and keybindings.
#[derive(Parser, Debug)]
#[command(
    name = "supershot",
    version,
    about = "GTK4 screenshot tool with annotation, watermark and delay timer",
    long_about = None
)]
struct Cli {
    /// Delay in seconds before capture
    #[arg(short, long, default_value_t = 0)]
    delay: u32,

    /// Take a screenshot immediately without showing the GUI
    #[arg(long)]
    now: bool,

    /// What to capture
    #[arg(short, long, value_enum, default_value_t = CliMode::Interactive)]
    mode: CliMode,

    /// Output format
    #[arg(short, long, value_parser = ["png", "jpeg", "jpg"], default_value = "png")]
    format: String,

    /// JPEG quality, 1-100
    #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: u8,

    /// Directory to save into (defaults to the configured location)
    #[arg(short, long)]
    output: Option<std::path::PathBuf>,

    /// Overlay the configured date/time watermark
    #[arg(long)]
    watermark: bool,

    /// Open an existing image in the annotation editor instead of capturing
    #[arg(short, long, value_name = "FILE")]
    edit: Option<std::path::PathBuf>,

    /// Print session diagnostics (display server, portal, fallback tools) and exit
    #[arg(long)]
    doctor: bool,
}

impl Cli {
    fn capture_options(&self) -> CaptureOptions {
        CaptureOptions {
            format_idx: if self.format == "png" { 0 } else { 1 },
            jpeg_quality: self.quality,
            watermark: self.watermark,
            save_dir: self.output.clone(),
            mode: self.mode.into(),
            ..CaptureOptions::default()
        }
    }
}

fn main() {
    // Must run before any thread exists: it calls setlocale, which mutates
    // process-global state the C library reads without synchronization.
    i18n::init();

    let cli = Cli::parse();

    // zbus (used by ashpd for portal communication) requires a tokio runtime,
    // and the save pipeline dispatches its CPU-bound work through
    // spawn_blocking. Entering the runtime context here lets futures spawned on
    // the GLib main loop reach both.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    let _guard = rt.enter();

    let app = SuperShotApp::new();

    if cli.doctor {
        // A display connection is needed to report the real GDK backend, so
        // this runs inside activate like the other headless paths.
        app.set_headless(true);
        app.connect_activate(|app| {
            println!("{}", capture::session_report());
            gtk4::prelude::ApplicationExt::quit(app);
        });
        app.run_with_args(&["supershot"]);
        return;
    }

    if let Some(path) = cli.edit {
        // Annotating a file that is already on disk never involves the portal.
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if !path.is_file() {
            eprintln!("supershot: {}: not a file", path.display());
            std::process::exit(1);
        }
        app.set_edit_target(Some(path));
        app.run_with_args(&["supershot"]);
        return;
    }

    if cli.now {
        // Headless mode still needs a GTK context: ashpd talks to the portal
        // over D-Bus through the GLib main loop, and the clipboard needs a
        // display connection.
        app.set_headless(true);
        let options = cli.capture_options();
        let delay = cli.delay;
        app.connect_activate(move |_app| {
            capture::start_headless(delay, options.clone());
        });
        // Arguments were already consumed by clap; hand GApplication only the
        // program name so it does not try to parse them a second time.
        app.run_with_args(&["supershot"]);
    } else {
        // The window is created by the application's own `activate`
        // implementation; no additional handler is needed here.
        app.run_with_args(&["supershot"]);
    }
}
