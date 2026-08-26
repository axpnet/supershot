// SuperShot - Internationalization setup
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Initializes GNU gettext for runtime string translation. The catalog
// directory is resolved at runtime by `config::localedir()` so that the same
// binary finds its translations under /usr, /app (Flatpak), $SNAP, or an
// AppImage mountpoint.
//
// Initialization failures are logged to stderr but do not terminate the
// application, as English source strings remain usable.

use gettextrs::{bindtextdomain, bind_textdomain_codeset, setlocale, textdomain, LocaleCategory};
use crate::config;

/// Initialize the gettext text domain and locale settings.
///
/// Must be called before any translatable string is accessed, and before any
/// thread is spawned.
///
/// # Safety contract
///
/// `setlocale` mutates process-global state that the C library reads without
/// synchronization (RUSTSEC-2026-0244). This function is called exactly once,
/// as the first statement of `main`, before the tokio runtime is built and
/// therefore before any other thread exists. No other code in SuperShot calls
/// `setlocale` or mutates the environment.
pub fn init() {
    // SAFETY: single-threaded at this point — see the contract above.
    unsafe {
        setlocale(LocaleCategory::LcAll, "");
    }

    let localedir = config::localedir();
    if let Err(e) = bindtextdomain(config::GETTEXT_PACKAGE, &localedir) {
        eprintln!(
            "gettext: failed to bind text domain at {}: {}",
            localedir.display(),
            e
        );
    }
    if let Err(e) = bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8") {
        eprintln!("gettext: failed to set text domain encoding: {}", e);
    }
    if let Err(e) = textdomain(config::GETTEXT_PACKAGE) {
        eprintln!("gettext: failed to switch text domain: {}", e);
    }
}
