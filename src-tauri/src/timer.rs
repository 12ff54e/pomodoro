use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionType {
    Pomodoro,
    PlayBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerPhase {
    Work,
    Break,
    Play,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerTick {
    pub remaining_seconds: u64,
    pub phase: TimerPhase,
    pub running: bool,
    pub daily_total_seconds: u64,
    pub session_type: SessionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroSettings {
    pub work_minutes: u64,
    pub break_minutes: u64,
    pub play_minutes: u64,
    pub play_break_minutes: u64,
}

impl Default for PomodoroSettings {
    fn default() -> Self {
        Self {
            work_minutes: 25,
            break_minutes: 5,
            play_minutes: 25,
            play_break_minutes: 5,
        }
    }
}

#[derive(Debug)]
pub struct PomodoroState {
    pub session_type: SessionType,
    pub phase: TimerPhase,
    pub remaining_seconds: u64,
    pub settings: PomodoroSettings,
    pub running: bool,
}

// ---------------------------------------------------------------------------
// Persistence (JSON file next to the executable)
// ---------------------------------------------------------------------------

fn exe_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

/// `<exe-dir>/pomodoro.json` — small, fast to load.
fn settings_path() -> PathBuf {
    exe_dir().join("pomodoro.json")
}

/// `<exe-dir>/pomodoro_daily.json` — grows over time, loaded separately.
fn daily_path() -> PathBuf {
    exe_dir().join("pomodoro_daily.json")
}

// ---- Settings file (tiny, always loaded as a whole) ----

#[derive(Debug, Serialize, Deserialize)]
struct SettingsFile {
    #[serde(default = "default_work")]
    #[serde(rename = "workMinutes")]
    work_minutes: u64,
    #[serde(default = "default_break")]
    #[serde(rename = "breakMinutes")]
    break_minutes: u64,
    #[serde(default = "default_play")]
    #[serde(rename = "playMinutes")]
    play_minutes: u64,
    #[serde(default = "default_play_break")]
    #[serde(rename = "playBreakMinutes")]
    play_break_minutes: u64,
    #[serde(default = "default_session_type")]
    #[serde(rename = "sessionType")]
    session_type: String,
}

fn default_work() -> u64 {
    25
}
fn default_break() -> u64 {
    5
}
fn default_play() -> u64 {
    25
}
fn default_play_break() -> u64 {
    5
}
fn default_session_type() -> String {
    "pomodoro".into()
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            work_minutes: 25,
            break_minutes: 5,
            play_minutes: 25,
            play_break_minutes: 5,
            session_type: "pomodoro".into(),
        }
    }
}

fn load_settings_file() -> SettingsFile {
    match std::fs::read_to_string(settings_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => SettingsFile::default(),
    }
}

fn save_settings_file(settings: &SettingsFile) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(settings_path(), json);
    }
}

// ---- Daily totals file (separate, can grow large) ----

fn load_daily_totals() -> BTreeMap<String, u64> {
    match std::fs::read_to_string(daily_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    }
}

fn save_daily_totals(totals: &BTreeMap<String, u64>) {
    if let Ok(json) = serde_json::to_string_pretty(totals) {
        let _ = std::fs::write(daily_path(), json);
    }
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

/// Load settings (work/break/play minutes) from the settings file.
pub fn load_settings() -> PomodoroSettings {
    let file = load_settings_file();
    PomodoroSettings {
        work_minutes: file.work_minutes,
        break_minutes: file.break_minutes,
        play_minutes: file.play_minutes,
        play_break_minutes: file.play_break_minutes,
    }
}

/// Load session type from the settings file.
pub fn load_session_type() -> SessionType {
    let file = load_settings_file();
    match file.session_type.as_str() {
        "playBreak" | "playbreak" => SessionType::PlayBreak,
        _ => SessionType::Pomodoro,
    }
}

/// Reads today's total work seconds from the daily file.
pub fn load_daily_total() -> u64 {
    let totals = load_daily_totals();
    let today = today_string();
    totals.get(&today).copied().unwrap_or(0)
}

/// Adds `seconds` of work time to today's entry in the daily file.
/// Returns the new daily total.
fn add_daily_work_seconds(seconds: u64) -> u64 {
    let mut totals = load_daily_totals();
    let today = today_string();
    let prev = totals.get(&today).copied().unwrap_or(0);
    let new_total = prev + seconds;
    totals.insert(today, new_total);
    save_daily_totals(&totals);
    new_total
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
        session_type: s.session_type,
    }
}

