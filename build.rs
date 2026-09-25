// SuperShot - Build script
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Two jobs, both scoped to OUT_DIR so nothing is ever written into $HOME:
//
//   1. Compile the gettext catalogs in po/*.po into binary .mo files under
//      OUT_DIR/locale/<lang>/LC_MESSAGES/supershot.mo. Packaging scripts copy
//      that tree into <prefix>/share/locale; `cargo run` picks it up directly
//      via the SUPERSHOT_LOCALEDIR variable emitted below.
//
//   2. Compile the GSettings XML schema into OUT_DIR/schemas/ so `cargo run`
//      works without a system-wide installation step, via the
//      SUPERSHOT_BUILD_SCHEMADIR variable emitted below.
//
// The schema is compiled into OUT_DIR even for release builds: an installed
// build resolves its own prefix at runtime and never consults OUT_DIR, but a
// developer still using plain `cargo run` needs the schema available no matter
// the profile.
//
// Historically the development schema was installed into
// $HOME/.local/share/glib-2.0/schemas instead. That directory outranks the
// system schema sources, so a dev build from an older release silently
// shadowed the freshly installed packaging schema and the application aborted
// with "Settings schema ... does not contain a key named '...'" because gio
// treats a missing key as a fatal error. Writing into $HOME during a build
// also made the build non-reproducible and polluted the home in distro
// packaging, Flatpak and Snap build environments, which all run with a
// synthetic HOME.

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

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is always set by cargo"));
    let locale_root = out_dir.join("locale");

    compile_catalogs(&locale_root);

    // Point the running binary at the freshly compiled catalogs. This makes
    // translations work under `cargo run` without installing anything; an
    // installed build resolves its own prefix at runtime instead.
    println!(
        "cargo:rustc-env=SUPERSHOT_BUILD_LOCALEDIR={}",
        locale_root.display()
    );

    let schema_root = out_dir.join("schemas");
    compile_schema(&schema_root);
    println!(
        "cargo:rustc-env=SUPERSHOT_BUILD_SCHEMADIR={}",
        schema_root.display()
    );
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

/// Compile the GSettings schema into `schema_dir`, producing a
/// `gschemas.compiled` the running binary can load directly.
///
/// A missing `glib-compile-schemas` or a broken XML is a warning, not an error:
/// the application falls back gracefully at runtime when no compiled schema is
/// available, so a deficient build toolchain must not break the build.
fn compile_schema(schema_dir: &Path) {
    let schema_src = "data/com.github.axpnet.SuperShot.gschema.xml";
    if !Path::new(schema_src).exists() {
        println!("cargo:warning=GSettings schema not found at {}", schema_src);
        return;
    }

    if let Err(e) = std::fs::create_dir_all(schema_dir) {
        println!(
            "cargo:warning=cannot create {}: {}",
            schema_dir.display(),
            e
        );
        return;
    }

    let copied = schema_dir.join("com.github.axpnet.SuperShot.gschema.xml");
    if let Err(e) = std::fs::copy(schema_src, &copied) {
        println!(
            "cargo:warning=cannot copy schema to {}: {}",
            copied.display(),
            e
        );
        return;
    }

    match Command::new("glib-compile-schemas")
        .arg(schema_dir)
        .status()
    {
        Ok(status) if !status.success() => {
            println!(
                "cargo:warning=glib-compile-schemas exited with status {}",
                status
            );
        }
        Err(e) => {
            println!(
                "cargo:warning=glib-compile-schemas not found or failed: {}",
                e
            );
        }
        _ => {}
    }
}
