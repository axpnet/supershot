// SuperShot - Application constants and runtime path resolution
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Centralizes identifiers and paths used across the application.
// The APP_ID must match the GSettings schema id, the .desktop file name,
// and the AppStream metadata component id.
//
// Installation-dependent paths (locale catalogs) are resolved at runtime
// rather than baked in at compile time, because SuperShot is shipped through
// four channels that each place data files somewhere different:
//
//   .deb / distro    /usr/share/locale
//   Flatpak          /app/share/locale
//   Snap             $SNAP/usr/share/locale
//   AppImage         <mountpoint>/usr/share/locale
//
// A compile-time constant would silently disable all 14 translations in three
// of those four cases.

use std::path::PathBuf;

/// Reverse-DNS application identifier, used for GSettings, D-Bus, and desktop integration.
pub const APP_ID: &str = "com.github.axpnet.SuperShot";

/// Gettext domain name for i18n string lookup.
pub const GETTEXT_PACKAGE: &str = "supershot";

/// Release version, taken from Cargo.toml so the two cannot drift apart.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Last-resort locale directory when no installation prefix can be determined.
const FALLBACK_LOCALEDIR: &str = "/usr/share/locale";

/// True when running inside a Flatpak sandbox.
pub fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// True when running inside a Snap confinement.
pub fn is_snap() -> bool {
    std::env::var_os("SNAP").is_some()
}

/// True when running inside any sandbox that blocks execution of host binaries.
///
/// Consulted by the capture pipeline: the CLI screenshot fallbacks
/// (`grim`, `scrot`, `gnome-screenshot`, …) live on the host and are
/// unreachable from strict confinement, so suggesting that the user install
/// them would be misleading advice.
pub fn is_sandboxed() -> bool {
    is_flatpak() || is_snap()
}

/// Resolve the directory holding compiled gettext catalogs (`*/LC_MESSAGES/supershot.mo`).
///
/// Candidates are probed in order of specificity and the first one that exists
/// on disk wins. `SUPERSHOT_LOCALEDIR` overrides everything, which is what the
/// development build uses to point at the catalogs `build.rs` compiles into
/// the target directory.
pub fn localedir() -> PathBuf {
    for candidate in locale_candidates() {
        if candidate.is_dir() {
            return candidate;
        }
    }
    PathBuf::from(FALLBACK_LOCALEDIR)
}

fn locale_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = std::env::var_os("SUPERSHOT_LOCALEDIR") {
        candidates.push(PathBuf::from(dir));
    }

    if is_flatpak() {
        candidates.push(PathBuf::from("/app/share/locale"));
    }

    if let Some(snap) = std::env::var_os("SNAP") {
        candidates.push(PathBuf::from(snap).join("usr/share/locale"));
    }

    // Derive the prefix from the running executable: <prefix>/bin/supershot
    // yields <prefix>/share/locale. This covers /usr, /usr/local, an AppImage
    // mountpoint, and any relocatable install without further configuration.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bindir) = exe.parent() {
            if let Some(prefix) = bindir.parent() {
                candidates.push(prefix.join("share/locale"));
            }
        }
    }

    // Catalogs compiled by build.rs into OUT_DIR. Present for every build, but
    // deliberately probed last among the relocatable candidates so an installed
    // copy always prefers its own prefix. In practice this only ever matches
    // under `cargo run`, where the target directory still exists.
    candidates.push(PathBuf::from(env!("SUPERSHOT_BUILD_LOCALEDIR")));

    candidates.push(PathBuf::from(FALLBACK_LOCALEDIR));
    candidates
}

/// Which packaging channel this build is running from.
pub fn channel() -> &'static str {
    if is_flatpak() {
        "Flatpak"
    } else if is_snap() {
        "Snap"
    } else if std::env::var_os("APPIMAGE").is_some() {
        "AppImage"
    } else {
        "native"
    }
}
