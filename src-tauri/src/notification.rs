//! Android notification service bridge.

use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};

use crate::timer::TimerTick;

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn init(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
    #[cfg(target_os = "android")]
    android::init_jvm();
}

pub enum ServiceEvent {
    Start { tick: TimerTick },
    PartUpdated { tick: TimerTick },
    Stop,
}

pub fn notify_service(_app: &AppHandle, _event: ServiceEvent) {
    #[cfg(target_os = "android")]
    {
        eprintln!("[notif] notify_service called on Android");
        android::notify(_event);
    }
    #[cfg(not(target_os = "android"))]
    let _ = (_app, _event);
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use jni::objects::JObject;
    use jni::JNIEnv;
    use std::sync::Mutex;
    use tauri::Manager;

    static JVM: std::sync::OnceLock<jni::JavaVM> = std::sync::OnceLock::new();

    pub(super) fn init_jvm() {
        // JVM is captured in JNI_OnLoad below.
        eprintln!("[notif] init_jvm: JVM is_set={}", JVM.get().is_some());
    }

    /// Captures the JavaVM when the native library is loaded.
    /// Called automatically by the Android runtime.
    #[no_mangle]
    pub extern "system" fn JNI_OnLoad(
        vm: jni::JavaVM,
        _reserved: *mut std::ffi::c_void,
    ) -> jni::sys::jint {
        eprintln!("[notif] JNI_OnLoad called!");
        let _ = JVM.set(vm);
        jni::sys::JNI_VERSION_1_6
    }

    /// Get a JNI environment, attaching the current thread if needed.
    fn with_jni<F: FnOnce(&mut JNIEnv)>(f: F) {
        eprintln!("[notif] with_jni entry");
        let Some(vm) = JVM.get() else {
            eprintln!("[notif] JVM not set!");
            return;
        };
        let mut env = match vm.attach_current_thread() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[notif] attach failed: {:?}", e);
                return;
            }
        };
        eprintln!("[notif] calling closure");
        f(&mut env);
        eprintln!("[notif] closure returned");
    }

    pub(super) fn notify(event: ServiceEvent) {
        match event {
            ServiceEvent::Start { tick } => {
                eprintln!("[notif] START: rem={} part={}", tick.remaining_seconds, tick.part_name);
                let s_name = tick.session_name.clone();
                let p_name = tick.part_name.clone();
                let rem = tick.remaining_seconds;
                let p_idx = tick.part_index as i32;
                let paused = tick.paused;

                with_jni(|env| {
                    eprintln!("[notif] inside with_jni for start");
                    let cls = match env.find_class("com/pomodoro/app/TimerForegroundService") {
                        Ok(c) => c,
                        Err(e) => { eprintln!("[notif] find_class start: {e:?}"); return; }
                    };
                    let s_str = env.new_string(&s_name).unwrap();
                    let p_str = env.new_string(&p_name).unwrap();
                    let s_obj = JObject::from(s_str);
                    let p_obj = JObject::from(p_str);
                    let args: [jni::objects::JValue; 5] = [
                        jni::objects::JValue::Long(rem),
                        jni::objects::JValue::Object(&s_obj),
                        jni::objects::JValue::Object(&p_obj),
                        jni::objects::JValue::Int(p_idx),
                        jni::objects::JValue::Bool(if paused { 1 } else { 0 }),
                    ];
                    match env.call_static_method(&cls, "start", "(JLjava/lang/String;Ljava/lang/String;IZ)V", &args) {
                        Ok(_) => eprintln!("[notif] start() OK"),
                        Err(e) => eprintln!("[notif] start() failed: {e:?}"),
                    }
                });
            }
            ServiceEvent::PartUpdated { tick } => {
                let p_name = tick.part_name.clone();
                let rem = tick.remaining_seconds;
                let p_idx = tick.part_index as i32;
                let paused = tick.paused;

                with_jni(|env| {
                    let cls = match env.find_class("com/pomodoro/app/TimerForegroundService") {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    let p_str = env.new_string(&p_name).unwrap();
                    let p_obj = JObject::from(p_str);
                    let args: [jni::objects::JValue; 4] = [
                        jni::objects::JValue::Long(rem),
                        jni::objects::JValue::Object(&p_obj),
                        jni::objects::JValue::Int(p_idx),
                        jni::objects::JValue::Bool(if paused { 1 } else { 0 }),
                    ];
                    let _ = env.call_static_method(&cls, "update", "(JLjava/lang/String;IZ)V", &args);
                });
            }
            ServiceEvent::Stop => {
                with_jni(|env| {
                    let cls = match env.find_class("com/pomodoro/app/TimerForegroundService") {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    let _ = env.call_static_method(&cls, "stop", "()V", &[]);
                });
            }
        }
    }

    // ---- JNI export: StopTimerReceiver → Rust ----------------------------

    #[no_mangle]
    pub extern "system" fn Java_com_pomodoro_app_StopTimerReceiver_nativeStopTimer(
        _env: JNIEnv,
        _this: jni::objects::JObject,
    ) {
        let Some(handle) = APP_HANDLE.get() else { return };
        let state = handle.state::<Mutex<crate::timer::PomodoroState>>();
        let mut s = match state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        if !s.running { return; }

        let sessions = s.settings.sessions.clone();
        let idx = crate::timer::find_session_index(&sessions, &s.active_session_id).unwrap_or(0);
        let part = &sessions[idx].parts[s.current_part_index];
        let full_seconds = crate::timer::minutes_to_seconds(part.minutes, s.test_mode) as u64;
        let session_id = s.active_session_id.clone();
        let part_index = s.current_part_index;

        if let Some(seconds) = crate::timer::stop_tracked_seconds(
            part.track_time, full_seconds, s.paused,
            s.remaining_seconds, s.overtime_tracked_seconds,
        ) {
            drop(s);
            crate::timer::add_record_seconds(&session_id, part_index, seconds);
            s = state.lock().unwrap();
        }

        s.running = false;
        s.paused = false;
        s.overtime_tracked_seconds = 0;
        s.current_part_index = 0;
        let idx = crate::timer::find_session_index(&s.settings.sessions, &s.active_session_id).unwrap_or(0);
        s.remaining_seconds = crate::timer::minutes_to_seconds(
            s.settings.sessions[idx].parts[0].minutes, s.test_mode,
        );
        let daily = crate::timer::load_daily_total();
        let tick = crate::timer::build_tick(&s, daily);
        drop(s);
        let _ = handle.emit("timer-tick", &tick);
        notify_service(handle, ServiceEvent::Stop);
    }
}
