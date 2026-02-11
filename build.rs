// SuperShot - Build script
// Copyright (c) 2026 axpnet <axp@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Automatically installs the GSettings XML schema into the user's local
// schema directory during development builds. This allows `cargo run`
// to work without requiring a system-wide installation step.
//
// For production packaging (deb, Flatpak), the schema should be installed
// to /usr/share/glib-2.0/schemas/ via the package manager instead.

use std::process::Command;

fn main() {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };

    let schema_dir = format!("{}/.local/share/glib-2.0/schemas", home);
    let schema_src = "data/com.github.axpnet.SuperShot.gschema.xml";

    if !std::path::Path::new(schema_src).exists() {
        println!("cargo:warning=GSettings schema not found at {}", schema_src);
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&schema_dir) {
        println!("cargo:warning=Failed to create schema directory {}: {}", schema_dir, e);
        return;
    }

    let dest = format!("{}/com.github.axpnet.SuperShot.gschema.xml", schema_dir);
    if let Err(e) = std::fs::copy(schema_src, &dest) {
        println!("cargo:warning=Failed to copy schema to {}: {}", dest, e);
        return;
    }

    let status = Command::new("glib-compile-schemas")
        .arg(&schema_dir)
        .status();

    match status {
        Ok(s) if !s.success() => {
            println!("cargo:warning=glib-compile-schemas exited with status {}", s);
        }
        Err(e) => {
            println!("cargo:warning=glib-compile-schemas not found or failed: {}", e);
        }
        _ => {}
    }
}
