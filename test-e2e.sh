#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# E2E test runner for the Pomodoro Tauri app.
#
# Prerequisites (one-time):
#   cargo install tauri-driver --locked
#   cargo install --git https://github.com/chippers/msedgedriver-tool --locked && msedgedriver-tool --install
#
# Usage:
#   ./test-e2e.sh
# ---------------------------------------------------------------------------
set -euo pipefail

# Ensure MSYS2 MinGW is first on PATH (see CLAUDE.md).
export PATH="/c/msys64/mingw64/bin:$PATH"

# ---- Configuration ----
TAURI_DRIVER_PORT="${TAURI_DRIVER_PORT:-4445}"
export TAURI_DRIVER_URL="http://127.0.0.1:${TAURI_DRIVER_PORT}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/src-tauri"

# ---- Build app in test mode ----
echo "==> Building app (test mode: minutes→seconds)..."
export POMODORO_TEST_MODE=1
cargo build
export APP_PATH="$(pwd)/target/debug/pomodoro.exe"
echo "    Binary: $APP_PATH"

# ---- Start tauri-driver ----
echo "==> Starting tauri-driver on port ${TAURI_DRIVER_PORT}..."
tauri-driver --port "$TAURI_DRIVER_PORT" &
DRIVER_PID=$!

# Cleanup on exit.
cleanup() {
  echo "==> Stopping tauri-driver (pid $DRIVER_PID)..."
  kill "$DRIVER_PID" 2>/dev/null || true
  wait "$DRIVER_PID" 2>/dev/null || true
}
trap cleanup EXIT

# ---- Wait for tauri-driver ----
echo "==> Waiting for tauri-driver to be ready..."
for i in $(seq 1 30); do
  if curl -s "$TAURI_DRIVER_URL/status" > /dev/null 2>&1; then
    echo "    Ready after ${i}s."
    break
  fi
  sleep 1
done

# ---- Run E2E tests ----
echo "==> Running E2E tests..."
cd "$SCRIPT_DIR/src-tauri"
cargo test --features e2e-test --test e2e -- --test-threads=1 --nocapture
EXIT_CODE=$?

echo "==> Done (exit code: $EXIT_CODE)."
exit $EXIT_CODE
