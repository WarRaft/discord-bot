#!/bin/bash
#
# Build script for Discord Bot
# 
# Prerequisites:
#   1. Copy build-sensitive.default.sh to build-sensitive.sh
#   2. Fill in your credentials in build-sensitive.sh
#
# Usage:
#   ./build.sh
#
set -e

# Load sensitive configuration
if [ ! -f "build-sensitive.sh" ]; then
    echo "❌ File 'build-sensitive.sh' not found!"
    echo "📝 Copy 'build-sensitive.default.sh' to 'build-sensitive.sh' and fill in your credentials."
    exit 1
fi

source build-sensitive.sh

DIST_DIR="bin"

# Clean bin directory before build
echo "🧹 Cleaning $DIST_DIR directory..."
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

echo "🐧 Building for Linux (x86_64-unknown-linux-musl)..."
rustup target add x86_64-unknown-linux-musl &>/dev/null || true
DISCORD_BOT_TOKEN="$DISCORD_TOKEN" MONGO_URL="$MONGO_URL" MONGO_DB="$MONGO_DB" cargo build --release --target x86_64-unknown-linux-musl
cp "target/x86_64-unknown-linux-musl/release/$PROJECT_NAME" "$DIST_DIR/$BINARY_NAME"

echo ""
echo "✅ Build complete:"
ls -lh "$DIST_DIR"