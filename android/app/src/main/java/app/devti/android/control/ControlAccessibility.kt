package app.devti.android.control

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Path
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import java.lang.ref.WeakReference

/**
 * Lets the computer operate this phone: taps, long presses, swipes, typing into the
 * focused field, and the system buttons (back, home, recents, notifications, lock).
 *
 * Android only allows this through an accessibility service the user turns on
 * themselves (Settings → Accessibility → Devti control). It does nothing unless a paired
 * computer with the "control" permission sends input, and it reads no screen content:
 * it only finds the focused text field so typed text lands in it.
 */
class ControlAccessibility : AccessibilityService() {
    override fun onServiceConnected() {
        instance = WeakReference(this)
    }

    override fun onUnbind(intent: Intent?): Boolean {
        if (instance?.get() === this) instance = null
        return super.onUnbind(intent)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit

    override fun onInterrupt() = Unit

    private fun screen(): Pair<Float, Float> {
        val m = resources.displayMetrics
        @Suppress("DEPRECATION")
        val real = android.util.DisplayMetrics().also { (getSystemService(WINDOW_SERVICE) as android.view.WindowManager).defaultDisplay.getRealMetrics(it) }
        return (real.widthPixels.takeIf { it > 0 } ?: m.widthPixels).toFloat() to (real.heightPixels.takeIf { it > 0 } ?: m.heightPixels).toFloat()
    }

    /** One stroke from (x1,y1) to (x2,y2), in fractions of the screen. */
    fun gesture(x1: Float, y1: Float, x2: Float, y2: Float, durationMs: Long): Boolean {
        val (w, h) = screen()
        val path = Path().apply {
            moveTo(x1.coerceIn(0f, 1f) * (w - 1), y1.coerceIn(0f, 1f) * (h - 1))
            lineTo(x2.coerceIn(0f, 1f) * (w - 1), y2.coerceIn(0f, 1f) * (h - 1))
        }
        val stroke = GestureDescription.StrokeDescription(path, 0, durationMs.coerceIn(1, 10_000))
        return dispatchGesture(GestureDescription.Builder().addStroke(stroke).build(), null, null)
    }

    fun tap(x: Float, y: Float) = gesture(x, y, x, y, 40)

    fun longPress(x: Float, y: Float) = gesture(x, y, x, y, 700)

    /** Types into the focused text field (appends, like a keyboard would). */
    fun type(text: String): Boolean {
        val node = rootInActiveWindow?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT) ?: return false
        val current = node.text?.toString().takeUnless { node.isShowingHintText } ?: ""
        return setText(node, current + text)
    }

    fun key(name: String): Boolean {
        val node = rootInActiveWindow?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
        return when (name) {
            "backspace" -> node != null && setText(node, (node.text?.toString().takeUnless { node.isShowingHintText } ?: "").dropLast(1))
            "enter" -> if (node != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                node.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id)
            } else {
                false
            }
            "escape" -> performGlobalAction(GLOBAL_ACTION_BACK)
            else -> false
        }
    }

    private fun setText(node: AccessibilityNodeInfo, text: String): Boolean {
        val args = Bundle().apply { putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text) }
        return node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
    }

    fun nav(action: String): Boolean = when (action) {
        "back" -> performGlobalAction(GLOBAL_ACTION_BACK)
        "home" -> performGlobalAction(GLOBAL_ACTION_HOME)
        "recents" -> performGlobalAction(GLOBAL_ACTION_RECENTS)
        "notifications" -> performGlobalAction(GLOBAL_ACTION_NOTIFICATIONS)
        "quick_settings" -> performGlobalAction(GLOBAL_ACTION_QUICK_SETTINGS)
        "lock_screen" -> Build.VERSION.SDK_INT >= Build.VERSION_CODES.P && performGlobalAction(GLOBAL_ACTION_LOCK_SCREEN)
        "screenshot" -> Build.VERSION.SDK_INT >= Build.VERSION_CODES.P && performGlobalAction(GLOBAL_ACTION_TAKE_SCREENSHOT)
        else -> false
    }

    companion object {
        @Volatile private var instance: WeakReference<ControlAccessibility>? = null

        fun running(): ControlAccessibility? = instance?.get()

        fun enabled(context: Context): Boolean {
            val list = Settings.Secure.getString(context.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES) ?: return false
            val me = ComponentName(context, ControlAccessibility::class.java)
            return list.split(':').any { ComponentName.unflattenFromString(it) == me }
        }

        fun settingsIntent(): Intent = Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
}
