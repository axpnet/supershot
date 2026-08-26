#!/bin/bash
# SuperShot - Install built artefacts into a prefix.
# Copyright (c) 2026 axpnet <https://github.com/axpnet>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# One installer shared by every packaging channel (.deb, Snap, Flatpak,
# AppImage, manual install) so the four cannot drift apart — which is how the
# translation catalogs previously ended up shipped by none of them.
#
# Usage: scripts/install.sh DESTDIR [PREFIX]
#   DESTDIR  staging root, e.g. pkg-deb or $CRAFT_PART_INSTALL
#   PREFIX   install prefix inside DESTDIR, default /usr

set -euo pipefail

# Package tooling expects 0755 directories; a developer umask of 002 would
# otherwise bake 0775 into every shipped directory.
umask 022
cd "$(dirname "$0")/.."

DESTDIR="${1:?usage: install.sh DESTDIR [PREFIX]}"
PREFIX="${2:-/usr}"
ROOT="${DESTDIR%/}${PREFIX}"

APP_ID="com.github.axpnet.SuperShot"
BINARY="${SUPERSHOT_BINARY:-target/release/supershot}"

[ -f "$BINARY" ] || { echo "binary not found at $BINARY (run cargo build --release)" >&2; exit 1; }

install -Dm755 "$BINARY"                                          "$ROOT/bin/supershot"
install -Dm644 "data/$APP_ID.desktop"                             "$ROOT/share/applications/$APP_ID.desktop"
install -Dm644 "data/$APP_ID.gschema.xml"                         "$ROOT/share/glib-2.0/schemas/$APP_ID.gschema.xml"
install -Dm644 "data/$APP_ID.metainfo.xml"                        "$ROOT/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "data/icons/hicolor/scalable/apps/$APP_ID.svg"     "$ROOT/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "data/icons/hicolor/scalable/actions/supershot-capture-symbolic.svg" \
                                                                  "$ROOT/share/icons/hicolor/scalable/actions/supershot-capture-symbolic.svg"

# --- Translation catalogs -------------------------------------------------
# Compiled here rather than copied out of cargo's OUT_DIR: that path is keyed by
# a build hash, exists under both target/debug and target/release, and picking
# the wrong one silently installs a stale catalog.
if command -v msgfmt >/dev/null; then
    count=0
    while read -r lang; do
        [ -z "$lang" ] && continue
        case "$lang" in \#*) continue ;; esac
        [ -f "po/$lang.po" ] || { echo "warning: po/$lang.po missing" >&2; continue; }

        dest="$ROOT/share/locale/$lang/LC_MESSAGES"
        mkdir -p "$dest"
        msgfmt --check-format -o "$dest/supershot.mo" "po/$lang.po"
        chmod 644 "$dest/supershot.mo"
        count=$((count + 1))
    done < po/LINGUAS
    echo "installed $count translation catalogs"
else
    echo "warning: msgfmt not found; the install will be English-only" >&2
fi

# --- Documentation --------------------------------------------------------
install -Dm644 LICENSE   "$ROOT/share/doc/supershot/LICENSE"
install -Dm644 README.md "$ROOT/share/doc/supershot/README.md"

if [ -f data/supershot.1 ]; then
    install -d "$ROOT/share/man/man1"
    gzip -9nc data/supershot.1 > "$ROOT/share/man/man1/supershot.1.gz"
    chmod 644 "$ROOT/share/man/man1/supershot.1.gz"
fi

echo "installed SuperShot into $ROOT"
