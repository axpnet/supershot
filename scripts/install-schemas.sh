#!/bin/bash
SCHEMA_DIR="${HOME}/.local/share/glib-2.0/schemas"
mkdir -p "${SCHEMA_DIR}"
cp data/com.github.axpnet.SuperShot.gschema.xml "${SCHEMA_DIR}/"
glib-compile-schemas "${SCHEMA_DIR}"
echo "GSettings schema installed to ${SCHEMA_DIR}"
