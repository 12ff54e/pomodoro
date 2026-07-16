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
    let file = serde_json::json!({
        "sessions": defs,
        "activeSession": active_index,
    });
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(settings_path(), json);
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
    days_since_epoch_to_date(days)
}

/// Convert days since Unix epoch to "YYYY-MM-DD" (Howard Hinnant's algorithm).
/// Extracted for testability — pass known day counts to verify calendar math.
fn days_since_epoch_to_date(days: i64) -> String {
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

/// Convert a parsed settings file into domain model + active index.
/// Extracted for testability — test the three-branch logic without touching disk.
fn settings_from_file(file: SettingsFile) -> (PomodoroSettings, usize) {
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
        let active = file
            .active_session
            .unwrap_or(0)
            .min(sessions.len().saturating_sub(1));
        (PomodoroSettings { sessions }, active)
    } else if file.work_minutes.is_some() || file.play_minutes.is_some() {
        // Old format — migrate from legacy fields.
        let sessions = legacy_sessions(&file);
        let active = match file.session_type.as_deref() {
            Some("playBreak" | "playbreak") => 1,
            _ => 0,
        };
        (PomodoroSettings { sessions }, active)
    } else {
        // No file yet — use defaults.
        let settings = PomodoroSettings::default();
        (settings, 0)
    }
}

