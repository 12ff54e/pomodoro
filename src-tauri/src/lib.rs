use tauri::Manager;

mod timer;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let settings = timer::load_settings();
            let state = timer::PomodoroState {
                phase: timer::TimerPhase::Work,
                remaining_seconds: settings.work_minutes * 60,
                settings,
                running: false,
            };
            app.manage(std::sync::Mutex::new(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            timer::get_state,
            timer::get_daily_total,
            timer::start_timer,
            timer::stop_timer,
            timer::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
