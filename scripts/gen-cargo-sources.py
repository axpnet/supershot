#!/usr/bin/env python3
"""Generate cargo-sources.json for the Flatpak build from Cargo.lock.

SuperShot's Flatpak module builds with `cargo --offline`, which needs every
crate vendored ahead of time. flatpak-builder does the vendoring itself from a
manifest of archive sources, and that manifest has to be regenerated whenever
Cargo.lock changes — otherwise the offline build fails, or worse, silently
builds an older dependency set than the one that was tested.

Every hash written here comes from Cargo.lock's own `checksum` field, so this
script needs no network access and cannot introduce a mismatch of its own.

Usage: scripts/gen-cargo-sources.py [output.json]
"""

import json
import os
import sys
import tomllib

CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
VENDOR = "cargo/vendor"


def main() -> int:
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    lock_path = os.path.join(root, "Cargo.lock")
    out_path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, "cargo-sources.json")

    with open(lock_path, "rb") as handle:
        lock = tomllib.load(handle)

    sources = []
    vendored = 0
    skipped = []

    for package in sorted(lock.get("package", []), key=lambda p: (p["name"], p["version"])):
        name = package["name"]
        version = package["version"]
        source = package.get("source")
        checksum = package.get("checksum")

        # The workspace member itself has no source and is built from the
        # checkout rather than vendored.
        if source is None:
            continue

        if source != CRATES_IO or not checksum:
            # A git or path dependency would need a different source entry.
            # None exist today; fail loudly rather than emit a manifest that
            # would break at build time in CI.
            skipped.append(f"{name} {version} ({source})")
            continue

        dest = f"{VENDOR}/{name}-{version}"

        sources.append({
            "type": "archive",
            "archive-type": "tar-gzip",
            "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
            "sha256": checksum,
            "dest": dest,
        })
        # Cargo refuses to use a vendored crate without this file. An empty
        # `files` map tells it not to verify individual file hashes, which is
        # what the official generator emits too.
        sources.append({
            "type": "inline",
            "contents": json.dumps({"package": checksum, "files": {}}),
            "dest": dest,
            "dest-filename": ".cargo-checksum.json",
        })
        vendored += 1

    if skipped:
        print("error: non-crates.io dependencies need manual handling:", file=sys.stderr)
        for entry in skipped:
            print(f"  - {entry}", file=sys.stderr)
        return 1

    cargo_config = (
        "[source.crates-io]\n"
        'replace-with = "vendored-sources"\n'
        "\n"
        "[source.vendored-sources]\n"
        f'directory = "{VENDOR}"\n'
    )
    sources.append({
        "type": "inline",
        "contents": cargo_config,
        "dest": "cargo",
        "dest-filename": "config.toml",
    })

    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(sources, handle, indent=2)
        handle.write("\n")

    print(f"{out_path}: {vendored} crates vendored")
    return 0


if __name__ == "__main__":
    sys.exit(main())
