use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

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
            let state = timer::PomodoroState {
                active_session_id: active_id,
                current_part_index: 0,
                remaining_seconds: remaining,
                settings,
                running: false,
                paused: false,
                overtime_tracked_seconds: 0,
                is_docked: false,
                test_mode,
            };
            app.manage(std::sync::Mutex::new(state));

            // Register global keyboard shortcuts (work even when app is not focused).
            let start_shortcut = "CmdOrCtrl+Shift+S"
                .parse::<Shortcut>()
                .expect("invalid shortcut string CmdOrCtrl+Shift+S");
            if let Err(e) = app.global_shortcut().on_shortcut(
                start_shortcut,
                |app_handle, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = timer::start_timer(
                            app_handle.clone(),
                            app_handle.state::<std::sync::Mutex<timer::PomodoroState>>(),
                        );
                    }
                },
            ) {
                eprintln!("Failed to register global shortcut CmdOrCtrl+Shift+S: {}", e);
            }

            let stop_shortcut = "CmdOrCtrl+Shift+X"
                .parse::<Shortcut>()
                .expect("invalid shortcut string CmdOrCtrl+Shift+X");
            if let Err(e) = app.global_shortcut().on_shortcut(
                stop_shortcut,
                |app_handle, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = timer::stop_timer(
                            app_handle.clone(),
                            app_handle.state::<std::sync::Mutex<timer::PomodoroState>>(),
                        );
                    }
                },
            ) {
                eprintln!("Failed to register global shortcut CmdOrCtrl+Shift+X: {}", e);
            }

            let continue_shortcut = "CmdOrCtrl+Shift+C"
                .parse::<Shortcut>()
                .expect("invalid shortcut string CmdOrCtrl+Shift+C");
            if let Err(e) = app.global_shortcut().on_shortcut(
                continue_shortcut,
                |app_handle, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = timer::continue_timer(
                            app_handle.clone(),
                            app_handle.state::<std::sync::Mutex<timer::PomodoroState>>(),
                        );
                    }
                },
            ) {
                eprintln!("Failed to register global shortcut CmdOrCtrl+Shift+C: {}", e);
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
