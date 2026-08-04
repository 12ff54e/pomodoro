use tauri::Manager;

mod timer;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let (settings, active_id) = timer::load_settings();
            let test_mode = std::env::var("POMODORO_TEST_MODE")
                .map(|v| v == "1")
                .unwrap_or(false);
            let active_idx = settings
                .sessions
                .iter()
                .position(|s| s.id == active_id)
                .unwrap_or(0);
            let remaining =
                timer::minutes_to_seconds(settings.sessions[active_idx].parts[0].minutes, test_mode);
            let shortcuts = settings.shortcuts.clone();
            let state = timer::PomodoroState {
                active_session_id: active_id,
                current_part_index: 0,
                remaining_seconds: remaining,
                settings,
                running: false,
                paused: false,
                manual_pause: false,
                overtime_tracked_seconds: 0,
                is_docked: false,
                is_settings_open: false,
                test_mode,
            };
            app.manage(std::sync::Mutex::new(state));

            // Register global keyboard shortcuts from persisted config.
            timer::register_shortcuts(app.app_handle(), &shortcuts);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            timer::get_state,
            timer::get_daily_total,
            timer::get_settings,
            timer::start_timer,
            timer::stop_timer,
            timer::continue_timer,
            timer::pause_timer,
            timer::resume_timer,
            timer::next_part,
            timer::update_settings,
            timer::switch_session,
            timer::toggle_dock_mode,
            timer::get_dock_state,
            timer::set_settings_open,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
