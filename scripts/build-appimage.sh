#!/bin/bash
# SuperShot - Build a self-contained AppImage.
# Copyright (c) 2026 axpnet <https://github.com/axpnet>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The AppImage exists for distributions that ship a GTK4 or libadwaita older
# than SuperShot needs, and for immutable systems where installing a .deb is
# not an option. It bundles the GTK stack; capture itself still goes through
# the host's desktop portal or its command-line screenshot tools, both of which
# an AppImage can reach because it is not sandboxed.
#
# Usage: scripts/build-appimage.sh [ARCH]

set -euo pipefail
cd "$(dirname "$0")/.."

ARCH="${ARCH:-${1:-$(uname -m)}}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
APP_ID="com.github.axpnet.SuperShot"
APPDIR="build/AppDir"
TOOLS="build/appimage-tools"

mkdir -p "$TOOLS"

fetch() {
    local url="$1" dest="$2"
    if [ ! -x "$dest" ]; then
        echo "downloading $(basename "$dest")"
        curl -fsSL -o "$dest" "$url"
        chmod +x "$dest"
    fi
}

BASE="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous"
GTK_BASE="https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master"

fetch "$BASE/linuxdeploy-$ARCH.AppImage"        "$TOOLS/linuxdeploy"
fetch "$GTK_BASE/linuxdeploy-plugin-gtk.sh"     "$TOOLS/linuxdeploy-plugin-gtk.sh"

# --- Stage the application ------------------------------------------------
rm -rf "$APPDIR"
# The AppImage runtime mounts the payload at an arbitrary path, so SuperShot
# resolves its data files relative to the executable at runtime. Installing
# under /usr inside the AppDir is what makes that resolution land correctly.
scripts/install.sh "$APPDIR" /usr

# glib needs the compiled schema, not the XML.
glib-compile-schemas "$APPDIR/usr/share/glib-2.0/schemas/"

export PATH="$TOOLS:$PATH"
export LDAI_OUTPUT="SuperShot-$VERSION-$ARCH.AppImage"
export LDAI_UPDATE_INFORMATION="gh-releases-zsync|axpnet|supershot|latest|SuperShot-*-$ARCH.AppImage.zsync"
export DEPLOY_GTK_VERSION=4

"$TOOLS/linuxdeploy" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/supershot" \
    --desktop-file "$APPDIR/usr/share/applications/$APP_ID.desktop" \
    --icon-file "data/icons/hicolor/scalable/apps/$APP_ID.svg" \
    --plugin gtk \
    --output appimage

echo "built $LDAI_OUTPUT"
