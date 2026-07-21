package com.pomodoro.app

import android.app.Application
import android.content.Context

/**
 * Global application-context holder.
 *
 * The [TimerForegroundService] companion object needs a [Context] to
 * start the foreground service, but companion methods called from
 * Rust via JNI don't have one.  [App] is set during [MainActivity.onCreate].
 */
object App {
    @Volatile
    private var appContext: Context? = null

    fun init(context: Context) {
        if (appContext == null) {
            appContext = context.applicationContext
        }
    }

    fun getContext(): Context =
        appContext ?: throw IllegalStateException("App context not initialized")
}
