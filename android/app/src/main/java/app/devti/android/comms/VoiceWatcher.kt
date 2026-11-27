package app.devti.android.comms

import android.content.ComponentName
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.AudioPlaybackConfiguration
import android.media.session.MediaController
import android.media.session.MediaSessionManager
import android.media.session.PlaybackState
import android.os.Handler
import android.os.Looper
import app.devti.android.continuity.NotificationMirror

/**
 * Tells the computers whether this phone is playing **music** or a **voice message**
 * (WhatsApp, Instagram, Telegram…), so music on the other devices can pause for it and
 * come back after. It also pauses and resumes this phone's music when a computer asks.
 *
 * Two signals, because apps differ:
 * * media sessions (needs Notification access, which Devti already uses to mirror
 *   notifications): which app is playing, and whether it's a messaging app;
 * * Android's playback list: any sound marked as speech or a call.
 */
class VoiceWatcher(private val context: Context, private val report: (music: Boolean, voice: Boolean, app: String) -> Unit) {
    private val main = Handler(Looper.getMainLooper())
    private val audio = context.getSystemService(AudioManager::class.java)
    private val sessions = context.getSystemService(MediaSessionManager::class.java)
    private val component = ComponentName(context, NotificationMirror::class.java)
    private var controllers: List<MediaController> = emptyList()
    private val callbacks = mutableMapOf<MediaController, MediaController.Callback>()
    private var last: Triple<Boolean, Boolean, String>? = null
    private val pausedByUs = mutableSetOf<String>()

    private val playback = object : AudioManager.AudioPlaybackCallback() {
        override fun onPlaybackConfigChanged(configs: MutableList<AudioPlaybackConfiguration>?) = schedule()
    }

    private val sessionsChanged = MediaSessionManager.OnActiveSessionsChangedListener { list -> watch(list.orEmpty()) }

    fun start() {
        runCatching { audio?.registerAudioPlaybackCallback(playback, main) }
        runCatching {
            sessions?.addOnActiveSessionsChangedListener(sessionsChanged, component, main)
            watch(sessions?.getActiveSessions(component).orEmpty())
        }
        schedule()
    }

    fun stop() {
        runCatching { audio?.unregisterAudioPlaybackCallback(playback) }
        runCatching { sessions?.removeOnActiveSessionsChangedListener(sessionsChanged) }
        callbacks.forEach { (c, cb) -> c.unregisterCallback(cb) }
        callbacks.clear()
    }

    private fun watch(list: List<MediaController>) {
        callbacks.forEach { (c, cb) -> c.unregisterCallback(cb) }
        callbacks.clear()
        controllers = list
        for (c in list) {
            val cb = object : MediaController.Callback() {
                override fun onPlaybackStateChanged(state: PlaybackState?) = schedule()
            }
            c.registerCallback(cb, main)
            callbacks[c] = cb
        }
        schedule()
    }

    private val compute = Runnable { compute() }

    /** Debounced: apps flip states quickly when a track changes. */
    private fun schedule() {
        main.removeCallbacks(compute)
        main.postDelayed(compute, 300)
    }

    private fun playing(c: MediaController) = c.playbackState?.state == PlaybackState.STATE_PLAYING

    private fun compute() {
        val playingNow = controllers.filter { playing(it) }
        val voiceApp = playingNow.firstOrNull { Messaging.isMessaging(it.packageName) }
        val speech = runCatching {
            audio?.activePlaybackConfigurations.orEmpty().any {
                val a = it.audioAttributes
                a.contentType == AudioAttributes.CONTENT_TYPE_SPEECH || a.usage == AudioAttributes.USAGE_VOICE_COMMUNICATION
            }
        }.getOrDefault(false)
        val voice = voiceApp != null || speech
        val music = playingNow.any { !Messaging.isMessaging(it.packageName) }
        val app = voiceApp?.packageName ?: playingNow.firstOrNull()?.packageName ?: ""
        val now = Triple(music, voice, app)
        if (now != last) {
            last = now
            report(music, voice, app)
        }
    }

    /** A computer asked: pause this phone's music for a voice message there, or resume it. */
    fun command(action: String) {
        if (action == "pause") {
            for (c in controllers) {
                if (playing(c) && !Messaging.isMessaging(c.packageName)) {
                    c.transportControls.pause()
                    pausedByUs += c.packageName
                }
            }
        } else {
            for (c in controllers) {
                if (c.packageName in pausedByUs) c.transportControls.play()
            }
            pausedByUs.clear()
        }
    }
}

/** Apps whose playback counts as a voice message (pure, unit-tested). */
object Messaging {
    private val PACKAGES = setOf(
        "com.whatsapp",
        "com.whatsapp.w4b",
        "com.instagram.android",
        "com.instagram.barcelona",
        "com.facebook.orca",
        "com.facebook.mlite",
        "org.telegram.messenger",
        "org.telegram.messenger.web",
        "org.thunderdog.challegram",
        "org.thoughtcrime.securesms",
        "com.snapchat.android",
        "com.viber.voip",
        "com.imo.android.imoim",
        "com.tencent.mm",
        "com.discord",
    )

    fun isMessaging(pkg: String?): Boolean = pkg != null && (pkg in PACKAGES || pkg.startsWith("org.telegram."))
}
