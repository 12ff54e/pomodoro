// E2E tests — each test creates a fresh WebDriver session (fresh app
// instance) and cleans up after itself.  Run with --test-threads=1.
//
// All tests assume POMODORO_TEST_MODE=1 was set when building the app,
// so that "minutes" are treated as seconds (25s work, 5s break).

use crate::webdriver::{self, WebDriverClient};
use std::time::Duration;

fn init() -> WebDriverClient {
    let url = webdriver::driver_url();
    let path = webdriver::app_path();
    WebDriverClient::new_session(&url, &path)
        .expect("Failed to create WebDriver session — is tauri-driver running?")
}

/// Helper: get the text content of an element found by CSS.
fn text(client: &WebDriverClient, css: &str) -> String {
    let id = client.find_element(css).unwrap();
    client.element_text(&id).unwrap()
}

/// Helper: click an element found by CSS.
fn click(client: &WebDriverClient, css: &str) {
    let id = client.find_element(css).unwrap();
    client.element_click(&id).unwrap();
}

// ---------------------------------------------------------------------------

#[test]
fn timer_starts_and_ticks_down() {
    let c = init();

    // In test mode, 25 "minutes" = 25 seconds → display "00:25".
    let initial = text(&c, "#timer");
    assert_eq!(initial, "00:25", "initial timer should be 00:25 in test mode");

    assert_eq!(text(&c, "#toggle-btn"), "Start");
    assert_eq!(text(&c, "#phase"), "WORK");

    // Click Start.
    click(&c, "#toggle-btn");

    // Wait a couple of seconds, then check the timer has decreased.
    std::thread::sleep(Duration::from_secs(2));
    let mid = text(&c, "#timer");
    let mid_secs: i64 = parse_timer(&mid);
    assert!(
        mid_secs < 25 && mid_secs > 0,
        "timer should have decreased from 25s, got '{}'", mid
    );

    // Stop button should be shown.
    assert_eq!(text(&c, "#toggle-btn"), "Stop");

    // Click Stop.
    click(&c, "#toggle-btn");

    // Timer should reset to initial value.
    let reset = text(&c, "#timer");
    assert_eq!(reset, "00:25", "timer should reset after stop");

    c.delete_session().unwrap();
}

#[test]
fn non_extendable_part_auto_advances() {
    let c = init();

    // Start the timer.
    click(&c, "#toggle-btn");

    // Wait for the work part to complete (25s) + the break part to start.
    // Give it a generous timeout: 35 seconds.
    std::thread::sleep(Duration::from_secs(35));

    // Phase should now be BREAK (auto-advanced).
    let phase = text(&c, "#phase");
    assert_eq!(phase, "BREAK", "should have auto-advanced to Break");

    // Timer should show the break duration (5s in test mode).
    let timer = text(&c, "#timer");
    let t = parse_timer(&timer);
    assert!(t <= 5, "break timer should be <= 5s, got '{}'", timer);

    c.delete_session().unwrap();
}

#[test]
fn extendable_part_enters_overtime_and_continue_appears() {
    let c = init();

    // Open settings to make Work extendable.
    click(&c, "#settings-btn");

    // We need to check the extendable checkbox for the Work part.
    // The settings form was built dynamically — find the checkbox.
    let checkboxes = c.find_elements("input[type='checkbox']").unwrap();
    assert!(!checkboxes.is_empty(), "should have extendable checkboxes");
    // The first checkbox is for the first part (Work) in the first session.
    c.element_click(&checkboxes[0]).unwrap();

    // Save settings.
    click(&c, "#save-settings");

    // Brief wait for settings to close and UI to update.
    std::thread::sleep(Duration::from_millis(500));

    // Start the extended work part.
    click(&c, "#toggle-btn");

    // Wait for the work part to finish (25s in test mode). After it hits 0,
    // the timer should enter overtime (negative) and the Continue button
    // should appear.
    std::thread::sleep(Duration::from_secs(30));

    // Continue button should be visible.
    let continue_btn = c.find_element("#continue-btn").unwrap();
    let prop = c.element_property(&continue_btn, "className").unwrap();
    assert!(
        !prop.contains("hidden"),
        "Continue button should be visible in overtime, got class '{}'", prop
    );

    // Timer should show negative seconds (overtime).
    let timer = text(&c, "#timer");
    let t = parse_timer(&timer);
    assert!(t < 0, "timer should be negative during overtime, got '{}'", timer);

    // Click Continue to advance past the extendable part.
    click(&c, "#continue-btn");

    // Should have advanced to Break.
    std::thread::sleep(Duration::from_millis(500));
    let phase = text(&c, "#phase");
    assert_eq!(phase, "BREAK", "should have advanced to Break after Continue");

    // Continue button should be hidden again.
    let prop2 = c.element_property(&continue_btn, "className").unwrap();
    assert!(
        prop2.contains("hidden"),
        "Continue button should be hidden after advancing"
    );

    c.delete_session().unwrap();
}

