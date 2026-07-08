#!/bin/bash
# Build the Pomodoro Tauri app
# Requires: Rust (stable-gnu), MSYS2 MinGW-w64 at C:\msys64\mingw64\bin

export PATH="/c/msys64/mingw64/bin:$PATH"
cd "$(dirname "$0")/src-tauri"
cargo build "$@"
