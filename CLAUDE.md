# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
# Git Bash users: MSYS2 MinGW must come first — Git's bundled old MinGW
# DLLs cause STATUS_ENTRYPOINT_NOT_FOUND if they shadow the MSYS2 ones.
export PATH="/c/msys64/mingw64/bin:$PATH"

# Build (debug)
cd src-tauri && cargo build

# Build (release)
cd src-tauri && cargo build --release

# Run directly
./src-tauri/target/debug/pomodoro.exe

# Dev mode (with hot-reload, requires tauri-cli)
cargo tauri dev
```

### Android

**Prerequisites:** Android SDK at `D:\app-dev-tools`, JDK 21 at `D:\app-dev-tools\jdk-21.0.2`,
NDK 27 at `D:\app-dev-tools\ndk\27.0.12077973`.  The emulator must already be running.

```bash
# Full build (Rust + Gradle, installs to connected device/emulator)
export PATH="/c/msys64/mingw64/bin:$PATH"
export ANDROID_HOME="D:\\app-dev-tools"
export ANDROID_SDK_ROOT="D:\\app-dev-tools"
export JAVA_HOME="D:\\app-dev-tools\\jdk-21.0.2"
cd src-tauri && cargo tauri android build --debug

# Install the APK on the running emulator
adb install -r gen/android/app/build/outputs/apk/debug/app-debug.apk

# Grant notification permission (needed once after install)
adb shell pm grant com.pomodoro.app android.permission.POST_NOTIFICATIONS

# Launch
adb shell am start -n com.pomodoro.app/.MainActivity
```

**Gradle-only rebuild** (when only Kotlin/XML changed, skip Rust compilation):

```bash
export JAVA_HOME="D:\\app-dev-tools\\jdk-21.0.2"
export ANDROID_HOME="D:\\app-dev-tools"
cd src-tauri/gen/android
cmd.exe //c "gradlew.bat assembleDebug"
```

## Testing

Two test suites — run both before tagging a release:

```bash
# 1. Rust unit tests (44 tests — state logic, date math, serialization, validation)
cd src-tauri && cargo test --lib

