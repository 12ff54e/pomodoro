# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
# MinGW toolchain must be on PATH (MSYS2 at C:\msys64\mingw64\bin)
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

## Architecture

Pomodoro desktop clock built with Rust + Tauri v2. Vanilla HTML/CSS/JS frontend (no framework, no npm).

**Timer state lives entirely in Rust.** The frontend is a dumb renderer: it calls `invoke()` to send commands and listens to `timer-tick` events to update the DOM. No time computation happens in JS.

### Backend (`src-tauri/src/`)

| File | Role |
|---|---|
| `main.rs` | Desktop entry point, hides console window in release |
| `lib.rs` | App builder: registers `tauri-plugin-store`, loads persisted settings in `setup`, manages `Mutex<PomodoroState>`, registers commands |
| `timer.rs` | State structs, 4 Tauri commands, background timer thread |

**State model** (`PomodoroState`): `active_session_index`, `current_part_index`, `remaining_seconds` (i64 — negative during overtime), `settings` (PomodoroSettings), `running` flag, `paused` flag (overtime waiting for user), `overtime_work_seconds`. Wrapped in `Mutex<PomodoroState>` managed by Tauri.

**Data model:** Each `Session` has a name and a list of `SessionPart`s. Each part has a `name`, `minutes` (1–120), and `extendable` (bool — when true, the timer enters paused overtime at 0 instead of auto-advancing). Settings are persisted as JSON next to the executable (`pomodoro.json`). Daily work totals are tracked separately in `pomodoro_daily.json`.

**Commands:**
- `get_state` — returns `TimerTick` snapshot
- `get_daily_total` — returns today's total work seconds
- `get_settings` — returns current `PomodoroSettings`
- `start_timer` — sets `running=true`, spawns `std::thread` that ticks every 1s. When a non-extendable part hits 0 → auto-advances to next part. When an extendable part hits 0 → enters `paused` overtime (keeps ticking into negative). When the last part finishes → stops and resets.
- `stop_timer` — sets `running=false`, records partial work time (handles overtime correctly by only adding overtime seconds since the full duration was already recorded at the zero-transition), resets to first part, emits final tick
- `continue_timer` — advances past an extendable part that is in overtime. Flushes accumulated overtime work seconds to the daily total. Errors if not paused.
- `update_settings` — validates (1–5 sessions, 1–10 parts each, 1–120 min), persists to JSON file, resets display if not running
- `switch_session` — switches active session (only when stopped)

**Why `std::thread::spawn` instead of `tokio::spawn`:** Tauri v2 doesn't guarantee a Tokio runtime is active in command handlers. `tokio::spawn` panics without one. A plain OS thread with `std::thread::sleep` is simpler and always works.

**`--exclude-all-symbols` linker flag:** The GNU toolchain fails on debug `cdylib` builds with "export ordinal too large" (92k+ exports). This flag in `.cargo/config.toml` is required for debug builds with the `x86_64-pc-windows-gnu` target.

### Frontend (`ui/`)

- `index.html` — timer display (`#timer`), phase indicator (`#phase`), session label (`#session-label`), toggle button (`#toggle-btn`), continue button (`#continue-btn`, shown during overtime), session switcher arrows, settings panel overlay
- `style.css` — dark theme (`#1a1a2e` bg), centered flexbox, `.phase-work` (red-orange) / `.phase-break` (teal-green) / `.phase-play` (cornflower blue), `.overtime` turns timer red, Continue button (teal outline → solid on hover)
- `app.js` — uses `window.__TAURI__` (global Tauri API, enabled via `withGlobalTauri: true`). Calls `invoke()` for commands, `listen('timer-tick', ...)` for state updates. Tracks `currentPartName`/`currentSessionName`/`isRunning`/`isPaused` locally. `formatTime` handles negative seconds (overtime). Beeps on part transitions and overtime entry. Dynamic settings form builds session/part cards with extendable checkboxes.

### Data flow

```
User clicks Start → invoke('start_timer') → Rust sets running=true, spawns thread
Thread each second: lock state → decrement → check phase switch → unlock → emit('timer-tick', tick)
Frontend listen('timer-tick'): render(tick) → update DOM
User clicks Stop → invoke('stop_timer') → Rust sets running=false, resets, emits final tick
```

**Extendable parts (overtime):**
```
Extendable part hits 0 → paused=true, timer keeps ticking into negative
  → frontend shows negative time (red), Continue button appears, triple-beep alert
User clicks Continue → invoke('continue_timer')
  → advances to next part (or stops if last), flushes overtime to daily total
User clicks Stop during overtime → records only the overtime seconds (full duration was
  already recorded at the zero-transition)
```

## Toolchain quirk

This project uses the **`stable-x86_64-pc-windows-gnu`** Rust toolchain (not MSVC). MSYS2 MinGW-w64 at `C:\msys64\mingw64\bin` provides the linker. If `dlltool.exe` or `gcc.exe` isn't found, ensure that directory is on PATH.
