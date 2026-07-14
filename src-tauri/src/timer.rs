use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPart {
    pub name: String,
    pub minutes: u64,
    #[serde(default)]
    pub extendable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub parts: Vec<SessionPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PomodoroSettings {
    pub sessions: Vec<Session>,
}

impl Default for PomodoroSettings {
    fn default() -> Self {
        Self {
            sessions: vec![Session {
                name: "Pomodoro".into(),
                parts: vec![
                    SessionPart {
                        name: "Work".into(),
                        minutes: 25,
                        extendable: false,
                    },
                    SessionPart {
                        name: "Break".into(),
                        minutes: 5,
                        extendable: false,
                    },
                ],
            }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerTick {
    pub remaining_seconds: i64,
    pub session_name: String,
    pub part_name: String,
    pub running: bool,
    pub paused: bool,
    pub daily_total_seconds: u64,
    pub active_session_index: usize,
    pub session_count: usize,
}

#[derive(Debug)]
pub struct PomodoroState {
    pub active_session_index: usize,
    pub current_part_index: usize,
    pub remaining_seconds: i64,
    pub settings: PomodoroSettings,
    pub running: bool,
    pub paused: bool,
    /// Accumulated overtime seconds for the current Work part (flushed on Continue/Stop).
    pub overtime_work_seconds: u64,
    /// Whether the window is in dock mode (small, always-on-top, docked to top of screen).
    pub is_docked: bool,
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

// ---- Settings file ----

#[derive(Debug, Serialize, Deserialize, Default)]
struct SettingsFile {
    #[serde(default)]
    sessions: Option<Vec<SessionDef>>,
    #[serde(default)]
    #[serde(rename = "activeSession")]
    active_session: Option<usize>,
    #[serde(default)]
    docked: Option<bool>,

    // Legacy fields for migration — only present in old-format files.
    #[serde(default)]
    #[serde(rename = "workMinutes")]
    work_minutes: Option<u64>,
    #[serde(default)]
    #[serde(rename = "breakMinutes")]
    break_minutes: Option<u64>,
    #[serde(default)]
    #[serde(rename = "playMinutes")]
    play_minutes: Option<u64>,
    #[serde(default)]
    #[serde(rename = "playBreakMinutes")]
    play_break_minutes: Option<u64>,
    #[serde(default)]
    #[serde(rename = "sessionType")]
    session_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionDef {
    name: String,
    parts: Vec<PartDef>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PartDef {
    name: String,
    minutes: u64,
    #[serde(default)]
    extendable: bool,
}

/// Build default sessions from legacy hardcoded sessions (migration path).
fn legacy_sessions(file: &SettingsFile) -> Vec<Session> {
    let wm = file.work_minutes.unwrap_or(25);
    let bm = file.break_minutes.unwrap_or(5);
    let pm = file.play_minutes.unwrap_or(25);
    let pbm = file.play_break_minutes.unwrap_or(5);

    vec![
        Session {
            name: "Pomodoro".into(),
            parts: vec![
                SessionPart {
                    name: "Work".into(),
                    minutes: wm,
                    extendable: false,
                },
                SessionPart {
                    name: "Break".into(),
                    minutes: bm,
                    extendable: false,
                },
            ],
        },
        Session {
            name: "Play / Break".into(),
            parts: vec![
                SessionPart {
                    name: "Play".into(),
                    minutes: pm,
                    extendable: false,
                },
                SessionPart {
                    name: "Break".into(),
                    minutes: pbm,
                    extendable: false,
                },
            ],
        },
    ]
}

fn load_settings_file() -> SettingsFile {
    match std::fs::read_to_string(settings_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => SettingsFile::default(),
    }
}

fn save_settings(sessions: &[Session], active_index: usize) {
    let defs: Vec<SessionDef> = sessions
        .iter()
        .map(|s| SessionDef {
            name: s.name.clone(),
            parts: s
                .parts
                .iter()
                .map(|p| PartDef {
                    name: p.name.clone(),
                    minutes: p.minutes,
                    extendable: p.extendable,
                })
                .collect(),
        })
        .collect();

    // Preserve dock state if it exists in the current file.
    let docked = load_settings_file().docked;
    let mut file = serde_json::json!({
        "sessions": defs,
        "activeSession": active_index,
    });
    if let Some(d) = docked {
        file["docked"] = serde_json::Value::Bool(d);
    }
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(settings_path(), json);
    }
}

/// Read the `docked` flag from the settings file (defaults to false).
pub fn load_dock_state() -> bool {
    load_settings_file().docked.unwrap_or(false)
}

/// Persist only the docked flag without touching other settings.
fn save_dock_state(docked: bool) {
    let path = settings_path();
    let mut file: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or(serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    };
    file["docked"] = serde_json::Value::Bool(docked);
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(path, json);
    }
}

// ---- Daily totals file ----

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

/// Load settings, migrating from the old format if necessary.
pub fn load_settings() -> (PomodoroSettings, usize) {
    let file = load_settings_file();

    if let Some(sessions) = file.sessions {
        // New format — convert SessionDef → Session.
        let sessions: Vec<Session> = sessions
            .into_iter()
            .map(|s| Session {
                name: s.name,
                parts: s
                    .parts
                    .into_iter()
                    .map(|p| SessionPart {
                        name: p.name,
                        minutes: p.minutes,
                        extendable: p.extendable,
                    })
                    .collect(),
            })
            .collect();
        let active = file.active_session.unwrap_or(0).min(sessions.len().saturating_sub(1));
        (PomodoroSettings { sessions }, active)
    } else if file.work_minutes.is_some() || file.play_minutes.is_some() {
        // Old format — migrate.
        let sessions = legacy_sessions(&file);
        let active = match file.session_type.as_deref() {
            Some("playBreak" | "playbreak") => 1,
            _ => 0,
        };
        // Save in new format right away.
        save_settings(&sessions, active);
        (PomodoroSettings { sessions }, active)
    } else {
        // No file yet — use defaults.
        let settings = PomodoroSettings::default();
        (settings, 0)
    }
}

/// Reads today's total work seconds from the daily file.
pub fn load_daily_total() -> u64 {
    let totals = load_daily_totals();
    let today = today_string();
    totals.get(&today).copied().unwrap_or(0)
}

/// Adds `seconds` of work time to today's entry in the daily file.
fn add_daily_work_seconds(seconds: u64) -> u64 {
    let mut totals = load_daily_totals();
    let today = today_string();
    let prev = totals.get(&today).copied().unwrap_or(0);
    let new_total = prev + seconds;
    totals.insert(today, new_total);
    save_daily_totals(&totals);
    new_total
}

/// Build a TimerTick from the current state.
fn build_tick(state: &PomodoroState, daily_total: u64) -> TimerTick {
    let session = &state.settings.sessions[state.active_session_index];
    let part = &session.parts[state.current_part_index];
    TimerTick {
        remaining_seconds: state.remaining_seconds,
        session_name: session.name.clone(),
        part_name: part.name.clone(),
        running: state.running,
        paused: state.paused,
        daily_total_seconds: daily_total,
        active_session_index: state.active_session_index,
        session_count: state.settings.sessions.len(),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_state(state: State<'_, Mutex<PomodoroState>>) -> TimerTick {
    let s = state.lock().unwrap();
    build_tick(&s, 0)
}

#[tauri::command]
pub fn get_daily_total() -> u64 {
    load_daily_total()
}

#[tauri::command]
pub fn get_settings(state: State<'_, Mutex<PomodoroState>>) -> PomodoroSettings {
    let s = state.lock().unwrap();
    s.settings.clone()
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
    // If paused, this acts as "continue" (legacy behaviour — start while paused
    // resumes without advancing).
    let sessions = s.settings.sessions.clone();
    let active_idx = s.active_session_index;
    s.running = true;
    s.paused = false;
    s.overtime_work_seconds = 0;
    drop(s);

    std::thread::spawn(move || {
        // Snapshot part metadata from the session at start time.
        let part_names: Vec<String> = sessions[active_idx]
            .parts.iter().map(|p| p.name.clone()).collect();
        let part_seconds: Vec<i64> = sessions[active_idx]
            .parts.iter().map(|p| (p.minutes * 60) as i64).collect();
        let part_extendable: Vec<bool> = sessions[active_idx]
            .parts.iter().map(|p| p.extendable).collect();

        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));

            let mut work_completed: Option<u64> = None;
            let mut phase_ended: bool = false;

            let tick = {
                let state = app.state::<Mutex<PomodoroState>>();
                let mut s = state.lock().unwrap();
                if !s.running {
                    break;
                }
                if s.remaining_seconds > 0 {
                    s.remaining_seconds -= 1;
                }
                if s.remaining_seconds == 0 && !s.paused {
                    // Timer just reached zero.
                    let idx = s.current_part_index;
                    if part_extendable[idx] {
                        // Extendable part — enter paused overtime mode.
                        s.paused = true;
                    } else if idx + 1 < part_seconds.len() {
                        // Advance to next part.
                        s.current_part_index += 1;
                        s.remaining_seconds = part_seconds[s.current_part_index];
                    } else {
                        // Last part finished — stop and reset.
                        s.running = false;
                        s.current_part_index = 0;
                        s.remaining_seconds = part_seconds[0];
                        phase_ended = true;
                    }

                    // Check if the completed part was "Work".
                    if part_names[idx].eq_ignore_ascii_case("work") {
                        work_completed = Some(part_seconds[idx].max(0) as u64);
                    }
                } else if s.paused {
                    // Overtime: keep decrementing into negative.
                    s.remaining_seconds -= 1;
                    if part_names[s.current_part_index].eq_ignore_ascii_case("work") {
                        s.overtime_work_seconds += 1;
                    }
                }
                // Read session/part names from current state for display.
                let sessions = &s.settings.sessions;
                let session = &sessions[s.active_session_index];
                let part = &session.parts[s.current_part_index];
                let tick = TimerTick {
                    remaining_seconds: s.remaining_seconds,
                    session_name: session.name.clone(),
                    part_name: part.name.clone(),
                    running: s.running,
                    paused: s.paused,
                    daily_total_seconds: 0,
                    active_session_index: s.active_session_index,
                    session_count: sessions.len(),
                };
                drop(s);
                tick
            };

            if let Some(secs) = work_completed {
                add_daily_work_seconds(secs);
            }

            let daily = load_daily_total();
            let tick = TimerTick {
                daily_total_seconds: daily,
                ..tick
            };
            let _ = app.emit("timer-tick", &tick);

            // If the session ended on its own, exit the loop.
            if phase_ended {
                break;
            }
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
    let sessions = &s.settings.sessions;
    let part = &sessions[s.active_session_index].parts[s.current_part_index];
    if part.name.eq_ignore_ascii_case("work") {
        if s.paused {
            // Full duration already recorded when part hit zero;
            // only the overtime seconds are new.
            let overtime = s.overtime_work_seconds;
            if overtime > 0 {
                drop(s);
                add_daily_work_seconds(overtime);
                s = state.lock().unwrap();
            }
        } else {
            let full = (part.minutes * 60) as i64;
            let elapsed = full.saturating_sub(s.remaining_seconds).max(0) as u64;
            if elapsed > 0 {
                drop(s);
                add_daily_work_seconds(elapsed);
                s = state.lock().unwrap();
            }
        }
    }

    s.running = false;
    s.paused = false;
    s.overtime_work_seconds = 0;
    s.current_part_index = 0;
    s.remaining_seconds = (s.settings.sessions[s.active_session_index].parts[0].minutes * 60) as i64;

    let daily = load_daily_total();
    let tick = build_tick(&s, daily);
    drop(s);

    let _ = app.emit("timer-tick", &tick);
    Ok(())
}

#[tauri::command]
pub fn continue_timer(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    if !s.paused {
        return Err("Timer is not in an extendable pause".into());
    }

    let part_count = s.settings.sessions[s.active_session_index].parts.len();
    let next_seconds: i64 = if s.current_part_index + 1 < part_count {
        (s.settings.sessions[s.active_session_index].parts[s.current_part_index + 1].minutes * 60) as i64
    } else {
        0
    };
    let reset_seconds: i64 = (s.settings.sessions[s.active_session_index].parts[0].minutes * 60) as i64;

    let overtime = s.overtime_work_seconds;
    s.overtime_work_seconds = 0;

    if s.current_part_index + 1 < part_count {
        // Advance to next part.
        s.current_part_index += 1;
        s.remaining_seconds = next_seconds;
        s.paused = false;
    } else {
        // Last part — stop and reset.
        s.running = false;
        s.paused = false;
        s.current_part_index = 0;
        s.remaining_seconds = reset_seconds;
    }

    let daily = load_daily_total();
    let tick = build_tick(&s, daily);
    drop(s);

    if overtime > 0 {
        add_daily_work_seconds(overtime);
    }

    let _ = app.emit("timer-tick", &tick);
    Ok(())
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
    sessions: Vec<Session>,
) -> Result<PomodoroSettings, String> {
    // Validate.
    if sessions.is_empty() || sessions.len() > 5 {
        return Err("Must have between 1 and 5 sessions".into());
    }
    for (si, session) in sessions.iter().enumerate() {
        if session.name.trim().is_empty() {
            return Err(format!("Session {} name cannot be empty", si + 1));
        }
        if session.parts.is_empty() || session.parts.len() > 10 {
            return Err(format!(
                "Session '{}' must have between 1 and 10 parts",
                session.name
            ));
        }
        for (pi, part) in session.parts.iter().enumerate() {
            if part.name.trim().is_empty() {
                return Err(format!(
                    "Part {} in session '{}' name cannot be empty",
                    pi + 1,
                    session.name
                ));
            }
            if part.minutes < 1 || part.minutes > 120 {
                return Err(format!(
                    "Part '{}' in session '{}' must be between 1 and 120 minutes",
                    part.name, session.name
                ));
            }
        }
    }

    let new_settings = PomodoroSettings {
        sessions: sessions.clone(),
    };

    let mut s = state.lock().unwrap();
    // Clamp active index if sessions were removed.
    if s.active_session_index >= sessions.len() {
        s.active_session_index = sessions.len() - 1;
    }

    s.settings = new_settings;

    // Persist.
    save_settings(&s.settings.sessions, s.active_session_index);

    // Reset display if stopped.
    if !s.running {
        s.paused = false;
        s.overtime_work_seconds = 0;
        s.current_part_index = 0;
        s.remaining_seconds =
            (s.settings.sessions[s.active_session_index].parts[0].minutes * 60) as i64;
    }

    let daily = load_daily_total();
    let tick = build_tick(&s, daily);
    drop(s);

    if !tick.running {
        let _ = app.emit("timer-tick", &tick);
    }
    Ok(PomodoroSettings {
        sessions,
    })
}

#[tauri::command]
pub fn switch_session(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
    index: usize,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    if s.running {
        return Err("Cannot switch session while timer is running".into());
    }
    if index >= s.settings.sessions.len() {
        return Err(format!(
            "Session index {} out of range (0–{})",
            index,
            s.settings.sessions.len() - 1
        ));
    }

    s.active_session_index = index;
    s.current_part_index = 0;
    s.paused = false;
    s.overtime_work_seconds = 0;
    s.remaining_seconds = (s.settings.sessions[index].parts[0].minutes * 60) as i64;

    save_settings(&s.settings.sessions, s.active_session_index);

    let daily = load_daily_total();
    let tick = build_tick(&s, daily);
    drop(s);

    let _ = app.emit("timer-tick", &tick);
    Ok(())
}

#[tauri::command]
pub fn toggle_dock_mode(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
) -> Result<bool, String> {
    let mut s = state.lock().unwrap();
    s.is_docked = !s.is_docked;
    let docked = s.is_docked;
    drop(s);

    let window = app
        .get_webview_window("main")
        .ok_or("No main window found")?;

    if docked {
        window
            .set_decorations(false)
            .map_err(|e| e.to_string())?;
        window
            .set_always_on_top(true)
            .map_err(|e| e.to_string())?;
        window
            .set_resizable(true)
            .map_err(|e| e.to_string())?;
        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize::new(360.0, 72.0)))
            .map_err(|e| e.to_string())?;

        // Position at top-center of the primary monitor.
        if let Ok(Some(monitor)) = window.primary_monitor() {
            let phys = monitor.size();
            let scale = monitor.scale_factor();
            let logical_width = phys.width as f64 / scale;
            let x = ((logical_width - 420.0) / 2.0).max(0.0);
            window
                .set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                    x, 0.0,
                )))
                .map_err(|e| e.to_string())?;
        }
    } else {
        window
            .set_decorations(true)
            .map_err(|e| e.to_string())?;
        window
            .set_always_on_top(false)
            .map_err(|e| e.to_string())?;
        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize::new(420.0, 520.0)))
            .map_err(|e| e.to_string())?;
        window.center().map_err(|e| e.to_string())?;
        window
            .set_resizable(false)
            .map_err(|e| e.to_string())?;
    }

    save_dock_state(docked);

    let _ = app.emit(
        "dock-mode-changed",
        serde_json::json!({ "docked": docked }),
    );

    Ok(docked)
}

#[tauri::command]
pub fn get_dock_state(state: State<'_, Mutex<PomodoroState>>) -> bool {
    state.lock().unwrap().is_docked
}
