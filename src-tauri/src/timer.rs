use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::SystemTime;
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
    pub daily_total_seconds: u64,
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
// Helpers
// ---------------------------------------------------------------------------

/// Returns today's date as "YYYY-MM-DD" using the system clock.
fn today_string() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Days since 1970-01-01.
    let days = (secs / 86400) as i64;

    // Convert days since epoch to Gregorian date (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Reads (or creates) the daily-totals map from the store and returns
/// today's total seconds.
fn load_daily_total(app: &AppHandle) -> u64 {
    if let Ok(store) = app.store("settings.json") {
        let totals = store
            .get("dailyTotals")
            .and_then(|v| {
                if let serde_json::Value::Object(map) = v {
                    Some(map)
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let today = today_string();
        totals
            .get(&today)
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    } else {
        0
    }
}

/// Adds `seconds` of work time to today's entry in the persisted store.
/// Returns the new daily total.
fn add_daily_work_seconds(app: &AppHandle, seconds: u64) -> u64 {
    if let Ok(store) = app.store("settings.json") {
        let mut totals = store
            .get("dailyTotals")
            .and_then(|v| {
                if let serde_json::Value::Object(map) = v {
                    Some(map)
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let today = today_string();
        let prev = totals
            .get(&today)
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let new_total = prev + seconds;
        totals.insert(today, serde_json::Value::Number(new_total.into()));
        let _ = store.set("dailyTotals", serde_json::Value::Object(totals));
        let _ = store.save();
        new_total
    } else {
        0
    }
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
        daily_total_seconds: 0, // caller should use get_daily_total for this
    }
}

#[tauri::command]
pub fn get_daily_total(app: AppHandle) -> u64 {
    load_daily_total(&app)
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
                        // Work done -> add full session, switch to break.
                        TimerPhase::Work => {
                            let full_work = settings.work_minutes * 60;
                            s.phase = TimerPhase::Break;
                            s.remaining_seconds = settings.break_minutes * 60;
                            drop(s);
                            let _total = add_daily_work_seconds(&app, full_work);
                        }
                        // Break done -> stop and reset.
                        TimerPhase::Break => {
                            s.running = false;
                            s.phase = TimerPhase::Work;
                            s.remaining_seconds = settings.work_minutes * 60;
                        }
                    }
                }

                // Re-lock if we dropped it above.
                let state = app.state::<Mutex<PomodoroState>>();
                let s = state.lock().unwrap();
                let daily = load_daily_total(&app);
                TimerTick {
                    remaining_seconds: s.remaining_seconds,
                    phase: s.phase,
                    running: s.running,
                    daily_total_seconds: daily,
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

    // Record partial work time.
    if s.phase == TimerPhase::Work {
        let full = s.settings.work_minutes * 60;
        let elapsed = full.saturating_sub(s.remaining_seconds);
        if elapsed > 0 {
            drop(s);
            add_daily_work_seconds(&app, elapsed);
            // Re-lock.
            s = state.lock().unwrap();
        }
    }

    s.running = false;
    s.phase = TimerPhase::Work;
    s.remaining_seconds = s.settings.work_minutes * 60;

    let daily = load_daily_total(&app);
    let tick = TimerTick {
        remaining_seconds: s.remaining_seconds,
        phase: s.phase,
        running: false,
        daily_total_seconds: daily,
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

    let daily = load_daily_total(&app);
    let tick = TimerTick {
        remaining_seconds: s.remaining_seconds,
        phase: s.phase,
        running: s.running,
        daily_total_seconds: daily,
    };
    drop(s);

    if !tick.running {
        let _ = app.emit("timer-tick", &tick);
    }
    Ok(new_settings)
}
