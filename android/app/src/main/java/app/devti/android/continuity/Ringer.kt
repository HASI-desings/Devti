package app.devti.android.continuity

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.MediaPlayer
import android.media.RingtoneManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import app.devti.android.DevtiApp
import app.devti.android.R
import app.devti.android.ui.RingActivity

/**
 * Find my device, phone side: rings at alarm volume (even on silent — alarms are
 * allowed through Do Not Disturb by default), vibrates, and shows a full-screen
 * "I found it" screen. Stops on that button, when the other device stops it, or
 * after three minutes so a lost phone doesn't drain its battery.
 */
class Ringer(private val context: Context) {
    private val main = Handler(Looper.getMainLooper())
    private val audio = context.getSystemService(AudioManager::class.java)
    private var player: MediaPlayer? = null
    private var savedVolume: Int? = null
    private var active = false

    fun start(fromName: String) = main.post {
        if (active) return@post
        active = true
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ALARM)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()
        val uri = RingtoneManager.getActualDefaultRingtoneUri(context, RingtoneManager.TYPE_ALARM)
            ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE)
            ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION)
        savedVolume = audio?.getStreamVolume(AudioManager.STREAM_ALARM)
        audio?.let { it.setStreamVolume(AudioManager.STREAM_ALARM, it.getStreamMaxVolume(AudioManager.STREAM_ALARM), 0) }
        player = runCatching {
            MediaPlayer().apply {
                setAudioAttributes(attrs)
                setDataSource(context, uri)
                isLooping = true
                prepare()
                start()
            }
        }.getOrNull() // No playable tone on this device: vibration and the screen still work.
        vibrate()
        showScreen(fromName)
        main.postDelayed(autoStop, AUTO_STOP_MS)
    }

    fun stop() = main.post {
        if (!active) return@post
        active = false
        main.removeCallbacks(autoStop)
        player?.let { runCatching { it.stop() }; it.release() }
        player = null
        savedVolume?.let { v -> runCatching { audio?.setStreamVolume(AudioManager.STREAM_ALARM, v, 0) } }
        savedVolume = null
        vibrator()?.cancel()
        NotificationManagerCompat.from(context).cancel(Alerts.ID_RING)
    }

    private val autoStop = Runnable { (context.applicationContext as DevtiApp).core.stopRinging() }

    private fun vibrator(): Vibrator? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            context.getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            context.getSystemService(Vibrator::class.java)
        }

    private fun vibrate() {
        val v = vibrator() ?: return
        if (!v.hasVibrator()) return
        val pattern = longArrayOf(0, 600, 400, 600, 400, 1200, 800)
        v.vibrate(VibrationEffect.createWaveform(pattern, 0))
    }

    private fun showScreen(fromName: String) {
        val screen = PendingIntent.getActivity(
            context,
            7,
            Intent(context, RingActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val found = PendingIntent.getBroadcast(
            context,
            8,
            Intent(context, StopRingReceiver::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        // Android 14+ only grants full-screen intents to some apps; without it this is a
        // high-priority heads-up notification with the same button.
        Alerts.post(
            context,
            Alerts.ID_RING,
            NotificationCompat.Builder(context, Alerts.RING)
                .setSmallIcon(R.drawable.ic_stat_devti)
                .setContentTitle(context.getString(R.string.ring_title))
                .setContentText(context.getString(R.string.ring_body, fromName))
                .setCategory(NotificationCompat.CATEGORY_ALARM)
                .setPriority(NotificationCompat.PRIORITY_MAX)
                .setOngoing(true)
                .setFullScreenIntent(screen, true)
                .setContentIntent(screen)
                .addAction(0, context.getString(R.string.ring_found), found),
        )
    }

    companion object {
        private const val AUTO_STOP_MS = 3 * 60 * 1000L
    }
}

/** "I found it" from the notification. */
class StopRingReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        (context.applicationContext as DevtiApp).core.stopRinging()
    }
}
