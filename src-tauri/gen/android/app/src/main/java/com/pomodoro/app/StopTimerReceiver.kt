package com.pomodoro.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Handles the Stop action from the persistent timer notification.
 *
 * Calls into the Rust native code via JNI to perform the same logic
 * as the `stop_timer` Tauri command.
 */
class StopTimerReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == TimerForegroundService.ACTION_STOP) {
            nativeStopTimer()
        }
    }

    /** Implemented in Rust (notification.rs). */
    private external fun nativeStopTimer()
}
