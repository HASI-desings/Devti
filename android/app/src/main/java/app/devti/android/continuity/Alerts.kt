package app.devti.android.continuity

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.devti.android.MainActivity
import app.devti.android.R

/**
 * Notification channels and the one-line alerts Devti posts on the phone.
 *
 * Channels are separate so the user can silence one kind (say, "Files received")
 * from Android's settings without losing the others.
 */
object Alerts {
    const val CONNECTION = "devti.connection"
    const val ALERTS = "devti.alerts"
    const val FILES = "devti.files"
    const val RING = "devti.ring"
    const val MIRRORED = "devti.mirrored"

    const val ID_CONNECTION = 1
    const val ID_RING = 2
    const val ID_SCREEN = 3
    private const val ID_SCREEN_REQUEST = 4
    private const val ID_BATTERY_BASE = 1000
    private const val ID_FILE_BASE = 2000
    private const val ID_MIRROR_BASE = 3000

    fun createChannels(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = context.getSystemService(NotificationManager::class.java)
        fun ch(id: String, name: Int, desc: Int, importance: Int, silent: Boolean = false) =
            NotificationChannel(id, context.getString(name), importance).apply {
                description = context.getString(desc)
                if (silent) setSound(null, null)
            }
        nm.createNotificationChannels(
            listOf(
                ch(CONNECTION, R.string.channel_connection, R.string.channel_connection_desc, NotificationManager.IMPORTANCE_LOW),
                ch(ALERTS, R.string.channel_alerts, R.string.channel_alerts_desc, NotificationManager.IMPORTANCE_DEFAULT),
                ch(FILES, R.string.channel_files, R.string.channel_files_desc, NotificationManager.IMPORTANCE_DEFAULT),
                // Devti plays the ringing sound itself (alarm volume), so the channel is silent.
                ch(RING, R.string.channel_ring, R.string.channel_ring_desc, NotificationManager.IMPORTANCE_HIGH, silent = true),
                ch(MIRRORED, R.string.channel_mirrored, R.string.channel_mirrored_desc, NotificationManager.IMPORTANCE_DEFAULT),
            ),
        )
    }

    fun canPost(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    fun openApp(context: Context, request: Int = 0): PendingIntent = PendingIntent.getActivity(
        context,
        request,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    fun lowBattery(context: Context, deviceId: String, name: String, percent: Int) = post(
        context,
        ID_BATTERY_BASE + (deviceId.hashCode() and 0x3ff),
        NotificationCompat.Builder(context, ALERTS)
            .setSmallIcon(R.drawable.ic_stat_devti)
            .setContentTitle(context.getString(R.string.low_battery_title, name, percent))
            .setContentText(context.getString(R.string.low_battery_body))
            .setContentIntent(openApp(context))
            .setAutoCancel(true),
    )

    fun fileReceived(context: Context, transferId: Long, name: String, from: String, open: Uri?, mime: String?) {
        val tap = open?.let {
            val view = Intent(Intent.ACTION_VIEW).setDataAndType(it, mime ?: "*/*")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            PendingIntent.getActivity(
                context,
                transferId.toInt(),
                Intent.createChooser(view, name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        } ?: openApp(context)
        post(
            context,
            ID_FILE_BASE + (transferId.toInt() and 0x3ff),
            NotificationCompat.Builder(context, FILES)
                .setSmallIcon(R.drawable.ic_stat_devti)
                .setContentTitle(context.getString(R.string.file_received_title, name))
                .setContentText(context.getString(R.string.file_received_body, from))
                .setContentIntent(tap)
                .setAutoCancel(true),
        )
    }

    /** A notification mirrored from another phone or tablet in the ecosystem. */
    fun mirrored(context: Context, key: String, title: String, text: String, silent: Boolean) = post(
        context,
        mirrorId(key),
        NotificationCompat.Builder(context, MIRRORED)
            .setSmallIcon(R.drawable.ic_stat_devti)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setSilent(silent)
            .setContentIntent(openApp(context))
            .setAutoCancel(true),
    )

    /** "Studio Laptop wants to see your screen" — tapping it opens Android's consent dialog. */
    fun screenRequest(context: Context, fromName: String, consent: Intent) = post(
        context,
        ID_SCREEN_REQUEST,
        NotificationCompat.Builder(context, RING)
            .setSmallIcon(R.drawable.ic_stat_devti)
            .setContentTitle(context.getString(R.string.screen_request_title, fromName))
            .setContentText(context.getString(R.string.screen_request_body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setAutoCancel(true)
            .setTimeoutAfter(60_000)
            .setContentIntent(
                PendingIntent.getActivity(context, 9, consent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
            ),
    )

    fun cancelMirrored(context: Context, key: String) = NotificationManagerCompat.from(context).cancel(mirrorId(key))

    private fun mirrorId(key: String) = ID_MIRROR_BASE + (key.hashCode() and 0xffff)

    @Suppress("MissingPermission") // checked by canPost()
    fun post(context: Context, id: Int, builder: NotificationCompat.Builder) {
        if (!canPost(context)) return
        NotificationManagerCompat.from(context).notify(id, builder.build())
    }
}
