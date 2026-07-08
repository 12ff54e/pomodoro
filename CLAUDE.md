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

**State model** (`PomodoroState`): `phase` (Work/Break), `remaining_seconds`, `settings` (PomodoroSettings), `running` flag. Wrapped in `Mutex<PomodoroState>` managed by Tauri.

**Commands:**
- `get_state` — returns `TimerTick` snapshot
- `start_timer` — sets `running=true`, spawns `std::thread` that ticks every 1s. When Work hits 0 → auto-switches to Break. When Break hits 0 → **stops** and resets to initial Work state.
- `stop_timer` — sets `running=false`, resets to full Work duration, emits final tick
- `update_settings` — validates (1–120 min), persists via `tauri-plugin-store`, resets display if not running

**Why `std::thread::spawn` instead of `tokio::spawn`:** Tauri v2 doesn't guarantee a Tokio runtime is active in command handlers. `tokio::spawn` panics without one. A plain OS thread with `std::thread::sleep` is simpler and always works.

**`--exclude-all-symbols` linker flag:** The GNU toolchain fails on debug `cdylib` builds with "export ordinal too large" (92k+ exports). This flag in `.cargo/config.toml` is required for debug builds with the `x86_64-pc-windows-gnu` target.

### Frontend (`ui/`)

- `index.html` — timer display (`#timer`), phase indicator (`#phase`), toggle button (`#toggle-btn`), settings panel overlay
- `style.css` — dark theme (`#1a1a2e` bg), centered flexbox, `.phase-work` (red-orange) / `.phase-break` (teal-green)
- `app.js` — uses `window.__TAURI__` (global Tauri API, enabled via `withGlobalTauri: true`). Calls `invoke()` for commands, `listen('timer-tick', ...)` for state updates. Tracks `currentPhase`/`isRunning` locally for toggle button logic.

### Data flow

```
User clicks Start → invoke('start_timer') → Rust sets running=true, spawns thread
Thread each second: lock state → decrement → check phase switch → unlock → emit('timer-tick', tick)
Frontend listen('timer-tick'): render(tick) → update DOM
User clicks Stop → invoke('stop_timer') → Rust sets running=false, resets, emits final tick
```

## Toolchain quirk

This project uses the **`stable-x86_64-pc-windows-gnu`** Rust toolchain (not MSVC). MSYS2 MinGW-w64 at `C:\msys64\mingw64\bin` provides the linker. If `dlltool.exe` or `gcc.exe` isn't found, ensure that directory is on PATH.