/// Load settings from disk, migrating from the old format if necessary.
pub fn load_settings() -> (PomodoroSettings, usize) {
    let file = load_settings_file();
    let has_legacy = file.sessions.is_none()
        && (file.work_minutes.is_some() || file.play_minutes.is_some());
    let (settings, active) = settings_from_file(file);
    if has_legacy {
        // Persist the migration to disk so we don't migrate again.
        save_settings(&settings.sessions, active);
    }
    (settings, active)
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

/// Validate sessions for update_settings. Extracted for testability.
fn validate_sessions(sessions: &[Session]) -> Result<(), String> {
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
    Ok(())
}

/// Calculate work seconds to record when stopping a running timer.
/// Returns `None` if the current part is not a "work" part or no time has elapsed.
fn stop_work_seconds(
    part_name: &str,
    part_full_seconds: u64,
    paused: bool,
    remaining_seconds: i64,
    overtime_work_seconds: u64,
) -> Option<u64> {
    if !part_name.eq_ignore_ascii_case("work") {
        return None;
    }
    if paused {
        // The full duration was already recorded when the part hit zero;
        // only the overtime seconds (accumulated past zero) are new.
        if overtime_work_seconds > 0 {
            Some(overtime_work_seconds)
        } else {
            None
        }
    } else {
        let full = part_full_seconds as i64;
        let elapsed = full.saturating_sub(remaining_seconds).max(0) as u64;
        if elapsed > 0 {
            Some(elapsed)
        } else {
            None
        }
    }
}

/// Result of advancing past an extendable (paused) part.
struct ContinueAdvance {
    new_part_index: usize,
    new_remaining_seconds: i64,
    new_running: bool,
    new_paused: bool,
}

/// Figure out what state to transition to when the user clicks Continue
/// on an extendable part that is in paused overtime.
fn continue_advance(
    current_part_index: usize,
    part_seconds: &[i64],
    first_part_seconds: i64,
) -> ContinueAdvance {
    if current_part_index + 1 < part_seconds.len() {
        // Advance to the next part.
        ContinueAdvance {
            new_part_index: current_part_index + 1,
            new_remaining_seconds: part_seconds[current_part_index + 1],
            new_running: true,
            new_paused: false,
        }
    } else {
        // Last part — stop and reset.
        ContinueAdvance {
            new_part_index: 0,
            new_remaining_seconds: first_part_seconds,
            new_running: false,
            new_paused: false,
        }
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
    let full_seconds = (part.minutes * 60) as u64;
    if let Some(seconds) = stop_work_seconds(
        &part.name,
        full_seconds,
        s.paused,
        s.remaining_seconds,
        s.overtime_work_seconds,
    ) {
        drop(s);
        add_daily_work_seconds(seconds);
        s = state.lock().unwrap();
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

    let part_seconds: Vec<i64> = s.settings.sessions[s.active_session_index]
        .parts
        .iter()
        .map(|p| (p.minutes * 60) as i64)
        .collect();
    let first_seconds = part_seconds[0];

    let overtime = s.overtime_work_seconds;
    s.overtime_work_seconds = 0;

    let adv = continue_advance(s.current_part_index, &part_seconds, first_seconds);
    s.current_part_index = adv.new_part_index;
    s.remaining_seconds = adv.new_remaining_seconds;
    s.running = adv.new_running;
    s.paused = adv.new_paused;

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
    validate_sessions(&sessions)?;

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
    let currently_docked = {
        let s = state.lock().unwrap();
        s.is_docked
    };
    let docked = !currently_docked;

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
            let x = ((logical_width - 360.0) / 2.0).max(0.0);
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

    // Only mutate state after all window ops succeed.
    {
        let mut s = state.lock().unwrap();
        s.is_docked = docked;
    }

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- PomodoroSettings::default ----

    #[test]
    fn default_settings_structure() {
        let settings = PomodoroSettings::default();
        assert_eq!(settings.sessions.len(), 1);
        let session = &settings.sessions[0];
        assert_eq!(session.name, "Pomodoro");
        assert_eq!(session.parts.len(), 2);
        assert_eq!(session.parts[0].name, "Work");
        assert_eq!(session.parts[0].minutes, 25);
        assert!(!session.parts[0].extendable);
        assert_eq!(session.parts[1].name, "Break");
        assert_eq!(session.parts[1].minutes, 5);
        assert!(!session.parts[1].extendable);
    }

    // ---- today_string ----

    #[test]
    fn today_string_format() {
        let today = today_string();
        // Must be exactly "YYYY-MM-DD"
        assert_eq!(today.len(), 10, "expected YYYY-MM-DD, got '{}'", today);
        assert_eq!(&today[4..5], "-");
        assert_eq!(&today[7..8], "-");
        let year: u32 = today[0..4].parse().unwrap();
        let month: u32 = today[5..7].parse().unwrap();
        let day: u32 = today[8..10].parse().unwrap();
        assert!(year >= 2024);
        assert!(month >= 1 && month <= 12);
        assert!(day >= 1 && day <= 31);
    }

    // ---- legacy_sessions ----

    #[test]
    fn legacy_migration_pomodoro_defaults() {
        // Only work_minutes and break_minutes set.
        let file = SettingsFile {
            sessions: None,
            active_session: None,
            work_minutes: Some(30),
            break_minutes: Some(10),
            play_minutes: None,
            play_break_minutes: None,
            session_type: None,
        };
        let sessions = legacy_sessions(&file);
        assert_eq!(sessions.len(), 2);
        // First session: Pomodoro (Work 30 / Break 10).
        assert_eq!(sessions[0].name, "Pomodoro");
        assert_eq!(sessions[0].parts[0].name, "Work");
        assert_eq!(sessions[0].parts[0].minutes, 30);
        assert_eq!(sessions[0].parts[1].name, "Break");
        assert_eq!(sessions[0].parts[1].minutes, 10);
        // Second session: Play / Break (defaults 25/5).
        assert_eq!(sessions[1].name, "Play / Break");
        assert_eq!(sessions[1].parts[0].name, "Play");
        assert_eq!(sessions[1].parts[0].minutes, 25);
        assert_eq!(sessions[1].parts[1].name, "Break");
        assert_eq!(sessions[1].parts[1].minutes, 5);
    }

    #[test]
    fn legacy_migration_all_fields() {
        // All four minute fields set.
        let file = SettingsFile {
            sessions: None,
            active_session: None,
            work_minutes: Some(25),
            break_minutes: Some(5),
            play_minutes: Some(45),
            play_break_minutes: Some(15),
            session_type: None,
        };
        let sessions = legacy_sessions(&file);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[1].parts[0].minutes, 45);
        assert_eq!(sessions[1].parts[1].minutes, 15);
    }

    #[test]
    fn legacy_migration_empty_uses_defaults() {
        let file = SettingsFile {
            sessions: None,
            active_session: None,
            work_minutes: None,
            break_minutes: None,
            play_minutes: None,
            play_break_minutes: None,
            session_type: None,
        };
        let sessions = legacy_sessions(&file);
        assert_eq!(sessions.len(), 2);
        // Defaults: 25/5 for both.
        assert_eq!(sessions[0].parts[0].minutes, 25);
        assert_eq!(sessions[0].parts[1].minutes, 5);
        assert_eq!(sessions[1].parts[0].minutes, 25);
        assert_eq!(sessions[1].parts[1].minutes, 5);
    }

    // ---- build_tick ----

    #[test]
    fn build_tick_basic() {
        let settings = PomodoroSettings {
            sessions: vec![Session {
                name: "Test Session".into(),
                parts: vec![
                    SessionPart { name: "Focus".into(), minutes: 25, extendable: false },
                    SessionPart { name: "Rest".into(), minutes: 5, extendable: false },
                ],
            }],
        };
        let state = PomodoroState {
            active_session_index: 0,
            current_part_index: 0,
            remaining_seconds: 1500,
            settings,
            running: true,
            paused: false,
            overtime_work_seconds: 0,
            is_docked: false,
        };
        let tick = build_tick(&state, 3600);
        assert_eq!(tick.remaining_seconds, 1500);
        assert_eq!(tick.session_name, "Test Session");
        assert_eq!(tick.part_name, "Focus");
        assert!(tick.running);
        assert!(!tick.paused);
        assert_eq!(tick.daily_total_seconds, 3600);
        assert_eq!(tick.active_session_index, 0);
        assert_eq!(tick.session_count, 1);
    }

    #[test]
    fn build_tick_paused() {
        let settings = PomodoroSettings {
            sessions: vec![Session {
                name: "S".into(),
                parts: vec![
                    SessionPart { name: "W".into(), minutes: 1, extendable: true },
                ],
            }],
        };
        let state = PomodoroState {
            active_session_index: 0,
            current_part_index: 0,
            remaining_seconds: -5,
            settings,
            running: true,
            paused: true,
            overtime_work_seconds: 5,
            is_docked: false,
        };
        let tick = build_tick(&state, 0);
        assert_eq!(tick.remaining_seconds, -5);
        assert!(tick.paused);
        assert_eq!(tick.part_name, "W");
    }

    // ---- validate_sessions ----

    #[test]
    fn validate_sessions_empty() {
        assert!(validate_sessions(&[]).is_err());
    }

    #[test]
    fn validate_sessions_too_many() {
        let too_many: Vec<Session> = (0..6)
            .map(|i| Session { name: format!("S{}", i), parts: vec![] })
            .collect();
        assert!(validate_sessions(&too_many).is_err());
    }

    #[test]
    fn validate_sessions_five_ok() {
        let five: Vec<Session> = (0..5)
            .map(|i| Session {
                name: format!("S{}", i),
                parts: vec![SessionPart { name: "P".into(), minutes: 1, extendable: false }],
            })
            .collect();
        assert!(validate_sessions(&five).is_ok());
    }

    #[test]
    fn validate_empty_session_name() {
        let sessions = vec![Session {
            name: "   ".into(),
            parts: vec![SessionPart { name: "P".into(), minutes: 1, extendable: false }],
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_zero_parts() {
        let sessions = vec![Session { name: "S".into(), parts: vec![] }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_eleven_parts() {
        let sessions = vec![Session {
            name: "S".into(),
            parts: (0..11)
                .map(|i| SessionPart { name: format!("P{}", i), minutes: 1, extendable: false })
                .collect(),
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_ten_parts_ok() {
        let sessions = vec![Session {
            name: "S".into(),
            parts: (0..10)
                .map(|i| SessionPart { name: format!("P{}", i), minutes: 1, extendable: false })
                .collect(),
        }];
        assert!(validate_sessions(&sessions).is_ok());
    }

    #[test]
    fn validate_empty_part_name() {
        let sessions = vec![Session {
            name: "S".into(),
            parts: vec![SessionPart { name: "  ".into(), minutes: 1, extendable: false }],
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_minutes_zero() {
        let sessions = vec![Session {
            name: "S".into(),
            parts: vec![SessionPart { name: "P".into(), minutes: 0, extendable: false }],
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_minutes_121() {
        let sessions = vec![Session {
            name: "S".into(),
            parts: vec![SessionPart { name: "P".into(), minutes: 121, extendable: false }],
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_minutes_120_ok() {
        let sessions = vec![Session {
            name: "S".into(),
            parts: vec![SessionPart { name: "P".into(), minutes: 120, extendable: false }],
        }];
        assert!(validate_sessions(&sessions).is_ok());
    }

    #[test]
    fn validate_valid_config() {
        let sessions = vec![
            Session {
                name: "Pomodoro".into(),
                parts: vec![
                    SessionPart { name: "Work".into(), minutes: 25, extendable: false },
                    SessionPart { name: "Break".into(), minutes: 5, extendable: false },
                ],
            },
            Session {
                name: "Play / Break".into(),
                parts: vec![
                    SessionPart { name: "Play".into(), minutes: 25, extendable: true },
                    SessionPart { name: "Break".into(), minutes: 10, extendable: false },
                ],
            },
        ];
        assert!(validate_sessions(&sessions).is_ok());
    }

    // ---- stop_work_seconds ----

    #[test]
    fn stop_work_non_work_part() {
        // "Break" parts should never record work seconds.
        assert_eq!(stop_work_seconds("Break", 300, false, 200, 0), None);
    }

    #[test]
    fn stop_work_mid_session() {
        // 25-min work part, 500s elapsed (1000s remaining).
        assert_eq!(stop_work_seconds("Work", 1500, false, 1000, 0), Some(500));
    }

    #[test]
    fn stop_work_no_elapsed() {
        // Just started, no time elapsed.
        assert_eq!(stop_work_seconds("Work", 1500, false, 1500, 0), None);
    }

    #[test]
    fn stop_work_fully_elapsed() {
        // Timer hit zero exactly (non-extendable).
        assert_eq!(stop_work_seconds("Work", 1500, false, 0, 0), Some(1500));
    }

    #[test]
    fn stop_work_paused_with_overtime() {
        // Paused in overtime, accumulated 30 overtime seconds.
        assert_eq!(stop_work_seconds("Work", 1500, true, -30, 30), Some(30));
    }

    #[test]
    fn stop_work_paused_no_overtime() {
        // Paused exactly at zero, no overtime accumulated yet.
        assert_eq!(stop_work_seconds("Work", 1500, true, 0, 0), None);
    }

    #[test]
    fn stop_work_case_insensitive() {
        // "WORK", "work", "Work" should all match.
        assert_eq!(stop_work_seconds("WORK", 1500, false, 0, 0), Some(1500));
        assert_eq!(stop_work_seconds("work", 1500, false, 0, 0), Some(1500));
        assert_eq!(stop_work_seconds("wOrK", 1500, false, 0, 0), Some(1500));
    }

    #[test]
    fn stop_work_remaining_negative_not_paused() {
        // Edge case: negative remaining without paused flag (shouldn't
        // happen in practice, but function handles it gracefully).
        let result = stop_work_seconds("Work", 1500, false, -10, 0);
        // full.saturating_sub(-10) = 1500 - (-10) = 1510, capped by max(0) → 1510
        assert_eq!(result, Some(1510));
    }

    // ---- continue_advance ----

    #[test]
    fn continue_advance_mid_session() {
        // Two-part session, currently on part 0 (index 0), advance to part 1.
        let adv = continue_advance(0, &[1500, 300], 1500);
        assert_eq!(adv.new_part_index, 1);
        assert_eq!(adv.new_remaining_seconds, 300);
        assert!(adv.new_running);
        assert!(!adv.new_paused);
    }

    #[test]
    fn continue_advance_last_part() {
        // Two-part session, currently on part 1 (last), advance should stop/reset.
        let adv = continue_advance(1, &[1500, 300], 1500);
        assert_eq!(adv.new_part_index, 0);
        assert_eq!(adv.new_remaining_seconds, 1500);
        assert!(!adv.new_running);
        assert!(!adv.new_paused);
    }

    #[test]
    fn continue_advance_single_part() {
        // Single-part session: only part is also the last part → stop and reset.
        let adv = continue_advance(0, &[600], 600);
        assert_eq!(adv.new_part_index, 0);
        assert_eq!(adv.new_remaining_seconds, 600);
        assert!(!adv.new_running);
        assert!(!adv.new_paused);
    }

    // ---- days_since_epoch_to_date ----

    #[test]
    fn days_to_date_epoch() {
        // Day 0 = 1970-01-01 (Unix epoch).
        assert_eq!(days_since_epoch_to_date(0), "1970-01-01");
    }

    #[test]
    fn days_to_date_known_dates() {
        // 1970-01-02 = day 1
        assert_eq!(days_since_epoch_to_date(1), "1970-01-02");
        // 2000-01-01 = day 10957
        assert_eq!(days_since_epoch_to_date(10957), "2000-01-01");
        // 2024-01-01 = day 19723
        assert_eq!(days_since_epoch_to_date(19723), "2024-01-01");
    }

    #[test]
    fn days_to_date_leap_year() {
        // 2024-02-29 (leap day) = day 19782
        assert_eq!(days_since_epoch_to_date(19782), "2024-02-29");
        // 2024-03-01 = day 19783
        assert_eq!(days_since_epoch_to_date(19783), "2024-03-01");
    }

    #[test]
    fn days_to_date_y2k_transition() {
        // 1999-12-31 = day 10956
        assert_eq!(days_since_epoch_to_date(10956), "1999-12-31");
        // 2000-01-01 = day 10957
        assert_eq!(days_since_epoch_to_date(10957), "2000-01-01");
        // 2000-02-29 (leap year, century divisible by 400) = day 11016
        assert_eq!(days_since_epoch_to_date(11016), "2000-02-29");
        // 2000-03-01 = day 11017
        assert_eq!(days_since_epoch_to_date(11017), "2000-03-01");
    }

    // ---- settings_from_file ----

    #[test]
    fn settings_from_file_new_format() {
        // A new-format file with two sessions, extendable work part.
        let file = SettingsFile {
            sessions: Some(vec![
                SessionDef {
                    name: "Pomodoro".into(),
                    parts: vec![
                        PartDef { name: "Work".into(), minutes: 25, extendable: true },
                        PartDef { name: "Break".into(), minutes: 5, extendable: false },
                    ],
                },
                SessionDef {
                    name: "Deep Focus".into(),
                    parts: vec![
                        PartDef { name: "Focus".into(), minutes: 50, extendable: true },
                        PartDef { name: "Rest".into(), minutes: 10, extendable: false },
                    ],
                },
            ]),
            active_session: Some(1),
            ..Default::default()
        };
        let (settings, active) = settings_from_file(file);
        assert_eq!(settings.sessions.len(), 2);
        assert_eq!(settings.sessions[0].parts[0].extendable, true);
        assert_eq!(settings.sessions[1].name, "Deep Focus");
        assert_eq!(settings.sessions[1].parts[0].minutes, 50);
        assert_eq!(settings.sessions[1].parts[1].minutes, 10);
        assert_eq!(active, 1);
    }

    #[test]
    fn settings_from_file_active_index_clamped() {
        // Active index beyond bounds should be clamped to last valid index.
        let file = SettingsFile {
            sessions: Some(vec![
                SessionDef {
                    name: "A".into(),
                    parts: vec![PartDef { name: "X".into(), minutes: 1, extendable: false }],
                },
                SessionDef {
                    name: "B".into(),
                    parts: vec![PartDef { name: "Y".into(), minutes: 1, extendable: false }],
                },
            ]),
            active_session: Some(99), // way out of bounds
            ..Default::default()
        };
        let (settings, active) = settings_from_file(file);
        assert_eq!(active, 1); // clamped to sessions.len() - 1
        assert_eq!(settings.sessions.len(), 2);
    }

    #[test]
    fn settings_from_file_new_format_no_active() {
        // active_session missing from file → default to 0.
        let file = SettingsFile {
            sessions: Some(vec![
                SessionDef {
                    name: "Solo".into(),
                    parts: vec![PartDef { name: "Task".into(), minutes: 30, extendable: false }],
                },
            ]),
            active_session: None,
            ..Default::default()
        };
        let (_, active) = settings_from_file(file);
        assert_eq!(active, 0);
    }

    #[test]
    fn settings_from_file_empty_defaults() {
        // Completely empty SettingsFile → PomodoroSettings::default().
        let file = SettingsFile::default();
        let (settings, active) = settings_from_file(file);
        assert_eq!(settings.sessions.len(), 1);
        assert_eq!(settings.sessions[0].name, "Pomodoro");
        assert_eq!(settings.sessions[0].parts.len(), 2);
        assert_eq!(settings.sessions[0].parts[0].name, "Work");
        assert_eq!(settings.sessions[0].parts[0].minutes, 25);
        assert_eq!(settings.sessions[0].parts[1].name, "Break");
        assert_eq!(settings.sessions[0].parts[1].minutes, 5);
        assert_eq!(active, 0);
    }

    // ---- JSON round-trip ----

    #[test]
    fn json_roundtrip_preserves_extendable() {
        // Session → JSON → Session — all fields survive, especially extendable.
        let original = Session {
            name: "Test".into(),
            parts: vec![
                SessionPart { name: "Focus".into(), minutes: 45, extendable: true },
                SessionPart { name: "Break".into(), minutes: 15, extendable: false },
            ],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.name, "Test");
        assert_eq!(parsed.parts.len(), 2);
        assert_eq!(parsed.parts[0].name, "Focus");
        assert_eq!(parsed.parts[0].minutes, 45);
        assert_eq!(parsed.parts[0].extendable, true);
        assert_eq!(parsed.parts[1].name, "Break");
        assert_eq!(parsed.parts[1].minutes, 15);
        assert_eq!(parsed.parts[1].extendable, false);
    }

    #[test]
    fn json_deserialize_missing_extendable_defaults_false() {
        // If the JSON omits "extendable", serde should default it to false.
        let json = r#"{"name":"Work","minutes":25}"#;
        let part: SessionPart = serde_json::from_str(json).expect("deserialize");
        assert_eq!(part.name, "Work");
        assert_eq!(part.minutes, 25);
        assert!(!part.extendable, "missing extendable should default to false");
    }

    #[test]
    fn json_deserialize_extendable_true() {
        let json = r#"{"name":"Work","minutes":25,"extendable":true}"#;
        let part: SessionPart = serde_json::from_str(json).expect("deserialize");
        assert_eq!(part.name, "Work");
        assert_eq!(part.minutes, 25);
        assert!(part.extendable);
    }

    #[test]
    fn json_settings_file_roundtrip() {
        // Full SettingsFile → JSON → SettingsFile round-trip.
        let file = SettingsFile {
            sessions: Some(vec![
                SessionDef {
                    name: "Pomodoro".into(),
                    parts: vec![
                        PartDef { name: "Work".into(), minutes: 30, extendable: true },
                        PartDef { name: "Break".into(), minutes: 10, extendable: false },
                    ],
                },
            ]),
            active_session: Some(0),
            ..Default::default()
        };
        let json = serde_json::to_string(&file).expect("serialize");
        let parsed: SettingsFile = serde_json::from_str(&json).expect("deserialize");
        let sessions = parsed.sessions.expect("sessions present");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "Pomodoro");
        assert_eq!(sessions[0].parts[0].minutes, 30);
        assert_eq!(sessions[0].parts[0].extendable, true);
        assert_eq!(sessions[0].parts[1].name, "Break");
        assert_eq!(sessions[0].parts[1].minutes, 10);
        assert_eq!(sessions[0].parts[1].extendable, false);
        assert_eq!(parsed.active_session, Some(0));
    }
}
