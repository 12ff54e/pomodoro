use tauri::Manager;

mod timer;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let (settings, active_index) = timer::load_settings();
            let is_docked = timer::load_dock_state();
            let remaining = (settings.sessions[active_index].parts[0].minutes * 60) as i64;
            let state = timer::PomodoroState {
                active_session_index: active_index,
                current_part_index: 0,
                remaining_seconds: remaining,
                settings,
                running: false,
                paused: false,
                overtime_work_seconds: 0,
                is_docked,
            };
            app.manage(std::sync::Mutex::new(state));

            // Apply docked window state on startup if persisted.
            if is_docked {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_decorations(false);
                    let _ = window.set_always_on_top(true);
                    let _ = window.set_resizable(true);
                    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                        420.0, 72.0,
                    )));
                    if let Ok(Some(monitor)) = window.primary_monitor() {
                        let phys = monitor.size();
                        let scale = monitor.scale_factor();
                        let logical_width = phys.width as f64 / scale;
                        let x = ((logical_width - 420.0) / 2.0).max(0.0);
                        let _ = window.set_position(tauri::Position::Logical(
                            tauri::LogicalPosition::new(x, 0.0),
                        ));
                    }
                }
            }

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
            timer::toggle_dock_mode,
            timer::get_dock_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
