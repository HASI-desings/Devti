package app.devti.android.comms

import android.annotation.SuppressLint
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioPlaybackCaptureConfiguration
import android.media.AudioRecord
import android.media.MediaRecorder
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import app.devti.android.DevtiApp
import app.devti.android.R
import app.devti.android.continuity.Alerts

/**
 * Streams audio to a computer, 48 kHz 16-bit in 20 ms chunks:
 * * **mic** — this phone's microphone becomes the computer's microphone;
 * * **relay** — what this phone is playing comes out of the computer (and earbuds
 *   connected to it). Android 10+ only, after the "start recording or casting" consent,
 *   and only from apps that allow it (most music and video apps do; calls never).
 */
class AudioShareService : Service() {
    private var projection: MediaProjection? = null

    override fun onBind(intent: Intent?): IBinder? = null

    @SuppressLint("MissingPermission") // RECORD_AUDIO is requested before this starts
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        val device = intent?.getStringExtra(EXTRA_DEVICE) ?: return START_NOT_STICKY
        val kind = intent.getStringExtra(EXTRA_KIND) ?: "mic"
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            // Playback capture also records through AudioRecord, so it needs the microphone type too.
            if (kind == "mic") ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
            else ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        } else {
            0
        }
        ServiceCompat.startForeground(this, Alerts.ID_AUDIO, notification(kind), type)
        // One stream at a time: the microphone or the phone's sound.
        if (running) return START_NOT_STICKY
        val record = try {
            if (kind == "relay") relayRecorder(intent) else micRecorder()
        } catch (e: Exception) {
            null
        } ?: run {
            stopSelf()
            return START_NOT_STICKY
        }
        val channels = if (kind == "relay") 2 else 1
        val core = (application as DevtiApp).core
        val stream = core.startAudio(device, kind, RATE, channels) ?: run {
            projection?.stop()
            record.release()
            stopSelf()
            return START_NOT_STICKY
        }
        running = true
        core.audioSharing(kind, true)
        Thread({
            val chunk = ByteArray(RATE / 50 * 2 * channels)
            record.startRecording()
            // 5 s of chunks the computer can't take (it went away) ends the share.
            var refused = 0
            while (running && refused < 250) {
                val n = record.read(chunk, 0, chunk.size)
                if (n > 0) refused = if (stream.push(chunk.copyOf(n))) 0 else refused + 1
            }
            record.stop()
            record.release()
            stream.stop()
            core.audioSharing(kind, false)
            if (running) stopSelf()
        }, "devti-audio").start()
        return START_NOT_STICKY
    }

    @SuppressLint("MissingPermission")
    private fun micRecorder(): AudioRecord {
        val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        return AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, maxOf(min, RATE / 5))
    }

    @SuppressLint("MissingPermission")
    private fun relayRecorder(intent: Intent): AudioRecord? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
        @Suppress("DEPRECATION")
        val data: Intent = intent.getParcelableExtra(EXTRA_DATA) ?: return null
        val code = intent.getIntExtra(EXTRA_CODE, 0)
        val p = getSystemService(MediaProjectionManager::class.java).getMediaProjection(code, data) ?: return null
        projection = p
        val config = AudioPlaybackCaptureConfiguration.Builder(p)
            .addMatchingUsage(AudioAttributes.USAGE_MEDIA)
            .addMatchingUsage(AudioAttributes.USAGE_GAME)
            .addMatchingUsage(AudioAttributes.USAGE_UNKNOWN)
            .build()
        return AudioRecord.Builder()
            .setAudioPlaybackCaptureConfig(config)
            .setAudioFormat(
                AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(RATE).setChannelMask(AudioFormat.CHANNEL_IN_STEREO).build(),
            )
            .build()
    }

    private fun notification(kind: String) = NotificationCompat.Builder(this, Alerts.CONNECTION)
        .setSmallIcon(R.drawable.ic_stat_devti)
        .setContentTitle(if (kind == "mic") "Your computer is using this phone's microphone" else "Playing this phone's sound on your computer")
        .setOngoing(true)
        .setContentIntent(Alerts.openApp(this))
        .addAction(
            0,
            "Stop",
            android.app.PendingIntent.getService(this, 4, Intent(this, AudioShareService::class.java).setAction(ACTION_STOP), android.app.PendingIntent.FLAG_IMMUTABLE),
        )
        .build()

    override fun onDestroy() {
        running = false
        projection?.stop()
        super.onDestroy()
    }

    companion object {
        const val RATE = 48_000

        /** A stream is being sent (only one at a time). */
        @Volatile var running = false
            private set
        private const val ACTION_STOP = "app.devti.android.STOP_AUDIO"
        const val EXTRA_DEVICE = "device"
        const val EXTRA_KIND = "kind"
        const val EXTRA_CODE = "code"
        const val EXTRA_DATA = "data"

        fun startMic(context: Context, device: String) {
            ContextCompat.startForegroundService(
                context,
                Intent(context, AudioShareService::class.java).putExtra(EXTRA_DEVICE, device).putExtra(EXTRA_KIND, "mic"),
            )
        }

        fun startRelay(context: Context, device: String, code: Int, data: Intent) {
            ContextCompat.startForegroundService(
                context,
                Intent(context, AudioShareService::class.java)
                    .putExtra(EXTRA_DEVICE, device)
                    .putExtra(EXTRA_KIND, "relay")
                    .putExtra(EXTRA_CODE, code)
                    .putExtra(EXTRA_DATA, data),
            )
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, AudioShareService::class.java))
        }
    }
}
