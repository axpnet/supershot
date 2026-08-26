#!/bin/bash
# SuperShot - Assemble a .deb from an already-built release binary.
# Copyright (c) 2026 axpnet <https://github.com/axpnet>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The version and architecture are derived rather than hard-coded: the previous
# static control file drifted a release behind Cargo.toml, so the 1.2.4 package
# was published declaring itself 1.2.3.
#
# Dependencies come from dpkg-shlibdeps, which reads the actual ELF. The old
# hand-written list named only GTK and libadwaita and omitted cairo,
# gdk-pixbuf, pango and glibc, so the package would install on a system that
# could not run it.

set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
ARCH="${DEB_ARCH:-$(dpkg --print-architecture)}"
STAGE="${1:-build/deb}"

rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN"

scripts/install.sh "$STAGE"

install -Dm644 pkg-deb/copyright "$STAGE/usr/share/doc/supershot/copyright"
rm -f "$STAGE/usr/share/doc/supershot/LICENSE"

# Debian requires a changelog in its own format in every binary package. The
# upstream Markdown changelog ships alongside it under its own name.
DATE="$(date -R)"
{
    printf 'supershot (%s) unstable; urgency=medium\n\n' "$VERSION"
    printf '  * Release %s. See /usr/share/doc/supershot/CHANGELOG.md for details.\n\n' "$VERSION"
    printf ' -- axpnet <axp@pm.me>  %s\n' "$DATE"
} | gzip -9n > "$STAGE/usr/share/doc/supershot/changelog.gz"
chmod 644 "$STAGE/usr/share/doc/supershot/changelog.gz"

if [ -f docs/CHANGELOG.md ]; then
    install -Dm644 docs/CHANGELOG.md "$STAGE/usr/share/doc/supershot/CHANGELOG.md"
fi

# Debug symbols roughly triple the package size and are of no use to users
# installing a release build.
strip --strip-unneeded "$STAGE/usr/bin/supershot" 2>/dev/null || true
install -m755 pkg-deb/DEBIAN/postinst "$STAGE/DEBIAN/postinst"
install -m755 pkg-deb/DEBIAN/postrm   "$STAGE/DEBIAN/postrm"

# --- Dependencies ---------------------------------------------------------
if command -v dpkg-shlibdeps >/dev/null; then
    mkdir -p "$STAGE/debian"
    touch "$STAGE/debian/control"
    SHLIBDEPS="$(cd "$STAGE" && dpkg-shlibdeps -O --ignore-missing-info usr/bin/supershot 2>/dev/null \
        | sed 's/^shlibs:Depends=//')"
    rm -rf "$STAGE/debian"
fi
# Fall back to a conservative hand-written set when shlibdeps is unavailable.
SHLIBDEPS="${SHLIBDEPS:-libc6, libgtk-4-1 (>= 4.14), libadwaita-1-0 (>= 1.5), libcairo2, libpango-1.0-0, libpangocairo-1.0-0, libgdk-pixbuf-2.0-0, libglib2.0-0}"
# shlibdeps reads the symbols the binary actually references, which understates
# the requirement: SuperShot is compiled against the gtk4 "v4_14" and
# libadwaita "v1_5" feature gates, so it needs those API levels even where it
# happens not to call a symbol introduced in them.
SHLIBDEPS="$(printf '%s' "$SHLIBDEPS" \
    | sed -E 's/libgtk-4-1 \(>= [^)]*\)/libgtk-4-1 (>= 4.14)/' \
    | sed -E 's/libadwaita-1-0 \(>= [^)]*\)/libadwaita-1-0 (>= 1.5)/')"

# glib-compile-schemas, invoked from postinst, lives in libglib2.0-bin.
SHLIBDEPS="$SHLIBDEPS, libglib2.0-bin"

sed -e "s|@VERSION@|$VERSION|" \
    -e "s|@ARCH@|$ARCH|" \
    -e "s|@SHLIBDEPS@|$SHLIBDEPS|" \
    pkg-deb/DEBIAN/control.in > "$STAGE/DEBIAN/control"

INSTALLED_SIZE="$(du -ks "$STAGE/usr" | cut -f1)"
sed -i "/^Maintainer:/i Installed-Size: $INSTALLED_SIZE" "$STAGE/DEBIAN/control"

OUT="supershot_${VERSION}_${ARCH}.deb"
dpkg-deb --build --root-owner-group "$STAGE" "$OUT"
echo "built $OUT"
command -v lintian >/dev/null && lintian --no-tag-display-limit "$OUT" || true