#[tauri::command]
pub fn get_daily_total() -> u64 {
    load_daily_total()
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
    let session_type = s.session_type;
    drop(s); // release lock before spawning

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));

            // Work that must be done AFTER releasing the state lock.
            let mut work_completed: Option<u64> = None;

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
                        TimerPhase::Work => {
                            work_completed = Some(settings.work_minutes * 60);
                            s.phase = TimerPhase::Break;
                            s.remaining_seconds = settings.break_minutes * 60;
                        }
                        TimerPhase::Play => {
                            s.phase = TimerPhase::Break;
                            s.remaining_seconds = settings.play_break_minutes * 60;
                        }
                        TimerPhase::Break => {
                            s.running = false;
                            match session_type {
                                SessionType::Pomodoro => {
                                    s.phase = TimerPhase::Work;
                                    s.remaining_seconds = settings.work_minutes * 60;
                                }
                                SessionType::PlayBreak => {
                                    s.phase = TimerPhase::Play;
                                    s.remaining_seconds = settings.play_minutes * 60;
                                }
                            }
                        }
                    }
                }
                let tick = TimerTick {
                    remaining_seconds: s.remaining_seconds,
                    phase: s.phase,
                    running: s.running,
                    daily_total_seconds: 0, // filled in below
                    session_type: s.session_type,
                };
                drop(s);
                tick
            }; // state lock released here

            // Record completed work (outside the lock to avoid deadlock).
            if let Some(secs) = work_completed {
                add_daily_work_seconds(secs);
            }

            // Attach fresh daily total and emit.
            let daily = load_daily_total();
            let tick = TimerTick {
                daily_total_seconds: daily,
                ..tick
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
            add_daily_work_seconds(elapsed);
            // Re-lock.
            s = state.lock().unwrap();
        }
    }

    s.running = false;
    // Reset to the starting phase for the current session type.
    match s.session_type {
        SessionType::Pomodoro => {
            s.phase = TimerPhase::Work;
            s.remaining_seconds = s.settings.work_minutes * 60;
        }
        SessionType::PlayBreak => {
            s.phase = TimerPhase::Play;
            s.remaining_seconds = s.settings.play_minutes * 60;
        }
    }

    let daily = load_daily_total();
    let tick = TimerTick {
        remaining_seconds: s.remaining_seconds,
        phase: s.phase,
        running: false,
        daily_total_seconds: daily,
        session_type: s.session_type,
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
    play_minutes: u64,
    play_break_minutes: u64,
) -> Result<PomodoroSettings, String> {
    if work_minutes < 1 || work_minutes > 120 {
        return Err("Work minutes must be between 1 and 120".into());
    }
    if break_minutes < 1 || break_minutes > 120 {
        return Err("Break minutes must be between 1 and 120".into());
    }
    if play_minutes < 1 || play_minutes > 120 {
        return Err("Play minutes must be between 1 and 120".into());
    }
    if play_break_minutes < 1 || play_break_minutes > 120 {
        return Err("Play-break minutes must be between 1 and 120".into());
    }

    let new_settings = PomodoroSettings {
        work_minutes,
        break_minutes,
        play_minutes,
        play_break_minutes,
    };

    // Persist to disk (alongside the exe).
    let mut file = load_settings_file();
    file.work_minutes = work_minutes;
    file.break_minutes = break_minutes;
    file.play_minutes = play_minutes;
    file.play_break_minutes = play_break_minutes;
    save_settings_file(&file);

    let mut s = state.lock().unwrap();
    s.settings = new_settings.clone();

    // If the timer is stopped, reset the display for the current session type.
    if !s.running {
        match s.session_type {
            SessionType::Pomodoro => {
                s.phase = TimerPhase::Work;
                s.remaining_seconds = new_settings.work_minutes * 60;
            }
            SessionType::PlayBreak => {
                s.phase = TimerPhase::Play;
                s.remaining_seconds = new_settings.play_minutes * 60;
            }
        }
    }

    let daily = load_daily_total();
    let tick = TimerTick {
        remaining_seconds: s.remaining_seconds,
        phase: s.phase,
        running: s.running,
        daily_total_seconds: daily,
        session_type: s.session_type,
    };
    drop(s);

    if !tick.running {
        let _ = app.emit("timer-tick", &tick);
    }
    Ok(new_settings)
}

#[tauri::command]
pub fn switch_session(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
    session_type: SessionType,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    if s.running {
        return Err("Cannot switch session while timer is running".into());
    }

    s.session_type = session_type;

    // Reset to the starting phase for the chosen session.
    match session_type {
        SessionType::Pomodoro => {
            s.phase = TimerPhase::Work;
            s.remaining_seconds = s.settings.work_minutes * 60;
        }
        SessionType::PlayBreak => {
            s.phase = TimerPhase::Play;
            s.remaining_seconds = s.settings.play_minutes * 60;
        }
    }

    // Persist session type to settings file.
    let mut file = load_settings_file();
    file.session_type = match session_type {
        SessionType::Pomodoro => "pomodoro".into(),
        SessionType::PlayBreak => "playbreak".into(),
    };
    save_settings_file(&file);

    let daily = load_daily_total();
    let tick = TimerTick {
        remaining_seconds: s.remaining_seconds,
        phase: s.phase,
        running: false,
        daily_total_seconds: daily,
        session_type: s.session_type,
    };
    drop(s);

    let _ = app.emit("timer-tick", &tick);
    Ok(())
}
