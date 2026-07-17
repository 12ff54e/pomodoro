// E2E test binary root. Uses a minimal WebDriver client (no heavyweight
// dependencies) to interact with a real Tauri app process via tauri-driver.
//
// Prerequisites:
//   1. tauri-driver running:  tauri-driver --port 4445 &
//   2. App built with test mode: POMODORO_TEST_MODE=1 cargo build
//
// Usage:
//   APP_PATH=./target/debug/pomodoro.exe \
//   TAURI_DRIVER_URL=http://127.0.0.1:4445 \
//   cargo test --features e2e-test --test e2e -- --test-threads=1 --nocapture

mod webdriver;
mod tests;
