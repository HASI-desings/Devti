package app.devti.android.core

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import app.devti.android.DevtiApp
import app.devti.android.MainActivity
import app.devti.android.R
import kotlinx.coroutines.launch

/**
 * Keeps Devti connected while the app is in the background. Android requires a visible
 * notification for this; it names the Hub so the user always knows why it is there.
 */
class DevtiService : LifecycleService() {
    override fun onCreate() {
        super.onCreate()
        createChannel()
        val core = (application as DevtiApp).core
        core.start()
        ServiceCompat.startForeground(
            this,
            NOTIFICATION_ID,
            notification("Keeping your devices connected"),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE else 0,
        )
        lifecycleScope.launch {
            core.state.collect { s ->
                val hub = s.hub
                val text = when {
                    s.local?.paused == true -> "Paused. Open Devti to resume."
                    hub == null -> "Not paired with a computer yet"
                    hub.online -> "Connected to ${hub.info.name}"
                    else -> "Waiting for ${hub.info.name}"
                }
                getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(text))
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        return START_STICKY
    }

    private fun notification(text: String): Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.mipmap.ic_launcher_foreground)
            .setContentTitle("Devti")
            .setContentText(text)
            .setOngoing(true)
            .setSilent(true)
            .setContentIntent(open)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .build()
    }

    private fun createChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(CHANNEL_ID, getString(R.string.channel_connection), NotificationManager.IMPORTANCE_LOW)
        channel.description = getString(R.string.channel_connection_desc)
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    companion object {
        private const val CHANNEL_ID = "devti.connection"
        private const val NOTIFICATION_ID = 1

        fun start(context: Context) {
            ContextCompat.startForegroundService(context, Intent(context, DevtiService::class.java))
        }
    }
}
