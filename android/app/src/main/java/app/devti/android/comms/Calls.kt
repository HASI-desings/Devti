package app.devti.android.comms

import android.Manifest
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioManager
import android.os.Build
import android.telecom.TelecomManager
import android.telephony.TelephonyManager
import androidx.core.content.ContextCompat
import app.devti.android.DevtiApp

/**
 * Calls on the computer: the phone tells connected computers when it rings (with the
 * caller's name from contacts), and answers, declines, ends or mutes when asked.
 *
 * The call's audio stays on the phone — Android doesn't let apps carry call audio. To
 * talk through the computer, pair the phone with the computer over Bluetooth as a
 * headset (Hands-Free); Devti still shows the call card and its buttons.
 */
class CallBridge(private val context: Context) {
    private val telecom = context.getSystemService(TelecomManager::class.java)
    private val audio = context.getSystemService(AudioManager::class.java)

    fun allowed(): Boolean = listOf(Manifest.permission.READ_PHONE_STATE, Manifest.permission.ANSWER_PHONE_CALLS).all {
        ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
    }

    /** Returns null on success or why it couldn't. */
    @Suppress("MissingPermission", "DEPRECATION")
    fun act(action: String): String? {
        if (!allowed()) return "Allow phone calls for Devti on the phone first."
        return runCatching {
            when (action) {
                "answer" -> telecom.acceptRingingCall()
                "decline", "end" -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) telecom.endCall() else return "Android 8 can't end calls from an app."
                "mute" -> audio.isMicrophoneMute = true
                "unmute" -> audio.isMicrophoneMute = false
            }
            null
        }.getOrElse { "The phone refused: ${it.message ?: "unknown error"}" }
    }
}

/** Call state changes, with the number (needs READ_CALL_LOG). */
class PhoneStateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != TelephonyManager.ACTION_PHONE_STATE_CHANGED) return
        val core = (context.applicationContext as DevtiApp).core
        core.start()
        val state = intent.getStringExtra(TelephonyManager.EXTRA_STATE) ?: return
        @Suppress("DEPRECATION")
        val number = intent.getStringExtra(TelephonyManager.EXTRA_INCOMING_NUMBER)
        core.callStateChanged(state, number)
    }
}
