// SuperShot - Application constants
// Copyright (c) 2026 axpnet <axp@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Centralizes identifiers and paths used across the application.
// The APP_ID must match the GSettings schema id, the .desktop file name,
// and the AppStream metadata component id.

/// Reverse-DNS application identifier, used for GSettings, D-Bus, and desktop integration.
pub const APP_ID: &str = "com.github.axpnet.SuperShot";

/// Gettext domain name for i18n string lookup.
pub const GETTEXT_PACKAGE: &str = "supershot";

/// Standard system path for compiled gettext locale catalogs.
pub const LOCALEDIR: &str = "/usr/share/locale";
