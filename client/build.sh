#!/usr/bin/env bash
set -euo pipefail

echo "=========================================="
echo " 1. Building release binaries via Trunk..."
echo "=========================================="

trunk build --release

DIST_DIR="./dist"

if ! command -v wasm-opt &>/dev/null; then
    echo "Error: wasm-opt is not installed."
    echo "Please install Binaryen."
    exit 1
fi

echo "=========================================="
echo " 2. Optimizing WASM..."
echo "=========================================="

for WASM_FILE in "$DIST_DIR"/*.wasm; do
    [ -f "$WASM_FILE" ] || continue

    echo "Processing: $WASM_FILE"

    TEMP_FILE="${WASM_FILE}.tmp"

    wasm-opt \
        -O1 \
        --strip-debug \
        --strip-dwarf \
        --strip-producers \
        # --coalesce-locals \
        # --reroute-calls \
        "$WASM_FILE" \
        -o "$TEMP_FILE"

    mv "$TEMP_FILE" "$WASM_FILE"

    echo "Successfully optimized: $WASM_FILE"
done

echo "=========================================="
echo " Build & Optimization Complete!"
echo " Artifacts ready in: $DIST_DIR"
echo "=========================================="
