package app.devti.android.continuity

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle

/**
 * The phone side of clipboard sync.
 *
 * Android 10+ only lets the app that has focus (or the keyboard) *read* the clipboard,
 * so Devti shares what you copied when you open it, when you tap "Send clipboard", or
 * when you share text to Devti. *Writing* works any time, so text copied on the
 * computer lands on the phone straight away. This limit is stated in the app and docs.
 */
class ClipboardBridge(private val context: Context) {
    private val cm = context.getSystemService(ClipboardManager::class.java)
    private val main = Handler(Looper.getMainLooper())

    /** Puts text from another device on the phone's clipboard. */
    fun apply(text: String) {
        main.post {
            val clip = ClipData.newPlainText(LABEL, text)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                // Marks the clip as Devti's own so read() doesn't send it straight back.
                clip.description.extras = PersistableBundle().apply { putBoolean(EXTRA_FROM_DEVTI, true) }
            }
            runCatching { cm?.setPrimaryClip(clip) }
        }
    }

    /**
     * Reads the clipboard as text. Returns null when Android refuses (app not focused),
     * when it is empty or not text, or when Devti put it there itself.
     */
    fun read(): String? {
        val clip = runCatching { cm?.primaryClip }.getOrNull() ?: return null
        if (clip.itemCount == 0) return null
        val desc: ClipDescription? = clip.description
        if (desc?.label == LABEL) return null
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            desc?.extras?.getBoolean(EXTRA_FROM_DEVTI) == true
        ) return null
        val text = clip.getItemAt(0).coerceToText(context)?.toString()
        return text?.takeIf { it.isNotBlank() }
    }

    fun addListener(listener: ClipboardManager.OnPrimaryClipChangedListener) = cm?.addPrimaryClipChangedListener(listener)

    fun removeListener(listener: ClipboardManager.OnPrimaryClipChangedListener) = cm?.removePrimaryClipChangedListener(listener)

    companion object {
        const val LABEL = "Devti"
        private const val EXTRA_FROM_DEVTI = "app.devti.from_devti"
    }
}
