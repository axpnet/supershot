// SuperShot - Screenshot tool for GNOME
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Application entry point. Parses CLI arguments and dispatches
// to either headless capture mode or the full GTK4 GUI.

mod app;
mod capture;
mod config;
mod editing;
mod i18n;
mod preview;
mod window;

use app::SuperShotApp;
use clap::Parser;
use gtk4::prelude::*;

/// Command-line interface definition.
///
/// Supports two execution modes:
/// - GUI mode (default): opens the main window with interactive controls.
/// - Headless mode (--now): opens the portal screenshot UI and exits after capture.
#[derive(Parser, Debug)]
#[command(name = "supershot", about = "GTK4 screenshot tool for GNOME with delay timer")]
struct Cli {
    /// Delay in seconds before capture
    #[arg(short, long, default_value_t = 0)]
    delay: u32,

    /// Take screenshot immediately without GUI
    #[arg(long)]
    now: bool,
}

fn main() {
    i18n::init();

    // zbus 5.x (used by ashpd 0.12) unconditionally requires a tokio runtime.
    // Create one and enter its context so async portal calls spawned on the
    // GLib main loop via glib::spawn_future_local() can access the tokio reactor.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    let _guard = rt.enter();

    let cli = Cli::parse();

    if cli.now {
        // Headless mode: start a minimal GTK application, perform capture, and exit.
        // A GTK context is still required because ashpd operates over D-Bus
        // through the GLib main loop.
        let app = SuperShotApp::new();
        let delay = cli.delay;

        app.connect_activate(move |_app| {
            capture::start_headless(delay);
        });
        app.run_with_args::<String>(&[]);
    } else {
        // GUI mode: launch the full Adwaita window.
        let app = SuperShotApp::new();
        app.run();
    }
}
