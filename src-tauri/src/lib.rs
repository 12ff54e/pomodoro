use tauri::Manager;

mod timer;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let settings = timer::load_settings();
            let session_type = timer::load_session_type();
            let state = timer::PomodoroState {
                session_type,
                phase: match session_type {
                    timer::SessionType::Pomodoro => timer::TimerPhase::Work,
                    timer::SessionType::PlayBreak => timer::TimerPhase::Play,
                },
                remaining_seconds: match session_type {
                    timer::SessionType::Pomodoro => settings.work_minutes * 60,
                    timer::SessionType::PlayBreak => settings.play_minutes * 60,
                },
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
            timer::switch_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
