// SuperShot - Build script
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Two jobs:
//
//   1. Compile the gettext catalogs in po/*.po into binary .mo files under
//      OUT_DIR/locale/<lang>/LC_MESSAGES/supershot.mo. Packaging scripts copy
//      that tree into <prefix>/share/locale; `cargo run` picks it up directly
//      via the SUPERSHOT_LOCALEDIR variable emitted below.
//
//   2. For development builds only, install and compile the GSettings XML
//      schema into the user's local schema directory so `cargo run` works
//      without a system-wide installation step.
//
// Job 2 is deliberately skipped for release builds and whenever
// SUPERSHOT_NO_DEV_SCHEMA is set: writing into $HOME during a build makes the
// build non-reproducible and pollutes the user's home in distro packaging,
// Flatpak and Snap build environments, which all run with a synthetic HOME.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // Track each catalog individually. A `rerun-if-changed` on the `po`
    // directory only notices files being added or removed, because that is all
    // a directory's mtime records — editing a translation in place would leave
    // the compiled catalogs stale and silently ship the previous release's
    // strings.
    println!("cargo:rerun-if-changed=po/LINGUAS");
    if let Ok(entries) = std::fs::read_dir("po") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "po") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    println!("cargo:rerun-if-changed=data/com.github.axpnet.SuperShot.gschema.xml");
    println!("cargo:rerun-if-env-changed=SUPERSHOT_NO_DEV_SCHEMA");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is always set by cargo"));
    let locale_root = out_dir.join("locale");

    compile_catalogs(&locale_root);

    // Point the running binary at the freshly compiled catalogs. This makes
    // translations work under `cargo run` without installing anything; an
    // installed build resolves its own prefix at runtime instead.
    println!("cargo:rustc-env=SUPERSHOT_BUILD_LOCALEDIR={}", locale_root.display());

    if std::env::var("PROFILE").as_deref() == Ok("debug")
        && std::env::var_os("SUPERSHOT_NO_DEV_SCHEMA").is_none()
    {
        install_dev_schema();
    }
}

/// Compile every catalog listed in po/LINGUAS into OUT_DIR/locale.
///
/// A missing `msgfmt` is a warning, not an error: the application falls back
/// to its English source strings, so a translator toolchain must not be a hard
/// build requirement for users compiling from source.
fn compile_catalogs(locale_root: &Path) {
    let linguas = match std::fs::read_to_string("po/LINGUAS") {
        Ok(s) => s,
        Err(_) => {
            println!("cargo:warning=po/LINGUAS not found; translations will be unavailable");
            return;
        }
    };

    let mut compiled = 0usize;
    let mut msgfmt_missing = false;

    for lang in linguas
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let po = PathBuf::from("po").join(format!("{}.po", lang));
        if !po.exists() {
            println!("cargo:warning=po/{}.po listed in LINGUAS but missing", lang);
            continue;
        }

        let dest_dir = locale_root.join(lang).join("LC_MESSAGES");
        if let Err(e) = std::fs::create_dir_all(&dest_dir) {
            println!("cargo:warning=cannot create {}: {}", dest_dir.display(), e);
            continue;
        }
        let mo = dest_dir.join("supershot.mo");

        match Command::new("msgfmt")
            .arg("--check-format")
            .arg("-o")
            .arg(&mo)
            .arg(&po)
            .output()
        {
            Ok(out) if out.status.success() => compiled += 1,
            Ok(out) => {
                println!(
                    "cargo:warning=msgfmt failed for {}: {}",
                    lang,
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Err(_) => {
                msgfmt_missing = true;
                break;
            }
        }
    }

    if msgfmt_missing {
        println!(
            "cargo:warning=msgfmt not found (install gettext); \
             the build will proceed with English strings only"
        );
    } else if compiled == 0 {
        println!("cargo:warning=no translation catalogs were compiled");
    }
}

/// Development convenience: make `cargo run` find the GSettings schema.
fn install_dev_schema() {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };

    let schema_src = "data/com.github.axpnet.SuperShot.gschema.xml";
    if !Path::new(schema_src).exists() {
        println!("cargo:warning=GSettings schema not found at {}", schema_src);
        return;
    }

    let schema_dir = PathBuf::from(home).join(".local/share/glib-2.0/schemas");
    if let Err(e) = std::fs::create_dir_all(&schema_dir) {
        println!("cargo:warning=Failed to create schema directory {}: {}", schema_dir.display(), e);
        return;
    }

    let dest = schema_dir.join("com.github.axpnet.SuperShot.gschema.xml");
    if let Err(e) = std::fs::copy(schema_src, &dest) {
        println!("cargo:warning=Failed to copy schema to {}: {}", dest.display(), e);
        return;
    }

    match Command::new("glib-compile-schemas").arg(&schema_dir).status() {
        Ok(s) if !s.success() => {
            println!("cargo:warning=glib-compile-schemas exited with status {}", s);
        }
        Err(e) => {
            println!("cargo:warning=glib-compile-schemas not found or failed: {}", e);
        }
        _ => {}
    }
}
