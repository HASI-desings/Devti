package app.devti.android.continuity

import android.app.ActivityOptions
import android.app.Notification
import android.app.PendingIntent
import android.app.RemoteInput
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import androidx.core.app.NotificationManagerCompat
import app.devti.android.DevtiApp
import app.devti.android.core.state.NotifAction
import app.devti.android.core.state.NotificationInfo
import app.devti.android.core.state.Reducer
import java.io.ByteArrayOutputStream
import java.lang.ref.WeakReference

/**
 * Mirrors this phone's notifications to the computer, and carries out what the
 * computer asks: run an action, send a quick reply, dismiss, or follow its Do Not
 * Disturb.
 *
 * Section by section:
 * - Filtering: Devti's own, ongoing (music, navigation, downloads), group summaries
 *   and empty notifications are skipped — they are noise on a computer.
 * - Conversion: title/text (big text when there is one), up to three actions
 *   (marking the ones that take a typed reply), a 64 px app icon as PNG.
 * - Actions: replies go through Android's RemoteInput exactly as if typed on the phone.
 * - DND: the phone's own Do Not Disturb and Devti's ecosystem-wide one follow each other.
 *
 * Android only runs this service after the user turns on "Notification access" for
 * Devti in system settings; the app explains that and links straight there.
 */
class NotificationMirror : NotificationListenerService() {
    private val core get() = (application as DevtiApp).core
    private val iconCache = HashMap<String, List<Int>?>()

    override fun onListenerConnected() {
        instance = WeakReference(this)
        if (core.mirrorNotifications) publishAll()
        // "On" wins when access starts: phone DND turns the ecosystem's on, and vice versa.
        if (core.state.value.dnd) applyDnd(true) else followPhoneDnd(currentInterruptionFilter)
    }

    fun publishAll() {
        runCatching { activeNotifications }.getOrNull()?.forEach { publish(it) }
    }

    fun unpublishAll() {
        runCatching { activeNotifications }.getOrNull()?.forEach { core.removeNotification(it.key) }
    }

    override fun onListenerDisconnected() {
        if (instance?.get() === this) instance = null
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        if (core.mirrorNotifications) publish(sbn)
    }

    override fun onNotificationRemoved(sbn: StatusBarNotification) {
        core.removeNotification(sbn.key)
    }

    override fun onInterruptionFilterChanged(interruptionFilter: Int) = followPhoneDnd(interruptionFilter)

    private fun followPhoneDnd(filter: Int) {
        if (filter == INTERRUPTION_FILTER_UNKNOWN) return
        core.phoneDndChanged(filter != INTERRUPTION_FILTER_ALL)
    }

    private fun publish(sbn: StatusBarNotification) {
        val info = convert(sbn) ?: return
        core.publishNotification(Reducer.json.encodeToString(NotificationInfo.serializer(), info))
    }

    private fun convert(sbn: StatusBarNotification): NotificationInfo? {
        if (!Pure.shouldMirror(sbn.packageName, packageName, sbn.isOngoing, sbn.notification.flags)) return null
        val n = sbn.notification
        val extras = n.extras
        val title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString().orEmpty()
        val text = (extras.getCharSequence(Notification.EXTRA_BIG_TEXT) ?: extras.getCharSequence(Notification.EXTRA_TEXT))
            ?.toString().orEmpty()
        if (title.isBlank() && text.isBlank()) return null
        val actions = n.actions.orEmpty().take(MAX_ACTIONS).mapIndexed { i, a ->
            NotifAction(
                id = i.toString(),
                label = a.title?.toString().orEmpty().ifBlank { "Action" },
                reply = a.remoteInputs?.any { it.allowFreeFormInput } == true,
            )
        }
        return NotificationInfo(
            key = sbn.key,
            appId = sbn.packageName,
            appName = appLabel(sbn.packageName),
            title = title,
            text = text,
            ts = sbn.postTime,
            actions = actions,
            iconPng = iconCache.getOrPut(sbn.packageName) { appIcon(sbn.packageName) },
            dismissable = sbn.isClearable,
        )
    }

    private fun appLabel(pkg: String): String = runCatching {
        packageManager.getApplicationLabel(packageManager.getApplicationInfo(pkg, 0)).toString()
    }.getOrDefault(pkg)

    private fun appIcon(pkg: String): List<Int>? = runCatching {
        val d = packageManager.getApplicationIcon(pkg)
        val bmp = Bitmap.createBitmap(ICON_PX, ICON_PX, Bitmap.Config.ARGB_8888)
        d.setBounds(0, 0, ICON_PX, ICON_PX)
        d.draw(Canvas(bmp))
        val out = ByteArrayOutputStream()
        bmp.compress(Bitmap.CompressFormat.PNG, 100, out)
        bmp.recycle()
        out.toByteArray().takeIf { it.size <= MAX_ICON_BYTES }?.map { it.toInt() and 0xff }
    }.getOrNull()

    /** Runs action [actionId] of notification [key], with [reply] typed on the computer. */
    fun runAction(key: String, actionId: String, reply: String?): Boolean {
        val sbn = runCatching { getActiveNotifications(arrayOf(key)) }.getOrNull()?.firstOrNull() ?: return false
        val action = sbn.notification.actions?.getOrNull(actionId.toIntOrNull() ?: return false) ?: return false
        val fill = Intent()
        val inputs = action.remoteInputs
        if (reply != null && inputs != null && inputs.isNotEmpty()) {
            val results = Bundle().apply { inputs.forEach { putCharSequence(it.resultKey, reply) } }
            RemoteInput.addResultsToIntent(inputs, fill, results)
        }
        return send(action.actionIntent, fill)
    }

    fun dismiss(key: String) = runCatching { cancelNotification(key) }.isSuccess

    /** Follows Devti's ecosystem-wide Do Not Disturb (needs notification access). */
    fun applyDnd(on: Boolean) {
        val now = currentInterruptionFilter
        // Leaves the user's own DND flavour (alarms only, total silence) alone.
        if (on && now == INTERRUPTION_FILTER_ALL) runCatching { requestInterruptionFilter(INTERRUPTION_FILTER_PRIORITY) }
        if (!on && now != INTERRUPTION_FILTER_ALL && now != INTERRUPTION_FILTER_UNKNOWN) {
            runCatching { requestInterruptionFilter(INTERRUPTION_FILTER_ALL) }
        }
    }

    private fun send(pi: PendingIntent, fill: Intent): Boolean = runCatching {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            // Some actions open an activity ("Mark as read" doesn't, "Open" does); allow it,
            // the user asked for it from their computer.
            val opts = ActivityOptions.makeBasic()
                .setPendingIntentBackgroundActivityStartMode(ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOWED)
            pi.send(this, 0, fill, null, null, null, opts.toBundle())
        } else {
            pi.send(this, 0, fill)
        }
    }.isSuccess

    companion object {
        private const val MAX_ACTIONS = 3
        private const val ICON_PX = 64
        private const val MAX_ICON_BYTES = 64 * 1024

        @Volatile private var instance: WeakReference<NotificationMirror>? = null

        /** The running listener, when the user has granted notification access. */
        fun running(): NotificationMirror? = instance?.get()

        fun hasAccess(context: Context): Boolean =
            NotificationManagerCompat.getEnabledListenerPackages(context).contains(context.packageName)

        fun settingsIntent(context: Context): Intent =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                Intent(Settings.ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS)
                    .putExtra(
                        Settings.EXTRA_NOTIFICATION_LISTENER_COMPONENT_NAME,
                        ComponentName(context, NotificationMirror::class.java).flattenToString(),
                    )
            } else {
                Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
            }
    }
}
