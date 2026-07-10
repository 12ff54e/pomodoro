use tauri::Manager;

mod timer;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let (settings, active_index) = timer::load_settings();
            let remaining = (settings.sessions[active_index].parts[0].minutes * 60) as i64;
            let state = timer::PomodoroState {
                active_session_index: active_index,
                current_part_index: 0,
                remaining_seconds: remaining,
                settings,
                running: false,
                paused: false,
                overtime_work_seconds: 0,
            };
            app.manage(std::sync::Mutex::new(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            timer::get_state,
            timer::get_daily_total,
            timer::get_settings,
            timer::start_timer,
            timer::stop_timer,
            timer::continue_timer,
            timer::update_settings,
            timer::switch_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
