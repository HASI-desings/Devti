package app.devti.android.core

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.Network
import android.net.wifi.WifiManager
import android.os.BatteryManager
import android.os.Build
import android.provider.Settings
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import app.devti.android.BuildConfig
import app.devti.android.continuity.Alerts
import app.devti.android.continuity.ClipboardBridge
import app.devti.android.continuity.Files
import app.devti.android.continuity.NotificationMirror
import app.devti.android.continuity.Ringer
import app.devti.android.core.state.CoreState
import app.devti.android.core.state.MirroredNotification
import app.devti.android.core.state.PairingStage
import app.devti.android.core.state.Permissions
import app.devti.android.core.state.Reducer
import app.devti.android.core.state.TransferRecord
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import uniffi.devti_ffi.DevtiException
import uniffi.devti_ffi.DevtiMobile
import uniffi.devti_ffi.EventListener
import uniffi.devti_ffi.MobileConfig

/**
 * The app's single Devti Core node and the observable [CoreState] the UI renders.
 *
 * Section by section:
 * - start(): creates the Rust node (identity in the Keystore, database in app storage).
 * - events: the core calls [onEvent] on its own thread; [Reducer] folds it into state.
 * - device signals: battery level and network changes are pushed into the core.
 * - multicast: Android drops mDNS packets unless a MulticastLock is held.
 * - continuity: [platform] turns core events into Android actions (clipboard, ringing,
 *   notification actions, Do Not Disturb, moving received files to Downloads).
 */
class DevtiCore(private val context: Context) : EventListener {
    private val _state = MutableStateFlow(CoreState())
    val state: StateFlow<CoreState> = _state.asStateFlow()

    @Volatile private var node: DevtiMobile? = null
    private var multicast: WifiManager.MulticastLock? = null
    private var batteryReceiver: BroadcastReceiver? = null
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private val io = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val main = Handler(Looper.getMainLooper())

    val files = Files(context)
    val clipboard = ClipboardBridge(context)
    private val ringer = Ringer(context)

    /** Mirror this phone's notifications to the computer (needs notification access too). */
    @Volatile var mirrorNotifications: Boolean = true
        private set

    /** Share the clipboard both ways. */
    @Volatile var clipboardSync: Boolean = true
        private set

    @Volatile private var lastSentClip: String? = null

    @Synchronized
    fun start(): DevtiMobile? {
        node?.let { return it }
        return try {
            val created = DevtiMobile(
                MobileConfig(
                    dataDir = context.filesDir.resolve("devti").apply { mkdirs() }.absolutePath,
                    deviceName = deviceName(),
                    kind = if (isTablet()) "tablet" else "phone",
                    platform = "android",
                    osVersion = "Android ${Build.VERSION.RELEASE}",
                    model = "${Build.MANUFACTURER.replaceFirstChar { it.uppercase() }} ${Build.MODEL}",
                    appVersion = BuildConfig.VERSION_NAME,
                    discovery = true,
                ),
                KeystoreSecretStore(context),
                this,
            )
            node = created
            runCatching { created.setDownloadsDir(files.incomingDir.absolutePath) }
            mirrorNotifications = created.setting(SETTING_MIRROR) != "false"
            clipboardSync = created.setting(SETTING_CLIPBOARD) != "false"
            refresh()
            watchDevice()
            io.launch { files.cleanStale() }
            created
        } catch (e: DevtiException) {
            _state.update { it.copy(error = e.readable()) }
            null
        } catch (e: Throwable) {
            _state.update { it.copy(error = "Devti couldn't start: ${e.message ?: e.javaClass.simpleName}") }
            null
        }
    }

    fun refresh() {
        val n = node ?: return
        val pairing = _state.value.pairing
        val prev = _state.value
        _state.value = Reducer.snapshot(n.localJson(), n.peersJson(), n.nearbyJson(), n.transfersJson(), n.dnd())
            .copy(pairing = pairing, ringing = prev.ringing, ringingOut = prev.ringingOut, lastClipboard = prev.lastClipboard)
    }

    override fun onEvent(eventJson: String) {
        if (eventJson.contains("\"resync\"")) {
            refresh()
            return
        }
        _state.update { Reducer.apply(it, eventJson) }
        val obj = runCatching { Reducer.json.parseToJsonElement(eventJson).jsonObject }.getOrNull() ?: return
        runCatching { platform(obj) }
    }

