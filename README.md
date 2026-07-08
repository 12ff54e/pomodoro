# Pomodoro

A simple Pomodoro clock built with Rust and Tauri v2. No framework, no npm — just a lightweight desktop timer.

## Features

- **Work / Break timer** — alternates between configurable work and break phases
- **Sound alerts** — beeps when a phase ends (Web Audio API)
- **Daily tracking** — records total work time per day alongside the executable
- **Persistent settings** — work/break durations saved to a JSON file next to the app
- **Small binary** — ~2.7 MB standalone executable

## Build

Requires Rust with the `stable-x86_64-pc-windows-gnu` target and MSYS2 MinGW-w64 at `C:\msys64\mingw64\bin`.

```bash
# Debug build
./build.sh

# Release build + zip package
./build.sh --release
```

The release zip is created at `pomodoro-v0.1.0.zip`.

## Usage

| Action | |
|---|---|
| **Start** | Begins the work timer |
| **Stop** | Halts the timer and resets to work phase |
| **Settings (gear icon)** | Adjust work and break durations (1–120 min) |

- When work ends → auto-switches to break, 3 high beeps
- When break ends → timer stops, resets to work, 1 low beep
- Partial work sessions (stopped early) are recorded in the daily total

## How it works

Timer state lives entirely in the Rust backend. A background thread ticks every second and emits events to the frontend. The frontend (vanilla HTML/CSS/JS) is a dumb renderer that listens for `timer-tick` events and updates the DOM.

```
User clicks Start → Rust spawns tick thread
  ↓ each second
Thread locks state → decrements timer → emits event → frontend renders
  ↓ on phase change
Work done → switch to Break (beep 3×)  |  Break done → stop & reset (beep 1×)
```

Settings and daily totals are stored as JSON files next to the executable:
- `pomodoro.json` — work/break minutes
- `pomodoro_daily.json` — work time per day

## License

MIT
