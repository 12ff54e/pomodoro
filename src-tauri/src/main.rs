#![cfg_attr(all(not(debug_assertions), not(target_os = "android")), windows_subsystem = "windows")]

fn main() {
    app_lib::run();
}
