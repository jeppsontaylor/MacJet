#!/bin/bash
# Build the MacJet native Swift helper
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
HELPER_SRC="$SCRIPT_DIR/macjet-helper.swift"
BUILD_DIR="${HOME}/.macjet/bin"

mkdir -p "$BUILD_DIR"

echo "🔨 Compiling MacJet Swift helper..."
swiftc \
    -O \
    -framework AppKit \
    -framework ApplicationServices \
    -o "$BUILD_DIR/macjet-helper" \
    "$HELPER_SRC"

echo "✅ Built: $BUILD_DIR/macjet-helper"

# Test it
echo "🧪 Testing..."
"$BUILD_DIR/macjet-helper" --test
