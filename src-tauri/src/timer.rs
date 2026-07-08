use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_store::StoreExt;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerPhase {
    Work,
    Break,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerTick {
    pub remaining_seconds: u64,
    pub phase: TimerPhase,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroSettings {
    pub work_minutes: u64,
    pub break_minutes: u64,
}

impl Default for PomodoroSettings {
    fn default() -> Self {
        Self {
            work_minutes: 25,
            break_minutes: 5,
        }
    }
}

#[derive(Debug)]
pub struct PomodoroState {
    pub phase: TimerPhase,
    pub remaining_seconds: u64,
    pub settings: PomodoroSettings,
    pub running: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_state(state: State<'_, Mutex<PomodoroState>>) -> TimerTick {
    let s = state.lock().unwrap();
    TimerTick {
        remaining_seconds: s.remaining_seconds,
        phase: s.phase,
        running: s.running,
    }
}

#[tauri::command]
pub fn start_timer(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    if s.running {
        return Err("Timer is already running".into());
    }
    s.running = true;
    let settings = s.settings.clone();
    drop(s); // release lock before spawning

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));

            let tick = {
                let state = app.state::<Mutex<PomodoroState>>();
                let mut s = state.lock().unwrap();
                if !s.running {
                    break;
                }
                if s.remaining_seconds > 0 {
                    s.remaining_seconds -= 1;
                }
                if s.remaining_seconds == 0 {
                    match s.phase {
                        // Work done → switch to break and keep going.
                        TimerPhase::Work => {
                            s.phase = TimerPhase::Break;
                            s.remaining_seconds = settings.break_minutes * 60;
                        }
                        // Break done → stop and reset to initial work state.
                        TimerPhase::Break => {
                            s.running = false;
                            s.phase = TimerPhase::Work;
                            s.remaining_seconds = settings.work_minutes * 60;
                        }
                    }
                }
                TimerTick {
                    remaining_seconds: s.remaining_seconds,
                    phase: s.phase,
                    running: s.running,
                }
            };

            let _ = app.emit("timer-tick", &tick);
        }
    });

    Ok(())
}

#[tauri::command]
pub fn stop_timer(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    if !s.running {
        return Err("Timer is not running".into());
    }
    s.running = false;
    s.phase = TimerPhase::Work;
    s.remaining_seconds = s.settings.work_minutes * 60;

    let tick = TimerTick {
        remaining_seconds: s.remaining_seconds,
        phase: s.phase,
        running: false,
    };
    drop(s);

    let _ = app.emit("timer-tick", &tick);
    Ok(())
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
    work_minutes: u64,
    break_minutes: u64,
) -> Result<PomodoroSettings, String> {
    if work_minutes < 1 || work_minutes > 120 {
        return Err("Work minutes must be between 1 and 120".into());
    }
    if break_minutes < 1 || break_minutes > 120 {
        return Err("Break minutes must be between 1 and 120".into());
    }

    let new_settings = PomodoroSettings {
        work_minutes,
        break_minutes,
    };

    // Persist to disk.
    if let Ok(store) = app.store("settings.json") {
        let _ = store.set(
            "workMinutes",
            serde_json::Value::Number(work_minutes.into()),
        );
        let _ = store.set(
            "breakMinutes",
            serde_json::Value::Number(break_minutes.into()),
        );
        let _ = store.save();
    }

    let mut s = state.lock().unwrap();
    s.settings = new_settings.clone();

    // If the timer is stopped, reset the display for the new work duration.
    if !s.running {
        s.phase = TimerPhase::Work;
        s.remaining_seconds = new_settings.work_minutes * 60;
    }

    let tick = TimerTick {
        remaining_seconds: s.remaining_seconds,
        phase: s.phase,
        running: s.running,
    };
    drop(s);

    if !tick.running {
        let _ = app.emit("timer-tick", &tick);
    }
    Ok(new_settings)
}