# 2. UI tests (48 tests — runs app.js in a Node.js vm sandbox with mocked
#    DOM, Tauri API, and AudioContext; no npm install needed, Node 18+)
node ui/test/test.js
```

**What the UI tests cover:** `formatTime` / `formatDailyTotal` / `phaseClass` (pure functions), `render` (DOM updates for running/paused/overtime/docked states), `buildSettingsForm` (form generation and two-way data binding), client-side settings validation, session-switcher wrap-around logic, keyboard-shortcut guards, and event-listener registration for `timer-tick` and `dock-mode-changed`.

**What they don't cover:** No actual Tauri IPC — `invoke()` calls are stubbed. No browser rendering or CSS layout. Timer-thread behavior, file I/O, and window management are tested only by the Rust unit tests.

### E2E tests (local only — not in CI)

End-to-end tests that drive a real Tauri app process via WebDriver (`tauri-driver` +
`msedgedriver`). The app is built with `POMODORO_TEST_MODE=1` so minutes become seconds
(25 s work, 5 s break), making time-based tests fast.

**Prerequisites (one-time):**

```bash
cargo install tauri-driver --locked
cargo install --git https://github.com/chippers/msedgedriver-tool --locked
msedgedriver-tool --install
```

**Run:**

```bash
./test-e2e.sh
```

**What they cover (7 tests):** start/stop/tick-down, non-extendable part auto-advance,
extendable-part overtime + Continue button, stop-records-work-time, settings panel
open/edit/save, session switcher, dock mode toggle.

**Architecture:** `src-tauri/tests/e2e/` — a minimal WebDriver client (`webdriver.rs`)
using only `ureq` + `serde_json` (no async, no tokio, no npm). Each test spawns a fresh
app instance via `WebDriverClient::new_session()` and cleans up with `delete_session()`.

**Why not in CI:** Removed from CI in commit `1cb5be6` because GitHub Actions Windows
runners have issues with `msedgedriver` + WebView2 in headless mode. Run locally before
releases.

## Architecture

Pomodoro desktop clock built with Rust + Tauri v2. Vanilla HTML/CSS/JS frontend (no framework, no npm).

**Timer state lives entirely in Rust.** The frontend is a dumb renderer: it calls `invoke()` to send commands and listens to `timer-tick` events to update the DOM. No time computation happens in JS.

### Backend (`src-tauri/src/`)

| File             | Role                                                                                                                                 |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `main.rs`        | Desktop entry point, hides console window in release                                                                                 |
| `lib.rs`         | App builder: loads persisted settings in `setup`, manages `Mutex<PomodoroState>`, registers commands. Also calls `notification::init` on startup |
| `timer.rs`       | State structs, Tauri commands, background timer thread. Calls `notification::notify_service()` at state transitions (start/stop/part-change/pause) |
| `notification.rs`| Android JNI bridge: captures `JavaVM` via `JNI_OnLoad`, provides `notify_service()` called from `timer.rs` at state transitions, and exports `nativeStopTimer` for the notification Stop button |

**State model** (`PomodoroState`): `active_session_id` (UUID string), `current_part_index`, `remaining_seconds` (i64 — negative during overtime), `settings` (PomodoroSettings), `running` flag, `paused` flag (overtime waiting for user), `overtime_tracked_seconds`, `is_docked` flag (window is in compact always-on-top mode). Wrapped in `Mutex<PomodoroState>` managed by Tauri.

**Data model:** Each `Session` has a stable `id` (UUID v4), a `name`, and a list of `SessionPart`s. Each part has an optional `name` (falls back to "Part N"), `minutes` (1–120), `extendable` (bool — when true, the timer enters paused overtime at 0 instead of auto-advancing), and `track_time` (bool — when true, seconds spent on this part are recorded to the daily log). Sessions are identified by UUID everywhere (not array index). Settings are persisted as JSON next to the executable (`pomodoro.json`). Detailed time records are tracked in `pomodoro_record.json` (date → session UUID → part index → accumulated seconds).

**Commands:**

- `get_state` — returns `TimerTick` snapshot (includes `active_session_id`, `part_index`)
- `get_daily_total` — returns today's total tracked seconds (sum across all sessions/parts in record)
- `get_settings` — returns current `PomodoroSettings`
- `start_timer` — sets `running=true`, spawns `std::thread` that ticks every 1s. Emits an initial tick immediately for instant UI feedback. When a non-extendable part hits 0 → auto-advances to next part. When an extendable part hits 0 → enters `paused` overtime (keeps ticking into negative). When the last part finishes → stops and resets. Records full part duration on auto-complete when `track_time` is enabled.
- `stop_timer` — sets `running=false`, records partial tracked time (handles overtime correctly by only adding overtime seconds since the full duration was already recorded at the zero-transition), resets to first part, emits final tick
- `continue_timer` — advances past an extendable part that is in overtime. Flushes accumulated overtime tracked seconds to the record. Errors if not paused.
- `update_settings` — validates (1–5 sessions, 1–10 parts each, 1–120 min), generates UUIDs for sessions without one, persists to JSON file, resets display if not running. Falls back to first session if active UUID no longer exists.
- `switch_session` — switches active session by UUID (only when stopped)
- `toggle_dock_mode` — toggles dock mode. Sets window to 360×72, always-on-top, undecorated, positioned at top-center of the primary monitor. Undocking restores 420×520 centered window with decorations. Emits `dock-mode-changed` event.
- `get_dock_state` — returns current `is_docked` boolean

**Why `std::thread::spawn` instead of `tokio::spawn`:** Tauri v2 doesn't guarantee a Tokio runtime is active in command handlers. `tokio::spawn` panics without one. A plain OS thread with `std::thread::sleep` is simpler and always works.

**`--exclude-all-symbols` linker flag:** The GNU toolchain fails on debug `cdylib` builds with "export ordinal too large" (92k+ exports). This flag in `.cargo/config.toml` is required for debug builds with the `x86_64-pc-windows-gnu` target.

### Frontend (`ui/`)

- `index.html` — timer display (`#timer`), phase indicator (`#phase`), session label (`#session-label`), dock button (`#dock-btn`), settings button (`#settings-btn`) both wrapped in `#controls` container, toggle button (`#toggle-btn`), continue button (`#continue-btn`, shown during overtime), session switcher arrows, settings panel overlay
- `style.css` — dark theme (`#1a1a2e` bg), centered flexbox, `.phase-part-0` through `.phase-part-4` (5-colour index-based palette wrapping via modulo), `.overtime` turns timer red, Continue button (teal outline → solid on hover). `body.docked` class switches to compact horizontal layout (72px tall bar, larger fonts, most controls hidden). Settings form: `.part-name-col` stacks name input above `.checkbox-row` (Ext + Track toggles).
- `app.js` — uses `window.__TAURI__` (global Tauri API, enabled via `withGlobalTauri: true`). Calls `invoke()` for commands, `listen('timer-tick', ...)` and `listen('dock-mode-changed', ...)` for state updates. Tracks `activeSessionId`/`sessionIds` (ordered UUID list for prev/next navigation), `currentPartIndex`/`currentPartName`/`currentSessionName`/`isRunning`/`isPaused`/`isDocked` locally. `formatTime` handles negative seconds (overtime). `phaseClass(partIndex)` — index-based with 5-colour modulo. Beeps on session start (long), part transitions (triple), session end (long), and overtime entry (triple). Dynamic settings form builds session/part cards with extendable + track-time checkboxes under the part name input. `setDocked()` toggles the `.docked` CSS class and button icon.

