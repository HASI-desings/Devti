package app.devti.android.comms

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Build
import uniffi.devti_ffi.AudioListener
import java.util.concurrent.ConcurrentHashMap

/**
 * Plays a computer's sound on this phone, so it comes out of the headphones connected
 * here, together with the phone's own sound (Android mixes them).
 *
 * The track is marked as not capturable, so sharing this phone's sound with the
 * computer can never send the computer's own sound back to it in a loop.
 */
class AudioPlayer : AudioListener {
    private val tracks = ConcurrentHashMap<String, AudioTrack>()

    fun open(deviceId: String, rate: Int, channels: Int) {
        close(deviceId)
        val mask = if (channels == 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        val min = AudioTrack.getMinBufferSize(rate, mask, AudioFormat.ENCODING_PCM_16BIT)
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
            .apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) setAllowedCapturePolicy(AudioAttributes.ALLOW_CAPTURE_BY_NONE) }
            .build()
        val track = AudioTrack.Builder()
            .setAudioAttributes(attrs)
            .setAudioFormat(AudioFormat.Builder().setSampleRate(rate).setChannelMask(mask).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
            // About 100 ms of cushion against network jitter.
            .setBufferSizeInBytes(maxOf(min * 2, rate * channels * 2 / 10))
            .setTransferMode(AudioTrack.MODE_STREAM)
            .apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY) }
            .build()
        track.play()
        tracks[deviceId] = track
    }

    fun close(deviceId: String) {
        tracks.remove(deviceId)?.let {
            runCatching { it.stop() }
            it.release()
        }
    }

    override fun onAudio(deviceId: String, kind: String, sampleRate: UInt, channels: UByte, pcm: ByteArray) {
        if (kind != "relay") return
        val track = tracks[deviceId] ?: return
        // Non-blocking: if the track is full, late audio is dropped rather than delaying everything.
        track.write(pcm, 0, pcm.size, AudioTrack.WRITE_NON_BLOCKING)
    }
}