    /** What the phone itself must do for an event (the UI state is already updated). */
    private fun platform(ev: JsonObject) {
        fun str(k: String) = ev[k]?.jsonPrimitive?.contentOrNull
        when (str("type")) {
            "clipboard_received" -> if (clipboardSync) str("text")?.let {
                lastSentClip = it
                clipboard.apply(it)
            }
            "ring_requested" -> if (ev["on"]?.jsonPrimitive?.booleanOrNull == true) ringer.start(str("from_name") ?: "") else ringer.stop()
            "notification_action_requested" -> {
                val ok = NotificationMirror.running()?.runAction(str("key") ?: return, str("action") ?: return, str("reply"))
                if (ok != true) toast("Couldn't do that on this phone: the notification is gone or access is off")
            }
            "notification_dismiss_requested" -> NotificationMirror.running()?.dismiss(str("key") ?: return)
            "dnd_changed" -> {
                // Only follow changes made elsewhere; ours already match the phone.
                if (str("by") != null) NotificationMirror.running()?.applyDnd(ev["on"]?.jsonPrimitive?.booleanOrNull ?: return)
            }
            "low_battery" -> Alerts.lowBattery(context, str("id") ?: "", str("name") ?: "", ev["percent"]?.jsonPrimitive?.intOrNull ?: 0)
            "transfer" -> {
                val t = ev["transfer"]?.let { runCatching { Reducer.json.decodeFromJsonElement(TransferRecord.serializer(), it) }.getOrNull() } ?: return
                onTransfer(t)
            }
            "notification_posted" -> {
                // Another phone or tablet's notification (computers don't publish theirs).
                val m = ev["notification"]?.let { runCatching { Reducer.json.decodeFromJsonElement(MirroredNotification.serializer(), it) }.getOrNull() } ?: return
                val title = listOf(m.notif.appName, m.notif.title).filter { it.isNotBlank() }.joinToString(" · ")
                Alerts.mirrored(context, "${m.from}/${m.notif.key}", title, "${m.notif.text}\n— ${m.fromName}", ev["dnd"]?.jsonPrimitive?.booleanOrNull == true)
            }
            "notification_removed" -> Alerts.cancelMirrored(context, "${str("from")}/${str("key")}")
        }
    }

    private fun onTransfer(t: TransferRecord) {
        if (t.incoming && t.state == "done") {
            io.launch {
                val already = files.published(t.id) != null
                val where = files.publish(t.id, t.path)
                if (!already) Alerts.fileReceived(context, t.id, t.name, t.deviceName, where?.uri, where?.mime)
            }
        } else if (!t.incoming && (t.state == "done" || t.state == "cancelled")) {
            io.launch { files.unstage(t.path) }
        }
    }

    // ----------------------------------------------------------- actions

    fun pairWithUri(uri: String): String? = act {
        _state.update { it.copy(pairing = PairingStage.Connecting(-1)) }
        start()?.pairWithUri(uri)
    }

    fun pairWithCode(deviceId: String, code: String): String? = act {
        _state.update { it.copy(pairing = PairingStage.Connecting(-1)) }
        start()?.pairWithCode(deviceId, code)
    }

    fun confirm(accept: Boolean): String? = act {
        val verify = _state.value.pairing as? PairingStage.Verify ?: return@act
        if (accept) _state.update { it.copy(pairing = verify.copy(confirmed = true)) }
        node?.confirmPairing(verify.pairingId.toULong(), accept)
    }

    fun clearPairing() = _state.update { it.copy(pairing = null) }

    fun dismissRemovedNotice() = _state.update { it.copy(removedNotice = null) }

    fun rename(name: String): String? = act { node?.rename(name) }

    fun setPaused(paused: Boolean): String? = act { node?.setPaused(paused) }

    fun forget(deviceId: String): String? = act { node?.revoke(deviceId) }

    fun reconnectNow() {
        node?.reconnectNow()
    }

    // ------------------------------------------------------------ continuity

    /** Shares the clipboard if it changed. Only works while Devti has focus (Android 10+). */
    fun captureClipboard(force: Boolean = false): Boolean {
        if (!clipboardSync && !force) return false
        val text = clipboard.read() ?: return false
        if (!force && text == lastSentClip) return false
        lastSentClip = text
        return runCatching { node?.sendClipboard(text) == true }.getOrDefault(false)
    }

    /** Text shared to Devti from another app goes to the computer's clipboard. */
    fun sendText(text: String): String? = act {
        lastSentClip = text
        node?.sendClipboard(text)
    }

    fun ring(deviceId: String, on: Boolean): String? = act {
        node?.ring(deviceId, on)
        _state.update { s -> s.copy(ringingOut = if (on) s.ringingOut + deviceId else s.ringingOut - deviceId) }
    }

    /** "I found it" — stops ringing here. */
    fun stopRinging() {
        ringer.stop()
        _state.update { it.copy(ringing = null) }
    }

