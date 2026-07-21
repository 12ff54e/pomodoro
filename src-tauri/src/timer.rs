use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::notification;
use crate::notification::ServiceEvent;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPart {
    pub name: String,
    pub minutes: u64,
    #[serde(default)]
    pub extendable: bool,
    /// When true, time spent on this part is recorded to the daily log.
    #[serde(default)]
    pub track_time: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Stable UUID, generated on creation. Empty string only during
    /// migration / frontend handoff — the backend assigns a real UUID.
    pub id: String,
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
                id: uuid::Uuid::new_v4().to_string(),
                name: "Pomodoro".into(),
                parts: vec![
                    SessionPart {
                        name: "Work".into(),
                        minutes: 25,
                        extendable: false,
                        track_time: true,
                    },
                    SessionPart {
                        name: "Break".into(),
                        minutes: 5,
                        extendable: false,
                        track_time: false,
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
    pub part_index: usize,
    pub running: bool,
    pub paused: bool,
    pub daily_total_seconds: u64,
    pub active_session_id: String,
    pub session_count: usize,
}

#[derive(Debug)]
pub struct PomodoroState {
    /// UUID of the currently active session.
    pub active_session_id: String,
    pub current_part_index: usize,
    pub remaining_seconds: i64,
    pub settings: PomodoroSettings,
    pub running: bool,
    pub paused: bool,
    /// Accumulated tracked overtime seconds for the current part
    /// (flushed on Continue/Stop). Only increments when the part has
    /// `track_time` enabled.
    pub overtime_tracked_seconds: u64,
    /// Whether the window is in dock mode (small, always-on-top, docked to top of screen).
    pub is_docked: bool,
    /// When true, `part.minutes` is interpreted as seconds instead of minutes
    /// so that E2E tests complete in seconds rather than minutes.
    pub test_mode: bool,
}

// ---------------------------------------------------------------------------
// Persistence (JSON file next to the executable)
// ---------------------------------------------------------------------------

/// Platform-appropriate data directory, set once by `init_data_dir()` at startup.
/// Falls back to the exe-adjacent directory (desktop backward compat) if not
/// yet initialised.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

fn data_dir() -> &'static PathBuf {
    DATA_DIR.get().unwrap_or_else(|| {
        static FALLBACK: OnceLock<PathBuf> = OnceLock::new();
        FALLBACK.get_or_init(|| {
            let exe = std::env::current_exe().unwrap_or_default();
            exe.parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf()
        })
    })
}

/// Called once from `setup()` to set the platform-appropriate data directory.
/// On Android this is the app's private data dir; on desktop it is exe-adjacent.
pub fn init_data_dir(path: PathBuf) {
    let _ = std::fs::create_dir_all(&path);
    DATA_DIR.set(path).ok(); // ok(): silently ignore if already set
}

/// `<data-dir>/pomodoro.json` — small, fast to load.
fn settings_path() -> PathBuf {
    data_dir().join("pomodoro.json")
}

/// `<data-dir>/pomodoro_record.json` — detailed per-session/per-part log.
fn record_path() -> PathBuf {
    data_dir().join("pomodoro_record.json")
}

// ---- Settings file ----

#[derive(Debug, Serialize, Deserialize, Default)]
struct SettingsFile {
    #[serde(default)]
    sessions: Option<Vec<SessionDef>>,
    #[serde(default)]
    #[serde(rename = "activeSession")]
    active_session: Option<serde_json::Value>,

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
    #[serde(default)]
    id: String,
    name: String,
    parts: Vec<PartDef>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PartDef {
    name: String,
    minutes: u64,
    #[serde(default)]
    extendable: bool,
    #[serde(default)]
    track_time: bool,
}

/// Build default sessions from legacy hardcoded sessions (migration path).
fn legacy_sessions(_file: &SettingsFile) -> Vec<Session> {
    let wm = _file.work_minutes.unwrap_or(25);
    let bm = _file.break_minutes.unwrap_or(5);
    let pm = _file.play_minutes.unwrap_or(25);
    let pbm = _file.play_break_minutes.unwrap_or(5);

    vec![
        Session {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Pomodoro".into(),
            parts: vec![
                SessionPart {
                    name: "Work".into(),
                    minutes: wm,
                    extendable: false,
                    track_time: true,
                },
                SessionPart {
                    name: "Break".into(),
                    minutes: bm,
                    extendable: false,
                    track_time: false,
                },
            ],
        },
        Session {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Play / Break".into(),
            parts: vec![
                SessionPart {
                    name: "Play".into(),
                    minutes: pm,
                    extendable: false,
                    track_time: true,
                },
                SessionPart {
                    name: "Break".into(),
                    minutes: pbm,
                    extendable: false,
                    track_time: false,
                },
            ],
        },
    ]
}

fn load_settings_file() -> SettingsFile {
    let raw = match std::fs::read_to_string(settings_path()) {
        Ok(s) => s,
        Err(_) => return SettingsFile::default(),
    };
    let root: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_default();
    let mut file: SettingsFile =
        serde_json::from_value(root.clone()).unwrap_or_default();

    // Migration 1: If activeSession is a number (old format), resolve to
    // the corresponding session UUID string.
    if let Some(active) = root.get("activeSession") {
        if active.is_number() {
            if let Some(sessions) = &file.sessions {
                if let Some(idx) = active.as_u64().map(|i| i as usize) {
                    if idx < sessions.len() {
                        let uuid = if sessions[idx].id.is_empty() {
                            uuid::Uuid::new_v4().to_string()
                        } else {
                            sessions[idx].id.clone()
                        };
                        file.active_session =
                            Some(serde_json::Value::String(uuid));
                    }
                }
            }
        }
    } else if root.get("activeSessionId").is_some() {
        // Already new format — handled via serde_json::Value.
        file.active_session =
            root.get("activeSessionId").cloned();
    }

    // Migration 2: Generate UUIDs for sessions that don't have one.
    if let Some(ref mut sessions) = file.sessions {
        for s in sessions.iter_mut() {
            if s.id.is_empty() {
                s.id = uuid::Uuid::new_v4().to_string();
            }
        }
    }

    file
}

fn save_settings(sessions: &[Session], active_id: &str) {
    let defs: Vec<SessionDef> = sessions
        .iter()
        .map(|s| SessionDef {
            id: s.id.clone(),
            name: s.name.clone(),
            parts: s
                .parts
                .iter()
                .map(|p| PartDef {
                    name: p.name.clone(),
                    minutes: p.minutes,
                    extendable: p.extendable,
                    track_time: p.track_time,
                })
                .collect(),
        })
        .collect();
    let file = serde_json::json!({
        "sessions": defs,
        "activeSessionId": active_id,
    });
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(settings_path(), json);
    }
}

// ---- Record file (per-session, per-part time tracking) ----

/// date → session_uuid → part_index_string → accumulated seconds
type PomodoroRecord = BTreeMap<String, BTreeMap<String, BTreeMap<String, u64>>>;

fn load_record() -> PomodoroRecord {
    match std::fs::read_to_string(record_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    }
}

fn save_record(record: &PomodoroRecord) {
    if let Ok(json) = serde_json::to_string_pretty(record) {
        let _ = std::fs::write(record_path(), json);
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

/// Convert configured minutes to seconds. When `test_mode` is true, treats
/// minutes as seconds so E2E tests complete in seconds instead of minutes.
pub fn minutes_to_seconds(minutes: u64, test_mode: bool) -> i64 {
    if test_mode { minutes as i64 } else { (minutes * 60) as i64 }
}

/// Find a session's array index by its UUID. Returns `None` if not found.
pub fn find_session_index(sessions: &[Session], id: &str) -> Option<usize> {
    sessions.iter().position(|s| s.id == id)
}

/// Convert a parsed settings file into domain model + active session UUID.
/// Extracted for testability — test the three-branch logic without touching disk.
fn settings_from_file(file: SettingsFile) -> (PomodoroSettings, String) {
    if let Some(sessions) = file.sessions {
        // New format — convert SessionDef → Session.
        let sessions: Vec<Session> = sessions
            .into_iter()
            .map(|s| Session {
                id: s.id,
                name: s.name,
                parts: s
                    .parts
                    .into_iter()
                    .map(|p| SessionPart {
                        name: p.name,
                        minutes: p.minutes,
                        extendable: p.extendable,
                        track_time: p.track_time,
                    })
                    .collect(),
            })
            .collect();

        // Resolve active_session Value to a UUID string.
        let active_id = match &file.active_session {
            Some(serde_json::Value::String(id)) => {
                if sessions.iter().any(|s| s.id == *id) {
                    id.clone()
                } else {
                    sessions.first().map(|s| s.id.clone()).unwrap_or_default()
                }
            }
            Some(serde_json::Value::Number(n)) => {
                // Old format number — resolve to index if possible.
                if let Some(idx) = n.as_u64().map(|i| i as usize) {
                    sessions
                        .get(idx)
                        .map(|s| s.id.clone())
                        .unwrap_or_else(|| {
                            sessions.first().map(|s| s.id.clone()).unwrap_or_default()
                        })
                } else {
                    sessions.first().map(|s| s.id.clone()).unwrap_or_default()
                }
            }
            _ => sessions.first().map(|s| s.id.clone()).unwrap_or_default(),
        };

        (PomodoroSettings { sessions }, active_id)
    } else if file.work_minutes.is_some() || file.play_minutes.is_some() {
        // Old format — migrate from legacy fields.
        let sessions = legacy_sessions(&file);
        let active_id = sessions[0].id.clone();
        (PomodoroSettings { sessions }, active_id)
    } else {
        // No file yet — use defaults.
        let settings = PomodoroSettings::default();
        let active_id = settings.sessions[0].id.clone();
        (settings, active_id)
    }
}

/// Load settings from disk, migrating from the old format if necessary.
pub fn load_settings() -> (PomodoroSettings, String) {
    let file = load_settings_file();
    let has_legacy = file.sessions.is_none()
        && (file.work_minutes.is_some() || file.play_minutes.is_some());
    let (settings, active_id) = settings_from_file(file);
    if has_legacy {
        // Persist the migration to disk so we don't migrate again.
        save_settings(&settings.sessions, &active_id);
    }
    (settings, active_id)
}

/// Reads today's total tracked seconds from the record file (sum across all
/// sessions and parts).
pub fn load_daily_total() -> u64 {
    let record = load_record();
    let today = today_string();
    record
        .get(&today)
        .map(|day| {
            day.values()
                .flat_map(|session| session.values())
                .sum()
        })
        .unwrap_or(0)
}

/// Records `seconds` of tracked time for a specific session/part on today's
/// date. Returns the new daily total.
pub fn add_record_seconds(session_id: &str, part_index: usize, seconds: u64) -> u64 {
    let mut record = load_record();
    let today = today_string();
    let day_entry = record.entry(today).or_default();
    let session_entry = day_entry.entry(session_id.to_string()).or_default();
    let prev = session_entry
        .get(&part_index.to_string())
        .copied()
        .unwrap_or(0);
    session_entry.insert(part_index.to_string(), prev + seconds);
    // Compute daily total before the mutable borrow ends.
    let daily_total: u64 = day_entry
        .values()
        .flat_map(|s| s.values())
        .sum();
    save_record(&record);
    daily_total
}

/// Build a TimerTick from the current state.
pub fn build_tick(state: &PomodoroState, daily_total: u64) -> TimerTick {
    let idx = find_session_index(&state.settings.sessions, &state.active_session_id)
        .unwrap_or(0);
    let session = &state.settings.sessions[idx];
    let part = &session.parts[state.current_part_index];
    let part_name = if part.name.trim().is_empty() {
        format!("Part {}", state.current_part_index + 1)
    } else {
        part.name.clone()
    };
    TimerTick {
        remaining_seconds: state.remaining_seconds,
        session_name: session.name.clone(),
        part_name,
        part_index: state.current_part_index,
        running: state.running,
        paused: state.paused,
        daily_total_seconds: daily_total,
        active_session_id: state.active_session_id.clone(),
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
        for (_pi, part) in session.parts.iter().enumerate() {
            if part.minutes < 1 || part.minutes > 120 {
                return Err(format!(
                    "Part {} in session '{}' must be between 1 and 120 minutes",
                    _pi + 1,
                    session.name
                ));
            }
        }
    }
    Ok(())
}

/// Calculate tracked seconds to record when stopping a running timer.
/// Returns `None` if the part does not have track_time enabled or no time
/// has elapsed.
pub fn stop_tracked_seconds(
    track_time: bool,
    part_full_seconds: u64,
    paused: bool,
    remaining_seconds: i64,
    overtime_tracked_seconds: u64,
) -> Option<u64> {
    if !track_time {
        return None;
    }
    if paused {
        // The full duration was already recorded when the part hit zero;
        // only the overtime seconds (accumulated past zero) are new.
        if overtime_tracked_seconds > 0 {
            Some(overtime_tracked_seconds)
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
    let active_id = s.active_session_id.clone();
    let active_idx = find_session_index(&sessions, &active_id).unwrap_or(0);
    let test_mode = s.test_mode;
    s.running = true;
    s.paused = false;
    s.overtime_tracked_seconds = 0;

    // Emit an initial tick immediately so the frontend reflects the new
    // state without waiting for the first 1 s sleep in the timer thread.
    let daily = load_daily_total();
    let tick = build_tick(&s, daily);
    drop(s);
    let _ = app.emit("timer-tick", &tick);

    notification::notify_service(&app, ServiceEvent::Start {
        tick: tick.clone(),
    });

    std::thread::spawn(move || {
        // Snapshot part metadata from the session at start time.
        let _part_names: Vec<String> = sessions[active_idx]
            .parts.iter().map(|p| p.name.clone()).collect();
        let part_seconds: Vec<i64> = sessions[active_idx]
            .parts.iter().map(|p| minutes_to_seconds(p.minutes, test_mode)).collect();
        let part_extendable: Vec<bool> = sessions[active_idx]
            .parts.iter().map(|p| p.extendable).collect();
        let part_track_time: Vec<bool> = sessions[active_idx]
            .parts.iter().map(|p| p.track_time).collect();
        let session_id = active_id;

        // Track previous state to detect transitions for the Android
        // notification service (otherwise skipped on desktop).
        let mut prev_part_index: usize = 0;
        let mut prev_paused: bool = false;

        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));

            let mut tracked_completed: Option<u64> = None;
            let mut completed_part_index: Option<usize> = None;
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

                    // Record the completed part duration if tracking is enabled.
                    if part_track_time[idx] {
                        tracked_completed = Some(part_seconds[idx].max(0) as u64);
                        completed_part_index = Some(idx);
                    }
                } else if s.paused {
                    // Overtime: keep decrementing into negative.
                    s.remaining_seconds -= 1;
                    if part_track_time[s.current_part_index] {
                        s.overtime_tracked_seconds += 1;
                    }
                }
                // Resolve session for display.
                let sessions = &s.settings.sessions;
                let idx = find_session_index(sessions, &s.active_session_id)
                    .unwrap_or(0);
                let session = &sessions[idx];
                let part = &session.parts[s.current_part_index];
                let part_name = if part.name.trim().is_empty() {
                    format!("Part {}", s.current_part_index + 1)
                } else {
                    part.name.clone()
                };
                let tick = TimerTick {
                    remaining_seconds: s.remaining_seconds,
                    session_name: session.name.clone(),
                    part_name,
                    part_index: s.current_part_index,
                    running: s.running,
                    paused: s.paused,
                    daily_total_seconds: 0,
                    active_session_id: s.active_session_id.clone(),
                    session_count: sessions.len(),
                };
                drop(s);
                tick
            };

            if let Some(secs) = tracked_completed {
                let pi = completed_part_index.unwrap_or(0);
                add_record_seconds(&session_id, pi, secs);
            }

            let daily = load_daily_total();
            let tick = TimerTick {
                daily_total_seconds: daily,
                ..tick
            };
            let _ = app.emit("timer-tick", &tick);

            // Notify Android notification service on state transitions.
            if phase_ended {
                notification::notify_service(&app, ServiceEvent::Stop);
                break;
            } else if tick.part_index != prev_part_index || tick.paused != prev_paused {
                notification::notify_service(
                    &app,
                    ServiceEvent::PartUpdated {
                        tick: tick.clone(),
                    },
                );
            }
            prev_part_index = tick.part_index;
            prev_paused = tick.paused;
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

    // Record partial tracked time.
    let sessions = s.settings.sessions.clone();
    let idx = find_session_index(&sessions, &s.active_session_id).unwrap_or(0);
    let part = &sessions[idx].parts[s.current_part_index];
    let full_seconds = minutes_to_seconds(part.minutes, s.test_mode) as u64;
    let session_id = s.active_session_id.clone();
    let part_index = s.current_part_index;

    if let Some(seconds) = stop_tracked_seconds(
        part.track_time,
        full_seconds,
        s.paused,
        s.remaining_seconds,
        s.overtime_tracked_seconds,
    ) {
        drop(s);
        add_record_seconds(&session_id, part_index, seconds);
        s = state.lock().unwrap();
    }

    s.running = false;
    s.paused = false;
    s.overtime_tracked_seconds = 0;
    s.current_part_index = 0;
    let idx = find_session_index(&s.settings.sessions, &s.active_session_id).unwrap_or(0);
    s.remaining_seconds = minutes_to_seconds(
        s.settings.sessions[idx].parts[0].minutes,
        s.test_mode,
    );

    let daily = load_daily_total();
    let tick = build_tick(&s, daily);
    drop(s);

    let _ = app.emit("timer-tick", &tick);
    notification::notify_service(&app, ServiceEvent::Stop);
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

    let idx = find_session_index(&s.settings.sessions, &s.active_session_id).unwrap_or(0);
    let part_seconds: Vec<i64> = s.settings.sessions[idx]
        .parts
        .iter()
        .map(|p| minutes_to_seconds(p.minutes, s.test_mode))
        .collect();
    let first_seconds = part_seconds[0];
    let session_id = s.active_session_id.clone();
    let part_index = s.current_part_index;

    let overtime = s.overtime_tracked_seconds;
    s.overtime_tracked_seconds = 0;

    let adv = continue_advance(s.current_part_index, &part_seconds, first_seconds);
    s.current_part_index = adv.new_part_index;
    s.remaining_seconds = adv.new_remaining_seconds;
    s.running = adv.new_running;
    s.paused = adv.new_paused;

    let daily = load_daily_total();
    let tick = build_tick(&s, daily);
    drop(s);

    if overtime > 0 {
        add_record_seconds(&session_id, part_index, overtime);
    }

    let _ = app.emit("timer-tick", &tick);

    // Notify Android notification service — advance to next part (or stop).
    if tick.running {
        notification::notify_service(
            &app,
            ServiceEvent::PartUpdated {
                tick: tick.clone(),
            },
        );
    } else {
        notification::notify_service(&app, ServiceEvent::Stop);
    }

    Ok(())
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, Mutex<PomodoroState>>,
    sessions: Vec<Session>,
) -> Result<PomodoroSettings, String> {
    validate_sessions(&sessions)?;

    // Ensure all sessions have UUIDs (new sessions from frontend may not).
    let sessions: Vec<Session> = sessions
        .into_iter()
        .map(|mut s| {
            if s.id.is_empty() {
                s.id = uuid::Uuid::new_v4().to_string();
            }
            s
        })
        .collect();

    let new_settings = PomodoroSettings {
        sessions: sessions.clone(),
    };

    let mut s = state.lock().unwrap();
    // If the active session UUID no longer exists (session deleted),
    // fall back to the first session.
    if find_session_index(&sessions, &s.active_session_id).is_none() {
        s.active_session_id = sessions[0].id.clone();
    }

    s.settings = new_settings;

    // Persist.
    save_settings(&s.settings.sessions, &s.active_session_id);

    // Reset display if stopped.
    if !s.running {
        s.paused = false;
        s.overtime_tracked_seconds = 0;
        s.current_part_index = 0;
        let idx = find_session_index(&s.settings.sessions, &s.active_session_id)
            .unwrap_or(0);
        s.remaining_seconds = minutes_to_seconds(
            s.settings.sessions[idx].parts[0].minutes,
            s.test_mode,
        );
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
    session_id: String,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    if s.running {
        return Err("Cannot switch session while timer is running".into());
    }

    let idx = find_session_index(&s.settings.sessions, &session_id)
        .ok_or_else(|| format!("Session '{}' not found", session_id))?;

    s.active_session_id = session_id;
    s.current_part_index = 0;
    s.paused = false;
    s.overtime_tracked_seconds = 0;
    s.remaining_seconds = minutes_to_seconds(
        s.settings.sessions[idx].parts[0].minutes,
        s.test_mode,
    );

    save_settings(&s.settings.sessions, &s.active_session_id);

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
    #[cfg(desktop)]
    {
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

    #[cfg(mobile)]
    {
        // Dock mode has no window-level meaning on mobile —
        // just toggle the state flag so the frontend stays consistent.
        let docked = {
            let mut s = state.lock().unwrap();
            s.is_docked = !s.is_docked;
            s.is_docked
        };
        let _ = app.emit(
            "dock-mode-changed",
            serde_json::json!({ "docked": docked }),
        );
        Ok(docked)
    }
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
        assert!(!session.id.is_empty(), "session should have a UUID");
        assert_eq!(session.name, "Pomodoro");
        assert_eq!(session.parts.len(), 2);
        assert_eq!(session.parts[0].name, "Work");
        assert_eq!(session.parts[0].minutes, 25);
        assert!(!session.parts[0].extendable);
        assert!(session.parts[0].track_time, "Work should have track_time=true");
        assert_eq!(session.parts[1].name, "Break");
        assert_eq!(session.parts[1].minutes, 5);
        assert!(!session.parts[1].extendable);
        assert!(!session.parts[1].track_time, "Break should have track_time=false");
    }

    // ---- minutes_to_seconds ----

    #[test]
    fn minutes_to_seconds_normal_mode() {
        assert_eq!(minutes_to_seconds(25, false), 1500);
        assert_eq!(minutes_to_seconds(5, false), 300);
        assert_eq!(minutes_to_seconds(1, false), 60);
        assert_eq!(minutes_to_seconds(120, false), 7200);
    }

    #[test]
    fn minutes_to_seconds_test_mode() {
        assert_eq!(minutes_to_seconds(25, true), 25);
        assert_eq!(minutes_to_seconds(5, true), 5);
        assert_eq!(minutes_to_seconds(1, true), 1);
        assert_eq!(minutes_to_seconds(120, true), 120);
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
        assert!(!sessions[0].id.is_empty());
        assert!(sessions[0].parts[0].track_time, "Work should track time");
        assert!(!sessions[0].parts[1].track_time, "Break should not track time");
        assert_eq!(sessions[0].name, "Pomodoro");
        assert_eq!(sessions[0].parts[0].name, "Work");
        assert_eq!(sessions[0].parts[0].minutes, 30);
        assert_eq!(sessions[0].parts[1].name, "Break");
        assert_eq!(sessions[0].parts[1].minutes, 10);
        // Second session: Play / Break (defaults 25/5).
        assert!(!sessions[1].id.is_empty());
        assert!(sessions[1].parts[0].track_time, "Play should track time");
        assert!(!sessions[1].parts[1].track_time);
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
        let session_id = "test-sid-1".to_string();
        let settings = PomodoroSettings {
            sessions: vec![Session {
                id: session_id.clone(),
                name: "Test Session".into(),
                parts: vec![
                    SessionPart { name: "Focus".into(), minutes: 25, extendable: false, track_time: true },
                    SessionPart { name: "Rest".into(), minutes: 5, extendable: false, track_time: false },
                ],
            }],
        };
        let state = PomodoroState {
            active_session_id: session_id,
            current_part_index: 0,
            remaining_seconds: 1500,
            settings,
            running: true,
            paused: false,
            overtime_tracked_seconds: 0,
            is_docked: false,
            test_mode: false,
        };
        let tick = build_tick(&state, 3600);
        assert_eq!(tick.remaining_seconds, 1500);
        assert_eq!(tick.session_name, "Test Session");
        assert_eq!(tick.part_name, "Focus");
        assert_eq!(tick.part_index, 0);
        assert!(tick.running);
        assert!(!tick.paused);
        assert_eq!(tick.daily_total_seconds, 3600);
        assert_eq!(tick.active_session_id, "test-sid-1");
        assert_eq!(tick.session_count, 1);
    }

    #[test]
    fn build_tick_paused() {
        let session_id = "s1".to_string();
        let settings = PomodoroSettings {
            sessions: vec![Session {
                id: session_id.clone(),
                name: "S".into(),
                parts: vec![
                    SessionPart { name: "W".into(), minutes: 1, extendable: true, track_time: false },
                ],
            }],
        };
        let state = PomodoroState {
            active_session_id: session_id,
            current_part_index: 0,
            remaining_seconds: -5,
            settings,
            running: true,
            paused: true,
            overtime_tracked_seconds: 5,
            is_docked: false,
            test_mode: false,
        };
        let tick = build_tick(&state, 0);
        assert_eq!(tick.remaining_seconds, -5);
        assert!(tick.paused);
        assert_eq!(tick.part_name, "W");
        assert_eq!(tick.part_index, 0);
    }

    #[test]
    fn build_tick_empty_part_name_fallback() {
        let session_id = "sid-fallback".to_string();
        let settings = PomodoroSettings {
            sessions: vec![Session {
                id: session_id.clone(),
                name: "Test".into(),
                parts: vec![
                    SessionPart { name: "  ".into(), minutes: 10, extendable: false, track_time: false },
                    SessionPart { name: "  ".into(), minutes: 10, extendable: false, track_time: false },
                    SessionPart { name: "  ".into(), minutes: 10, extendable: false, track_time: false },
                ],
            }],
        };
        let state = PomodoroState {
            active_session_id: session_id,
            current_part_index: 2,   // 0-based → "Part 3"
            remaining_seconds: 600,
            settings,
            running: false,
            paused: false,
            overtime_tracked_seconds: 0,
            is_docked: false,
            test_mode: false,
        };
        let tick = build_tick(&state, 0);
        assert_eq!(tick.part_name, "Part 3");
        assert_eq!(tick.part_index, 2);
    }

    // ---- validate_sessions ----

    #[test]
    fn validate_sessions_empty() {
        assert!(validate_sessions(&[]).is_err());
    }

    #[test]
    fn validate_sessions_too_many() {
        let too_many: Vec<Session> = (0..6)
            .map(|i| Session { id: format!("s{}", i), name: format!("S{}", i), parts: vec![] })
            .collect();
        assert!(validate_sessions(&too_many).is_err());
    }

    #[test]
    fn validate_sessions_five_ok() {
        let five: Vec<Session> = (0..5)
            .map(|i| Session {
                id: format!("s{}", i),
                name: format!("S{}", i),
                parts: vec![SessionPart { name: "P".into(), minutes: 1, extendable: false, track_time: false }],
            })
            .collect();
        assert!(validate_sessions(&five).is_ok());
    }

    #[test]
    fn validate_empty_session_name() {
        let sessions = vec![Session {
            id: "x".into(),
            name: "   ".into(),
            parts: vec![SessionPart { name: "P".into(), minutes: 1, extendable: false, track_time: false }],
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_zero_parts() {
        let sessions = vec![Session { id: "x".into(), name: "S".into(), parts: vec![] }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_eleven_parts() {
        let sessions = vec![Session {
            id: "x".into(),
            name: "S".into(),
            parts: (0..11)
                .map(|i| SessionPart { name: format!("P{}", i), minutes: 1, extendable: false, track_time: false })
                .collect(),
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_ten_parts_ok() {
        let sessions = vec![Session {
            id: "x".into(),
            name: "S".into(),
            parts: (0..10)
                .map(|i| SessionPart { name: format!("P{}", i), minutes: 1, extendable: false, track_time: false })
                .collect(),
        }];
        assert!(validate_sessions(&sessions).is_ok());
    }

    #[test]
    fn validate_minutes_zero() {
        let sessions = vec![Session {
            id: "x".into(),
            name: "S".into(),
            parts: vec![SessionPart { name: "P".into(), minutes: 0, extendable: false, track_time: false }],
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_minutes_121() {
        let sessions = vec![Session {
            id: "x".into(),
            name: "S".into(),
            parts: vec![SessionPart { name: "P".into(), minutes: 121, extendable: false, track_time: false }],
        }];
        assert!(validate_sessions(&sessions).is_err());
    }

    #[test]
    fn validate_minutes_120_ok() {
        let sessions = vec![Session {
            id: "x".into(),
            name: "S".into(),
            parts: vec![SessionPart { name: "P".into(), minutes: 120, extendable: false, track_time: false }],
        }];
        assert!(validate_sessions(&sessions).is_ok());
    }

    #[test]
    fn validate_valid_config() {
        let sessions = vec![
            Session {
                id: "a".into(),
                name: "Pomodoro".into(),
                parts: vec![
                    SessionPart { name: "Work".into(), minutes: 25, extendable: false, track_time: true },
                    SessionPart { name: "Break".into(), minutes: 5, extendable: false, track_time: false },
                ],
            },
            Session {
                id: "b".into(),
                name: "Play / Break".into(),
                parts: vec![
                    SessionPart { name: "Play".into(), minutes: 25, extendable: true, track_time: false },
                    SessionPart { name: "Break".into(), minutes: 10, extendable: false, track_time: false },
                ],
            },
        ];
        assert!(validate_sessions(&sessions).is_ok());
    }

    // ---- stop_tracked_seconds ----

    #[test]
    fn stop_tracked_when_disabled() {
        // track_time=false — never record.
        assert_eq!(stop_tracked_seconds(false, 300, false, 200, 0), None);
    }

    #[test]
    fn stop_tracked_mid_session() {
        // track_time=true, 500s elapsed (1000s remaining).
        assert_eq!(stop_tracked_seconds(true, 1500, false, 1000, 0), Some(500));
    }

    #[test]
    fn stop_tracked_no_elapsed() {
        // Just started, no time elapsed.
        assert_eq!(stop_tracked_seconds(true, 1500, false, 1500, 0), None);
    }

    #[test]
    fn stop_tracked_fully_elapsed() {
        // Timer hit zero exactly (non-extendable).
        assert_eq!(stop_tracked_seconds(true, 1500, false, 0, 0), Some(1500));
    }

    #[test]
    fn stop_tracked_paused_with_overtime() {
        // Paused in overtime, accumulated 30 overtime seconds.
        assert_eq!(stop_tracked_seconds(true, 1500, true, -30, 30), Some(30));
    }

    #[test]
    fn stop_tracked_paused_no_overtime() {
        // Paused exactly at zero, no overtime accumulated yet.
        assert_eq!(stop_tracked_seconds(true, 1500, true, 0, 0), None);
    }

    #[test]
    fn stop_tracked_remaining_negative_not_paused() {
        // Edge case: negative remaining without paused flag (shouldn't
        // happen in practice, but function handles it gracefully).
        let result = stop_tracked_seconds(true, 1500, false, -10, 0);
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
                    id: "sid-a".into(),
                    name: "Pomodoro".into(),
                    parts: vec![
                        PartDef { name: "Work".into(), minutes: 25, extendable: true, track_time: true },
                        PartDef { name: "Break".into(), minutes: 5, extendable: false, track_time: false },
                    ],
                },
                SessionDef {
                    id: "sid-b".into(),
                    name: "Deep Focus".into(),
                    parts: vec![
                        PartDef { name: "Focus".into(), minutes: 50, extendable: true, track_time: false },
                        PartDef { name: "Rest".into(), minutes: 10, extendable: false, track_time: false },
                    ],
                },
            ]),
            active_session: Some(serde_json::Value::String("sid-b".into())),
            ..Default::default()
        };
        let (settings, active) = settings_from_file(file);
        assert_eq!(settings.sessions.len(), 2);
        assert_eq!(settings.sessions[0].parts[0].extendable, true);
        assert_eq!(settings.sessions[0].parts[0].track_time, true);
        assert_eq!(settings.sessions[1].name, "Deep Focus");
        assert_eq!(settings.sessions[1].parts[0].minutes, 50);
        assert_eq!(settings.sessions[1].parts[1].minutes, 10);
        assert_eq!(active, "sid-b");
    }

    #[test]
    fn settings_from_file_active_index_clamped() {
        // Old numeric active index beyond bounds → fall back to first session.
        let file = SettingsFile {
            sessions: Some(vec![
                SessionDef {
                    id: "a1".into(),
                    name: "A".into(),
                    parts: vec![PartDef { name: "X".into(), minutes: 1, extendable: false, track_time: false }],
                },
                SessionDef {
                    id: "b2".into(),
                    name: "B".into(),
                    parts: vec![PartDef { name: "Y".into(), minutes: 1, extendable: false, track_time: false }],
                },
            ]),
            active_session: Some(serde_json::Value::Number(99.into())),
            ..Default::default()
        };
        let (settings, active) = settings_from_file(file);
        assert_eq!(active, "a1"); // out of bounds → first session UUID
        assert_eq!(settings.sessions.len(), 2);
    }

    #[test]
    fn settings_from_file_new_format_no_active() {
        // active_session missing from file → default to first session's UUID.
        let file = SettingsFile {
            sessions: Some(vec![
                SessionDef {
                    id: "solo-id".into(),
                    name: "Solo".into(),
                    parts: vec![PartDef { name: "Task".into(), minutes: 30, extendable: false, track_time: false }],
                },
            ]),
            active_session: None,
            ..Default::default()
        };
        let (_, active) = settings_from_file(file);
        assert_eq!(active, "solo-id");
    }

    #[test]
    fn settings_from_file_empty_defaults() {
        // Completely empty SettingsFile → PomodoroSettings::default().
        let file = SettingsFile::default();
        let (settings, active) = settings_from_file(file);
        assert!(!active.is_empty(), "should get a UUID from defaults");
        assert_eq!(settings.sessions.len(), 1);
        assert_eq!(settings.sessions[0].name, "Pomodoro");
        assert_eq!(settings.sessions[0].parts.len(), 2);
        assert_eq!(settings.sessions[0].parts[0].name, "Work");
        assert_eq!(settings.sessions[0].parts[0].minutes, 25);
        assert!(settings.sessions[0].parts[0].track_time);
        assert_eq!(settings.sessions[0].parts[1].name, "Break");
        assert_eq!(settings.sessions[0].parts[1].minutes, 5);
        assert!(!settings.sessions[0].parts[1].track_time);
    }

    // ---- JSON round-trip ----

    #[test]
    fn json_roundtrip_preserves_extendable() {
        // Session → JSON → Session — all fields survive, especially extendable.
        let original = Session {
            id: "json-rt-1".into(),
            name: "Test".into(),
            parts: vec![
                SessionPart { name: "Focus".into(), minutes: 45, extendable: true, track_time: true },
                SessionPart { name: "Break".into(), minutes: 15, extendable: false, track_time: false },
            ],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.id, "json-rt-1");
        assert_eq!(parsed.name, "Test");
        assert_eq!(parsed.parts.len(), 2);
        assert_eq!(parsed.parts[0].name, "Focus");
        assert_eq!(parsed.parts[0].minutes, 45);
        assert_eq!(parsed.parts[0].extendable, true);
        assert_eq!(parsed.parts[0].track_time, true);
        assert_eq!(parsed.parts[1].name, "Break");
        assert_eq!(parsed.parts[1].minutes, 15);
        assert_eq!(parsed.parts[1].extendable, false);
        assert_eq!(parsed.parts[1].track_time, false);
    }

    #[test]
    fn json_deserialize_missing_extendable_defaults_false() {
        // If the JSON omits "extendable" and "track_time", defaults kick in.
        let json = r#"{"name":"Work","minutes":25}"#;
        let part: SessionPart = serde_json::from_str(json).expect("deserialize");
        assert_eq!(part.name, "Work");
        assert_eq!(part.minutes, 25);
        assert!(!part.extendable, "missing extendable should default to false");
        assert!(!part.track_time, "missing track_time should default to false");
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
    fn json_deserialize_track_time_true() {
        let json = r#"{"name":"Work","minutes":25,"extendable":false,"track_time":true}"#;
        let part: SessionPart = serde_json::from_str(json).expect("deserialize");
        assert_eq!(part.name, "Work");
        assert_eq!(part.minutes, 25);
        assert!(part.track_time);
    }

    #[test]
    fn json_settings_file_roundtrip() {
        // Full SettingsFile → JSON → SettingsFile round-trip.
        let file = SettingsFile {
            sessions: Some(vec![
                SessionDef {
                    id: "roundtrip-sid".into(),
                    name: "Pomodoro".into(),
                    parts: vec![
                        PartDef { name: "Work".into(), minutes: 30, extendable: true, track_time: true },
                        PartDef { name: "Break".into(), minutes: 10, extendable: false, track_time: false },
                    ],
                },
            ]),
            active_session: Some(serde_json::Value::Number(0.into())),
            ..Default::default()
        };
        let json = serde_json::to_string(&file).expect("serialize");
        let parsed: SettingsFile = serde_json::from_str(&json).expect("deserialize");
        let sessions = parsed.sessions.expect("sessions present");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "roundtrip-sid");
        assert_eq!(sessions[0].name, "Pomodoro");
        assert_eq!(sessions[0].parts[0].minutes, 30);
        assert_eq!(sessions[0].parts[0].extendable, true);
        assert_eq!(sessions[0].parts[0].track_time, true);
        assert_eq!(sessions[0].parts[1].name, "Break");
        assert_eq!(sessions[0].parts[1].minutes, 10);
        assert_eq!(sessions[0].parts[1].extendable, false);
        assert_eq!(sessions[0].parts[1].track_time, false);
    }
}
