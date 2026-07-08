#!/bin/bash
# Build and package the Pomodoro Tauri app.
# Requires: Rust (stable-gnu), MSYS2 MinGW-w64 at C:\msys64\mingw64\bin
#
# Usage:
#   ./build.sh              debug build
#   ./build.sh --release    release build + zip

set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
export PATH="/c/msys64/mingw64/bin:$PATH"

RELEASE=false
BUILD_FLAGS=""
for arg in "$@"; do
  case "$arg" in
    --release|-r) RELEASE=true ;;
    *) BUILD_FLAGS="$BUILD_FLAGS $arg" ;;
  esac
done

cd "$ROOT/src-tauri"

PROFILE="debug"
if $RELEASE; then
  PROFILE="release"
  cargo build --release
else
  cargo build
fi

# --- Package release ---
if $RELEASE; then
  VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
  PKG_DIR="$ROOT/target/release/pomodoro-v$VERSION"
  ZIP_FILE="$ROOT/pomodoro-v$VERSION.zip"

  rm -rf "$PKG_DIR" "$ZIP_FILE"
  mkdir -p "$PKG_DIR"
  cp "target/$PROFILE/pomodoro.exe" "$PKG_DIR/"
  cp "target/$PROFILE/WebView2Loader.dll" "$PKG_DIR/"

  echo ""
  echo "=== Release package ==="
  ls -lh "$PKG_DIR"/

  # Zip with the folder as the root (so it extracts cleanly).
  cd "$ROOT/target/release"
  zip -r "$ZIP_FILE" "pomodoro-v$VERSION"
  echo ""
  echo "Created: $ZIP_FILE"
  ls -lh "$ZIP_FILE"
fi
