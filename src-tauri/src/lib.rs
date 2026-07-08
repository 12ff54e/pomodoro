use tauri::Manager;
use tauri_plugin_store::StoreExt;

mod timer;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            // Load persisted settings (or defaults).
            let settings = {
                let store = app.store("settings.json").unwrap_or_else(|_| {
                    // Should not happen, but fall back to a sensible default.
                    app.store("settings.json")
                        .expect("store plugin must be available")
                });
                let work = store
                    .get("workMinutes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(25);
                let brk = store
                    .get("breakMinutes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5);
                timer::PomodoroSettings {
                    work_minutes: work,
                    break_minutes: brk,
                }
            };

            let state = timer::PomodoroState {
                phase: timer::TimerPhase::Work,
                remaining_seconds: settings.work_minutes * 60,
                settings: settings.clone(),
                running: false,
            };
            app.manage(std::sync::Mutex::new(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            timer::get_state,
            timer::start_timer,
            timer::stop_timer,
            timer::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
