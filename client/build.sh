#!/usr/bin/env bash
set -e

echo "=========================================="
echo " 1. Building release binaries via Trunk..."
echo "=========================================="
trunk build --release

DIST_DIR="./dist"

if ! command -v wasm-snip &> /dev/null; then
    echo "Error: wasm-snip is not installed. Run: cargo install wasm-snip"
    exit 1
fi

if ! command -v wasm-opt &> /dev/null; then
    echo "Error: wasm-opt is not installed. Please install Binaryen."
    exit 1
fi

echo "=========================================="
echo " 2. Snapping panic code & Optimizing WASM..."
echo "=========================================="

for WASM_FILE in "$DIST_DIR"/*.wasm; do
    if [ -f "$WASM_FILE" ]; then
        echo "Processing: $WASM_FILE"

        wasm-snip --pattern ".*panic.*" "$WASM_FILE" -o "$WASM_FILE.snipped"
        wasm-snip --pattern ".*core::fmt.*" "$WASM_FILE.snipped" -o "$WASM_FILE.snipped"

        wasm-opt -Oz \
            --strip-debug \
            --strip-dwarf \
            --strip-producers \
            --coalesce-locals \
            --reroute-calls \
            "$WASM_FILE.snipped" -o "$WASM_FILE"

        rm "$WASM_FILE.snipped"

        echo "Successfully optimized: $WASM_FILE"
    fi
done

echo "=========================================="
echo " Build & Obfuscation Complete!"
echo " Artifacts ready in: $DIST_DIR"
echo "=========================================="
