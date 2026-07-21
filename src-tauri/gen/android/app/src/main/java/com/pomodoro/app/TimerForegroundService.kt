package com.pomodoro.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlin.math.abs

class TimerForegroundService : Service() {

    companion object {
        const val CHANNEL_ID = "pomodoro_timer"
        const val NOTIFICATION_ID = 1
        const val ACTION_STOP = "com.pomodoro.app.STOP_TIMER"

        @Volatile var isRunning = false
        @Volatile var remainingSeconds: Long = 0
        @Volatile var sessionName: String = ""
        @Volatile var partName: String = ""
        @Volatile var partIndex: Int = 0
        @Volatile var paused: Boolean = false

        @Volatile var instance: TimerForegroundService? = null

        // Wall-clock anchors so the countdown never drifts.
        private var startTimeMillis: Long = 0
        private var startRemainingSeconds: Long = 0

        /** Called from Rust via JNI when the timer starts. */
        @JvmStatic
        fun start(remSecs: Long, sName: String, pName: String,
                  pIdx: Int, isPaused: Boolean) {
            Log.i("Pomodoro", "TimerFgSvc.start: rem=$remSecs part=$pName paused=$isPaused")

            // Reset wall-clock anchor.
            startTimeMillis = SystemClock.elapsedRealtime()
            startRemainingSeconds = remSecs

            remainingSeconds = remSecs
            sessionName = sName
            partName = pName
            partIndex = pIdx
            paused = isPaused
            isRunning = true

            val ctx = App.getContext()
            val intent = Intent(ctx, TimerForegroundService::class.java)
            ContextCompat.startForegroundService(ctx, intent)
            Log.i("Pomodoro", "TimerFgSvc.start: service started")
        }

        /** Called from Rust via JNI on part transitions / pause changes.
         *  Resets the wall-clock anchor so the countdown stays accurate. */
        @JvmStatic
        fun update(remSecs: Long, pName: String, pIdx: Int, isPaused: Boolean) {
            Log.d("Pomodoro", "TimerFgSvc.update: rem=$remSecs part=$pName paused=$isPaused")

            // Re-anchor so elapsed time is relative to the new part.
            startTimeMillis = SystemClock.elapsedRealtime()
            startRemainingSeconds = remSecs

            remainingSeconds = remSecs
            partName = pName
            partIndex = pIdx
            paused = isPaused

            // Force immediate notification refresh.
            instance?.let { svc ->
                val nm = svc.getSystemService(Context.NOTIFICATION_SERVICE)
                    as NotificationManager
                nm.notify(NOTIFICATION_ID, svc.buildNotification(svc))
            }
        }

        /** Called from Rust via JNI when the timer stops. */
        @JvmStatic
        fun stop() {
            isRunning = false
            val ctx = App.getContext()
            instance?.let {
                it.stopForeground(STOP_FOREGROUND_REMOVE)
                it.stopSelf()
            }
            // If the service wasn't running, ensure the notification is gone.
            val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE)
                as NotificationManager
            nm.cancel(NOTIFICATION_ID)
        }
    }

    private val handler = Handler(Looper.getMainLooper())
    private var tickRunnable: Runnable? = null

    override fun onCreate() {
        super.onCreate()
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.timer_channel_name),
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = getString(R.string.timer_channel_desc)
            setShowBadge(false)
        }
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        nm.createNotificationChannel(channel)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i("Pomodoro", "TimerFgSvc.onStartCommand: rem=$remainingSeconds paused=$paused")
        instance = this
        val notification = buildNotification(this)
        startForeground(NOTIFICATION_ID, notification)
        startTicking()
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        stopTicking()
        instance = null
        super.onDestroy()
    }

    // ---- Countdown loop (wall-clock based, no drift) ------------------------

    private fun startTicking() {
        // Compute remaining seconds from wall clock when the runnable fires,
        // NOT by decrementing — immune to Handler.postDelayed drift.
        tickRunnable = object : Runnable {
            override fun run() {
                if (!isRunning) return

                val elapsed = (SystemClock.elapsedRealtime() - startTimeMillis) / 1000
                val newRemaining = if (paused) {
                    // Overtime: keep counting into negative.
                    startRemainingSeconds - elapsed
                } else {
                    startRemainingSeconds - elapsed
                }

                // Only update when the displayed second changes.
                if (newRemaining != remainingSeconds) {
                    remainingSeconds = newRemaining.coerceAtMost(remainingSeconds)
                    val nm = getSystemService(Context.NOTIFICATION_SERVICE)
                        as NotificationManager
                    nm.notify(NOTIFICATION_ID, buildNotification(this@TimerForegroundService))
                }

                // If the timer reached zero and it's an extendable part (paused),
                // the notification stays visible; Rust handles the state logic.

                handler.postDelayed(this, 500) // tick twice per second for precision
            }
        }
        Log.i("Pomodoro", "TimerFgSvc.startTicking: rem=$remainingSeconds")
        handler.post(tickRunnable!!)
    }

    private fun stopTicking() {
        tickRunnable?.let { handler.removeCallbacks(it) }
        tickRunnable = null
    }

    // ---- Notification builder ---------------------------------------------

    internal fun buildNotification(ctx: Context): Notification {
        val stopIntent = Intent(ctx, StopTimerReceiver::class.java).apply {
            action = ACTION_STOP
        }
        val stopPendingIntent = PendingIntent.getBroadcast(
            ctx, 0, stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val timeText = formatTime(remainingSeconds)
        val contentText = if (paused) {
            "$sessionName — $partName (Overtime)"
        } else {
            "$sessionName — $partName"
        }

        return NotificationCompat.Builder(ctx, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle("$timeText remaining")
            .setContentText(contentText)
            .setOngoing(true)
            .setSilent(true)
            .addAction(0, "Stop", stopPendingIntent)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .build()
    }

    private fun formatTime(totalSeconds: Long): String {
        val absSecs = abs(totalSeconds)
        val mins = absSecs / 60
        val secs = absSecs % 60
        val sign = if (totalSeconds < 0) "-" else ""
        return "$sign$mins:${secs.toString().padStart(2, '0')}"
    }
}
