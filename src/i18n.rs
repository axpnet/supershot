// SuperShot - Internationalization setup
// Copyright (c) 2026 axpnet <https://github.com/axpnet>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Initializes GNU gettext for runtime string translation.
// When compiled translations are installed under LOCALEDIR,
// the application automatically displays localized strings.
// Without installed translations, all strings fall back to the
// English source text.
//
// Initialization failures are logged to stderr but do not terminate
// the application, as English source strings remain usable.

use gettextrs::{bindtextdomain, bind_textdomain_codeset, setlocale, textdomain, LocaleCategory};
use crate::config;

/// Initialize the gettext text domain and locale settings.
///
/// Must be called before any translatable string is accessed.
/// Failures are non-fatal: the application degrades gracefully
/// to the English source strings embedded in the binary.
pub fn init() {
    setlocale(LocaleCategory::LcAll, "");
    if let Err(e) = bindtextdomain(config::GETTEXT_PACKAGE, config::LOCALEDIR) {
        eprintln!("gettext: failed to bind text domain: {}", e);
    }
    if let Err(e) = bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8") {
        eprintln!("gettext: failed to set text domain encoding: {}", e);
    }
    if let Err(e) = textdomain(config::GETTEXT_PACKAGE) {
        eprintln!("gettext: failed to switch text domain: {}", e);
    }
}