### Data flow

```
User clicks Start → invoke('start_timer') → Rust sets running=true, emits initial tick immediately
  → thread spawns: sleep 1s → lock state → decrement → check phase switch → unlock → emit('timer-tick', tick)
Frontend listen('timer-tick'): render(tick) → update DOM (part name/index, phase colour, button state)
User clicks Stop → invoke('stop_timer') → Rust sets running=false, records tracked time, resets, emits final tick
Session switch → invoke('switch_session', { sessionId }) → Rust resolves UUID→index, updates active_session_id
```

**Extendable parts (overtime):**

```
Extendable part hits 0 → paused=true, timer keeps ticking into negative
  → frontend shows negative time (red), Continue button appears, triple-beep alert
User clicks Continue → invoke('continue_timer')
  → advances to next part (or stops if last), flushes tracked overtime to record
User clicks Stop during overtime → records only the overtime seconds (full duration was
  already recorded at the zero-transition)
```

**Time recording:** A part's `track_time` flag controls whether time is recorded.
Completed durations and overtime are written to `pomodoro_record.json` keyed by
date → session UUID → part index. The daily total sums across all sessions/parts.

### Android notification (`src-tauri/gen/android/`)

While the timer is running, a persistent foreground-service notification shows the
countdown and part name. Architecture:

```
Rust timer thread → emit tick to JS (existing)
                  → notify_service() at transitions (start/stop/part-change/pause)
                       → JNI → TimerForegroundService companion fields
                            → Kotlin Handler countdown loop (wall-clock based)
                                 → NotificationManager.notify()

Stop button → PendingIntent → StopTimerReceiver → JNI (nativeStopTimer)
                                                    → Rust stop_timer logic
```

**Rust side** (`notification.rs`):

- `ServiceEvent` enum: `Start { tick }`, `PartUpdated { tick }`, `Stop`
- `notify_service()` — called from `timer.rs` at 5 transition points. On Android calls into Kotlin via JNI; completely no-op on desktop.
- `JNI_OnLoad` — captures `JavaVM` at library load time (reliable on Android, unlike `ndk_context` or `JNI_GetCreatedJavaVMs`).
- `Java_com_pomodoro_app_StopTimerReceiver_nativeStopTimer` — JNI export that runs full `stop_timer` logic (state reset + record + emit + notify_service(Stop)).