#[test]
fn stop_records_work_time() {
    let c = init();

    // Read initial daily total.
    let initial_total = text(&c, "#daily-total");
    // e.g. "Today: 2h 30m" or "Today: 0m"

    // Start and let it run a few seconds.
    click(&c, "#toggle-btn");
    std::thread::sleep(Duration::from_secs(3));
    click(&c, "#toggle-btn"); // Stop

    // Daily total should have changed (increased or now non-zero).
    let new_total = text(&c, "#daily-total");
    assert_ne!(
        new_total, initial_total,
        "daily total should have changed after recording work time"
    );
    assert!(
        !new_total.contains("0m") || new_total != initial_total,
        "daily total should show recorded time, got '{}'", new_total
    );

    c.delete_session().unwrap();
}

#[test]
fn settings_panel_opens_edits_saves() {
    let c = init();

    // Open settings.
    click(&c, "#settings-btn");

    // Settings panel should be visible (find it, verify offsetParent is not null).
    let panel = c.find_element("#settings-panel").unwrap();
    let offset = c.element_property(&panel, "offsetParent").unwrap();
    assert_ne!(offset, "null", "settings panel should be visible");

    // Find the session name input and change it.
    let session_inputs = c.find_elements("#sessions-container input[type='text']").unwrap();
    assert!(!session_inputs.is_empty(), "should have session name input");

    // The first text input is the session name. Click it and type a new name.
    // We use execute_script to set the value directly (WebDriver "clear" +
    // "sendKeys" is fragile with Tauri's WebView).
    c.execute_script(
        r#"document.querySelector('#sessions-container input[type="text"]').value = 'E2E Session'"#,
        &[],
    ).unwrap();
    // Fire input event so the JS binding picks up the change.
    c.execute_script(
        r#"document.querySelector('#sessions-container input[type="text"]').dispatchEvent(new Event('input'))"#,
        &[],
    ).unwrap();

    // Save.
    click(&c, "#save-settings");

    // Brief wait for settings to close.
    std::thread::sleep(Duration::from_millis(500));

    // The session label should now show the new name.
    let label = text(&c, "#session-label");
    assert_eq!(label, "E2E Session", "session label should reflect saved name");

    c.delete_session().unwrap();
}

#[test]
fn session_switcher_works() {
    let c = init();

    // Default settings have 2 sessions: "Pomodoro" and "Deep Focus"
    let initial = text(&c, "#session-label");

    // Click the right arrow to switch to session 2.
    click(&c, "#session-right");

    std::thread::sleep(Duration::from_millis(500));
    let after_right = text(&c, "#session-label");
    assert_ne!(
        after_right, initial,
        "session label should change after switching right"
    );

    // Click left to go back.
    click(&c, "#session-left");

    std::thread::sleep(Duration::from_millis(500));
    let after_left = text(&c, "#session-label");
    assert_eq!(
        after_left, initial,
        "session label should return to initial after switching back"
    );

    c.delete_session().unwrap();
}

#[test]
fn dock_mode_toggles() {
    let c = init();

    // Verify body does NOT have 'docked' class initially.
    let docked = c
        .execute_script("return document.body.classList.contains('docked');", &[])
        .unwrap();
    assert_eq!(docked, serde_json::Value::Bool(false));

    // Click dock button.
    click(&c, "#dock-btn");

    std::thread::sleep(Duration::from_millis(500));

    // Body should now have 'docked' class.
    let docked = c
        .execute_script("return document.body.classList.contains('docked');", &[])
        .unwrap();
    assert_eq!(
        docked,
        serde_json::Value::Bool(true),
        "body should have 'docked' class after clicking dock"
    );

    // Click again to undock.
    click(&c, "#dock-btn");

    std::thread::sleep(Duration::from_millis(500));

    let undocked = c
        .execute_script("return document.body.classList.contains('docked');", &[])
        .unwrap();
    assert_eq!(
        undocked,
        serde_json::Value::Bool(false),
        "docked class should be removed after second click"
    );

    c.delete_session().unwrap();
}

// ---- Helpers ----

/// Parse a MM:SS or -MM:SS timer string into total seconds.
fn parse_timer(s: &str) -> i64 {
    let negative = s.starts_with('-');
    let parts: Vec<&str> = s.trim_start_matches('-').split(':').collect();
    if parts.len() != 2 {
        return 0;
    }
    let min: i64 = parts[0].parse().unwrap_or(0);
    let sec: i64 = parts[1].parse().unwrap_or(0);
    let total = min * 60 + sec;
    if negative { -total } else { total }
}
