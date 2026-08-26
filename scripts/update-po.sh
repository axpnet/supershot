#!/bin/bash
# SuperShot - Regenerate the translation template and merge it into the catalogs.
# Copyright (c) 2026 axpnet <https://github.com/axpnet>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Two kinds of translatable string live in this project and xgettext cannot see
# both with one invocation:
#
#   1. gettext("…") calls in the Rust sources.
#   2. translatable="yes" attributes in the GtkBuilder template, which is
#      embedded in src/window.rs as a Rust raw string rather than shipped as a
#      standalone .ui file.
#
# The template is therefore extracted to a temporary .ui file so xgettext can
# parse it as Glade markup, and the two catalogs are concatenated.

set -euo pipefail
cd "$(dirname "$0")/.."

command -v xgettext >/dev/null || { echo "xgettext not found (install gettext)" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"

# --- 1. Rust sources -------------------------------------------------------
xgettext \
    --language=C \
    --keyword=gettext \
    --keyword=ngettext:1,2 \
    --from-code=UTF-8 \
    --add-comments=TRANSLATORS \
    --package-name=supershot \
    --package-version="$VERSION" \
    --msgid-bugs-address=axp@pm.me \
    --output="$WORK/rust.pot" \
    $(grep -v '^#' po/POTFILES.in | grep '\.rs$')

# --- 2. Inline GtkBuilder template ----------------------------------------
python3 - "$WORK/template.ui" <<'PYEOF'
import re, sys

src = open('src/window.rs', encoding='utf-8').read()
match = re.search(r'#\[template\(string = r#"(.*?)"#\)\]', src, re.S)
if not match:
    raise SystemExit('could not locate the inline GtkBuilder template in src/window.rs')
open(sys.argv[1], 'w', encoding='utf-8').write(match.group(1))
PYEOF

xgettext \
    --language=Glade \
    --from-code=UTF-8 \
    --output="$WORK/ui.pot" \
    "$WORK/template.ui"

# Point references at the real file rather than the temporary one.
sed -i "s|$WORK/template.ui|src/window.rs|g" "$WORK/ui.pot"

# --- 3. Merge --------------------------------------------------------------
msgcat --use-first "$WORK/rust.pot" "$WORK/ui.pot" --output-file=po/supershot.pot
sed -i 's/charset=CHARSET/charset=UTF-8/' po/supershot.pot

echo "po/supershot.pot: $(grep -c '^msgid "' po/supershot.pot) entries"

# --- 4. Merge into every catalog listed in LINGUAS --------------------------
while read -r lang; do
    [ -z "$lang" ] && continue
    case "$lang" in \#*) continue ;; esac
    if [ -f "po/$lang.po" ]; then
        msgmerge --quiet --update --backup=none "po/$lang.po" po/supershot.pot
    else
        msginit --no-translator --locale="$lang" --input=po/supershot.pot --output-file="po/$lang.po"
    fi
    printf '%-8s %s\n' "$lang" "$(msgfmt --statistics -o /dev/null "po/$lang.po" 2>&1)"
done < po/LINGUAS