**Timer transition points** (`timer.rs`):

| Location | Event |
|----------|-------|
| `start_timer` — after initial tick emit | `Start { tick }` |
| `stop_timer` — after state reset + final tick | `Stop` |
| `continue_timer` — after advancing state | `PartUpdated { tick }` or `Stop` |
| Thread loop — part auto-advances | `PartUpdated { tick }` |
| Thread loop — extendable part enters pause | `PartUpdated { tick }` |
| Thread loop — last part finishes | `Stop` |

**Kotlin side** (`gen/android/app/…/`):

| File | Role |
|------|------|
| `TimerForegroundService.kt` | Foreground service. Countdown uses `SystemClock.elapsedRealtime()` wall-clock — immune to `Handler.postDelayed` drift. Companion `start()`/`update()`/`stop()` called from Rust via JNI. Rebuilds notification on each tick and on `update()` syncs from Rust. |
| `StopTimerReceiver.kt` | `BroadcastReceiver` wired to notification Stop action. Calls `nativeStopTimer()` (JNI → Rust). |
| `App.kt` | Singleton holding a global `Application` context so JNI companion methods (which have no `Context`) can start the foreground service. Initialised from `MainActivity.onCreate`. |
| `MainActivity.kt` | `App.init(this)` before `super.onCreate()`. Requests `POST_NOTIFICATIONS` runtime permission on Android 13+. Sets up `WebViewAssetLoader` in `onWebViewCreate` so the WebView loads frontend from APK assets via `https://tauri.localhost`. |
| `AndroidManifest.xml` | `FOREGROUND_SERVICE` + `POST_NOTIFICATIONS` permissions. `TimerForegroundService` (foregroundServiceType="dataSync") + `StopTimerReceiver` declarations. |

**Countdown accuracy:** The Kotlin tick loop reads `SystemClock.elapsedRealtime()` (monotonic
milliseconds since boot) and computes `remainingSeconds = startRemainingSeconds - elapsed`.
On part transitions Rust calls `update()` which resets the wall-clock anchor. This keeps
the notification within ±0.5 s of the Rust timer without per-second JNI calls.

**Android builds use `cargo tauri android build --debug`** — the Tauri CLI sets up
NDK toolchains, cross-compiles Rust, symlinks the `.so` into `jniLibs/`, and runs
Gradle. Never manually `cargo build --target aarch64-linux-android` — the Tauri
pipeline passes the correct NDK linker flags that a bare `cargo build` misses.

**Generated files** — `.gradle/`, `assets/`, `jniLibs/`, `generated/` Kotlin, and
`build/reports/` are excluded from git via `.gitignore`. They are regenerated on
every build.

## Releasing

```bash
# 1. Bump version in src-tauri/Cargo.toml (e.g., version = "0.3.1")
# 2. Rebuild to update Cargo.lock with the new version
export PATH="/c/msys64/mingw64/bin:$PATH"
cd src-tauri && cargo build --release

# 3. Commit the version bump (Cargo.toml + Cargo.lock)
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "Bump version to X.Y.Z"
git push

# 4. Tag using the script (reads version from Cargo.toml)
./tag-release.sh --push
```

The `tag-release.sh` script reads the version from `src-tauri/Cargo.toml` and creates an annotated `v<version>` tag. Pushing the tag triggers `.github/workflows/release.yml` which builds and packages the release.

**Important:** After changing the version in `Cargo.toml`, always rebuild so that `Cargo.lock` reflects the new version — otherwise the lockfile will be out of sync.

## Toolchain quirk

This project uses the **`stable-x86_64-pc-windows-gnu`** Rust toolchain (not MSVC). MSYS2 MinGW-w64 at `C:\msys64\mingw64\bin` provides the linker. If `dlltool.exe` or `gcc.exe` isn't found, ensure that directory is on PATH.