    /** Copies the documents into Devti's cache and queues them for [deviceId]. */
    fun sendFiles(deviceId: String, uris: List<Uri>) {
        if (uris.isEmpty()) return
        _state.update { it.copy(preparing = it.preparing + uris.size) }
        io.launch {
            var failed = 0
            for (uri in uris) {
                try {
                    val staged = files.stage(uri)
                    try {
                        node?.sendFile(deviceId, staged.absolutePath)
                    } catch (e: DevtiException) {
                        files.unstage(staged.absolutePath)
                        throw e
                    }
                } catch (e: Exception) {
                    failed++
                } finally {
                    _state.update { it.copy(preparing = (it.preparing - 1).coerceAtLeast(0)) }
                }
            }
            if (failed > 0) toast(if (failed == 1) "One file couldn't be read" else "$failed files couldn't be read")
        }
    }

    fun cancelTransfer(id: Long): String? = act { node?.cancelTransfer(id.toULong()) }

    fun retryTransfer(id: Long): String? = act { node?.retryTransfer(id.toULong()) }

    fun clearFinishedTransfers(): String? = act {
        node?.clearFinishedTransfers()
        _state.update { s -> s.copy(transfers = s.transfers.filterNot { it.finished }) }
    }

    fun publishNotification(json: String) {
        runCatching { node?.publishNotification(json) }
    }

    fun removeNotification(key: String) {
        node?.removeNotification(key)
    }

    /** The phone's own Do Not Disturb changed. */
    fun phoneDndChanged(on: Boolean) {
        val n = node ?: return
        // When notification access starts, "on" wins: never switch off DND the user wants.
        if (n.dnd() != on) n.setDnd(on)
    }

    /** Ecosystem-wide Do Not Disturb from the app's switch. */
    fun setDnd(on: Boolean) {
        node?.setDnd(on)
        NotificationMirror.running()?.applyDnd(on)
    }

    fun setMirrorNotifications(on: Boolean): String? = act {
        node?.setSetting(SETTING_MIRROR, on.toString())
        mirrorNotifications = on
        NotificationMirror.running()?.let { if (on) it.publishAll() else it.unpublishAll() }
    }

    fun setClipboardSync(on: Boolean): String? = act {
        node?.setSetting(SETTING_CLIPBOARD, on.toString())
        clipboardSync = on
    }

    /** What [deviceId] may do on this phone. */
    fun setPermissions(deviceId: String, permissions: Permissions): String? = act {
        node?.setPermissions(deviceId, Reducer.json.encodeToString(Permissions.serializer(), permissions))
        refresh()
    }

    private fun toast(text: String) = main.post { Toast.makeText(context, text, Toast.LENGTH_LONG).show() }

    private inline fun act(block: () -> Unit): String? =
        try {
            block()
            null
        } catch (e: DevtiException) {
            val msg = e.readable()
            if (_state.value.pairing is PairingStage.Connecting) {
                _state.update { it.copy(pairing = PairingStage.Failed(-1, "other", msg)) }
            }
            msg
        }

    // ------------------------------------------------------- device signals

    private fun watchDevice() {
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        multicast = wifi?.createMulticastLock("devti-mdns")?.apply {
            setReferenceCounted(false)
            acquire()
        }

        val receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context, intent: Intent) {
                val level = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
                val scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, 100)
                val status = intent.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
                if (level < 0 || scale <= 0) return
                val pct = (level * 100 / scale).coerceIn(0, 100)
                val charging = status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL
                node?.setBattery(pct.toUByte(), charging)
            }
        }
        batteryReceiver = receiver
        // Protected system broadcast: still delivered to a not-exported receiver.
        ContextCompat.registerReceiver(
            context,
            receiver,
            IntentFilter(Intent.ACTION_BATTERY_CHANGED),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )

        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) = reconnectNow()
        }
        networkCallback = callback
        cm.registerDefaultNetworkCallback(callback)
    }

    private fun deviceName(): String {
        val user = Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME)
        return user?.takeIf { it.isNotBlank() } ?: Build.MODEL
    }

    private fun isTablet(): Boolean = context.resources.configuration.smallestScreenWidthDp >= 600
}

private const val SETTING_MIRROR = "mirror_notifications"
private const val SETTING_CLIPBOARD = "clipboard_sync"

/** The human sentence inside a core error. */
fun DevtiException.readable(): String = when (this) {
    is DevtiException.InvalidInput -> reason
    is DevtiException.Failed -> reason
    is DevtiException.Paused -> "Devti is paused on this phone. Resume it in Settings."
}
