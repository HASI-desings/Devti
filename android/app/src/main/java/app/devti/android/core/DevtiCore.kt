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
import app.devti.android.comms.AudioShareService
import app.devti.android.comms.Beacon
import app.devti.android.comms.CallBridge
import app.devti.android.comms.CameraShareService
import app.devti.android.comms.Handoff
import app.devti.android.comms.SmsBridge
import app.devti.android.continuity.Alerts
import app.devti.android.continuity.ClipboardBridge
import app.devti.android.continuity.Files
import app.devti.android.continuity.NotificationMirror
import app.devti.android.continuity.Ringer
import app.devti.android.control.ControlAccessibility
import app.devti.android.control.PhoneCommands
import app.devti.android.control.ScreenCaptureService
import app.devti.android.control.ScreenShareActivity
import app.devti.android.control.screenTargetOf
import app.devti.android.core.state.AnywhereInfo
import app.devti.android.core.state.Catalog
import app.devti.android.core.state.LocalGuard
import app.devti.android.core.state.CallInfo
import app.devti.android.core.state.HandoffItem
import app.devti.android.core.state.ReachInfo
import app.devti.android.core.state.SharedFolder
import app.devti.android.continuity.PhoneFolders
import kotlinx.serialization.builtins.serializer
import app.devti.android.core.state.SmsMessage
import app.devti.android.core.state.SmsThread
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.floatOrNull
import org.json.JSONObject
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
import kotlinx.serialization.json.longOrNull
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import uniffi.devti_ffi.AudioStream
import uniffi.devti_ffi.DevtiException
import uniffi.devti_ffi.DevtiMobile
import uniffi.devti_ffi.EventListener
import uniffi.devti_ffi.FrameListener
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
 * - communication (Phase 6): texts, calls, "continue on phone", audio and camera sharing
 *   and the walk-away beacon, each in its own class under `comms/`.
 */
/** One frame of a computer's screen this phone is showing (Phase 4). */
class ScreenFrame(val seq: Long, val width: Int, val height: Int, val latencyMs: Int, val fps: Int, val jpeg: ByteArray)

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
    private val phone = PhoneCommands(context)
    val sms = SmsBridge(context)
    val calls = CallBridge(context)
    private val beacon = Beacon(context) { runCatching { node?.proximityToken() }.getOrNull() }
    @Volatile private var call: CallInfo? = null
    @Volatile private var cameraOfferedAt = 0L
    @Volatile private var warnedAccessibility = false

    /** Mirror this phone's notifications to the computer (needs notification access too). */
    @Volatile var mirrorNotifications: Boolean = true
        private set

    /** Share the clipboard both ways. */
    @Volatile var clipboardSync: Boolean = true
        private set

    @Volatile private var lastSentClip: String? = null

    // Only the newest frame matters: a StateFlow conflates, so a slow decoder skips frames.
    private val _frames = MutableStateFlow<ScreenFrame?>(null)
    val frames: StateFlow<ScreenFrame?> = _frames.asStateFlow()
    private val frameListener = object : FrameListener {
        override fun onFrame(deviceId: String, seq: ULong, width: UInt, height: UInt, latencyMs: UInt, fps: UByte, jpeg: ByteArray) {
            _frames.value = ScreenFrame(seq.toLong(), width.toInt(), height.toInt(), latencyMs.toInt(), fps.toInt(), jpeg)
        }
    }

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
            if (created.setting(SETTING_WALK_AWAY) == "true") {
                _state.update { it.copy(walkAway = true) }
                beacon.start()
            }
            refresh()
            watchDevice()
            watchSim()
            syncGuard()
            io.launch { files.cleanStale() }
            io.launch { runCatching { created.setCatalog(Reducer.json.encodeToString(Catalog.serializer(), phone.catalog())) } }
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
            .copy(
                pairing = pairing,
                ringing = prev.ringing,
                ringingOut = prev.ringingOut,
                lastClipboard = prev.lastClipboard,
                audioSharing = prev.audioSharing,
                cameraSharing = prev.cameraSharing,
                walkAway = prev.walkAway,
                fsListings = prev.fsListings,
                locations = prev.locations,
                guardClaim = prev.guardClaim,
                guardVersion = prev.guardVersion,
            )
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
            // ---- Stolen mode
            "guard_claim_requested" -> {
                val from = str("from") ?: return
                val name = str("from_name") ?: "Your computer"
                _state.update { it.copy(guardClaim = from to name) }
                Alerts.guardClaim(context, name)
            }
            "guard_changed", "guard_owner_changed" -> syncGuard()
            // Find my device: answer with this phone's location and network.
            "locate_requested" -> {
                val from = str("from") ?: return
                app.devti.android.continuity.Locator.fix(context) { fix ->
                    io.launch { runCatching { node?.reportLocation(from, fix) } }
                }
            }
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
            // ---- Phase 3: a computer controls this phone
            "remote_input" -> (ev["events"] as? JsonArray)?.forEach { e -> perform(e as? JsonObject ?: return@forEach) }
            "system_requested" -> {
                val a = (ev["action"] as? JsonObject) ?: return
                when (a["a"]?.jsonPrimitive?.contentOrNull) {
                    "lock" -> if (ControlAccessibility.running()?.nav("lock_screen") != true) needAccessibility()
                    "set_volume" -> phone.setVolume(a["percent"]?.jsonPrimitive?.intOrNull ?: return)
                }
            }
            "launch_requested" -> str("app")?.let { phone.launch(it) }
            "run_requested" -> {
                val from = str("from") ?: return
                val command = str("command") ?: return
                io.launch {
                    val (ok, out) = phone.run(command)
                    runCatching { node?.reportRun(from, command, ok, out) }
                }
            }
            "screen_requested" -> {
                val camera = str("source") == "camera"
                val from = str("from") ?: return
                if (ev["on"]?.jsonPrimitive?.booleanOrNull == true) {
                    val name = str("from_name") ?: "Your computer"
                    if (camera) requestCamera(from, name, back = str("facing") == "back") else requestScreenShare(from, name)
                } else if (camera) {
                    if (node?.cameraViewers().isNullOrEmpty()) CameraShareService.stop(context)
                } else if (screenOnlyViewers().isEmpty()) {
                    ScreenCaptureService.stop(context)
                }
            }
            // ---- Phase 6: texts, calls and "continue on phone"
            "sms_threads_requested" -> {
                val from = str("from") ?: return
                io.launch { runCatching { node?.answerSmsThreads(from, Reducer.json.encodeToString(ListSerializer(SmsThread.serializer()), sms.threads())) } }
            }
            "sms_messages_requested" -> {
                val from = str("from") ?: return
                val thread = str("thread") ?: return
                io.launch {
                    val list = sms.messages(thread)
                    runCatching { node?.answerSmsMessages(from, thread, Reducer.json.encodeToString(ListSerializer(SmsMessage.serializer()), list)) }
                }
            }
            "sms_send_requested" -> {
                val from = str("from") ?: return
                val to = (ev["to"] as? JsonArray)?.mapNotNull { it.jsonPrimitive.contentOrNull } ?: return
                val text = str("text") ?: return
                io.launch {
                    val err = sms.send(to, text)
                    runCatching { node?.reportSmsResult(from, err == null, err) }
                    if (err == null && to.size == 1) publishSms(sms.sent(to.first(), text))
                }
            }
            "call_action_requested" -> calls.act(str("action") ?: return)?.let { toast(it) }
            "handoff" -> {
                val item = ev["item"]?.let { runCatching { Reducer.json.decodeFromJsonElement(HandoffItem.serializer(), it) }.getOrNull() } ?: return
                Handoff.show(context, str("from_name") ?: "your computer", item) { clipboard.apply(it) }
            }
        }
    }

    private fun screenOnlyViewers(): List<String> = node?.screenViewers().orEmpty()

    /** One input event from a computer, performed through the accessibility service. */
    private fun perform(e: JsonObject) {
        fun f(k: String) = e[k]?.jsonPrimitive?.floatOrNull ?: 0f
        val k = e["k"]?.jsonPrimitive?.contentOrNull ?: return
        if (k == "media") {
            phone.media(e["action"]?.jsonPrimitive?.contentOrNull ?: return)
            return
        }
        val a = ControlAccessibility.running() ?: return needAccessibility()
        when (k) {
            "tap" -> a.tap(f("x"), f("y"))
            "long_press" -> a.longPress(f("x"), f("y"))
            "swipe" -> a.gesture(f("x1"), f("y1"), f("x2"), f("y2"), (e["ms"]?.jsonPrimitive?.longOrNull ?: 200L))
            "text" -> a.type(e["text"]?.jsonPrimitive?.contentOrNull ?: return)
            "key" -> a.key(e["key"]?.jsonPrimitive?.contentOrNull ?: return)
            "nav" -> a.nav(e["action"]?.jsonPrimitive?.contentOrNull ?: return)
        }
    }

    private fun needAccessibility() {
        if (warnedAccessibility) return
        warnedAccessibility = true
        toast(PhoneCommands.NEEDS_ACCESSIBILITY)
    }

    /** A computer wants to see this screen: Android requires the user to say yes each time. */
    private fun requestScreenShare(from: String, fromName: String) {
        if (ScreenCaptureService.running) return
        // The screen and the camera are separate streams: both can be shared at once.
        val intent = ScreenShareActivity.intent(context, ScreenShareActivity.MODE_SCREEN, from, fromName).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        // Works when Devti is open; otherwise the notification below lets the user say yes.
        runCatching { context.startActivity(intent) }
        Alerts.screenRequest(context, fromName, intent)
    }

    /** A computer wants this phone's camera (front or back). */
    private fun requestCamera(from: String, fromName: String, back: Boolean) {
        // Already on (the computer switched camera): turn the other way round, no second yes.
        if (CameraShareService.running || cameraAllowedRecently(from)) {
            if (context.checkSelfPermission(android.Manifest.permission.CAMERA) == android.content.pm.PackageManager.PERMISSION_GRANTED) {
                CameraShareService.start(context, back)
                return
            }
        }
        val intent = ScreenShareActivity.intent(context, ScreenShareActivity.MODE_CAMERA, from, fromName, back).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        // Works when Devti is open; otherwise the full-screen notification asks.
        runCatching { context.startActivity(intent) }
        Alerts.screenRequest(context, fromName, intent, camera = true)
    }

    private val cameraAllowed = mutableMapOf<String, Long>()

    /** The user just allowed `device` to use the camera. */
    fun rememberCameraAllowed(device: String) {
        cameraAllowed[device] = System.currentTimeMillis()
    }

    /** Allowed in the last 10 minutes: switching or restarting the camera needs no new yes. */
    private fun cameraAllowedRecently(device: String): Boolean =
        System.currentTimeMillis() - (cameraAllowed[device] ?: 0L) < 10 * 60_000

    /** Says no to every viewer waiting for the screen (or the camera). */
    fun declineScreenShare(camera: Boolean = false) {
        val source = if (camera) "camera" else "screen"
        val viewers = if (camera) node?.cameraViewers() else node?.screenViewers()
        viewers?.forEach { runCatching { node?.shareStop(it, source) } }
    }

    /** The camera couldn't start: tell the computers waiting for it, and the user here. */
    fun cameraFailed(why: String) {
        declineScreenShare(camera = true)
        toast("The camera couldn't start: $why")
    }

    fun stopSharingScreen() {
        declineScreenShare()
        ScreenCaptureService.stop(context)
    }

    // ---- Phase 6: this phone's camera, microphone and sound on a computer

    /** "Use as webcam": the computer shows an offer; accepting starts the camera here. */
    fun offerCamera(deviceId: String): String? = act {
        node?.cameraOffer(deviceId)
        cameraOfferedAt = System.currentTimeMillis()
    }

    /** True right after this phone offered its camera, so the computer's request needs no second yes. */
    fun cameraOfferedRecently(): Boolean = System.currentTimeMillis() - cameraOfferedAt < 120_000

    fun stopCamera() {
        node?.cameraViewers()?.forEach { runCatching { node?.shareStop(it, "camera") } }
        CameraShareService.stop(context)
    }

    fun pushCameraFrame(width: Int, height: Int, jpeg: ByteArray) {
        runCatching { node?.pushCameraFrame(width.toUInt(), height.toUInt(), jpeg) }
    }

    fun cameraSharing(on: Boolean) = _state.update { it.copy(cameraSharing = on) }

    /** Opens the consent flow for the microphone ("mic") or the phone's sound ("relay"). */
    fun shareAudio(deviceId: String, kind: String) {
        val mode = if (kind == "relay") ScreenShareActivity.MODE_RELAY else ScreenShareActivity.MODE_MIC
        context.startActivity(ScreenShareActivity.intent(context, mode, deviceId).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    fun stopAudio() = AudioShareService.stop(context)

    fun startAudio(deviceId: String, kind: String, rate: Int, channels: Int): AudioStream? = try {
        node?.startAudio(deviceId, kind, rate.toUInt(), channels.toUByte())
    } catch (e: DevtiException) {
        toast(e.readable())
        null
    }

    fun audioSharing(kind: String, on: Boolean) = _state.update { s -> s.copy(audioSharing = if (on) s.audioSharing + kind else s.audioSharing - kind) }

    // ---- Phase 6: texts, calls, handoff, walk-away

    fun publishSms(message: SmsMessage) {
        runCatching { node?.publishSms(Reducer.json.encodeToString(SmsMessage.serializer(), message)) }
    }

    /** Android's call state changed (from [app.devti.android.comms.PhoneStateReceiver]). */
    fun callStateChanged(state: String, number: String?) {
        val name = number?.let { sms.nameOf(it) }.orEmpty()
        val next = app.devti.android.continuity.Pure.nextCall(call, state, number, name, System.currentTimeMillis()) ?: return
        call = next.takeUnless { it.state == "ended" || it.state == "missed" }
        runCatching { node?.publishCall(Reducer.json.encodeToString(CallInfo.serializer(), next)) }
    }

    /** "Continue on computer": a link or text from this phone. */
    fun handoff(deviceId: String, item: HandoffItem): String? = act {
        node?.handoff(deviceId, Reducer.json.encodeToString(HandoffItem.serializer(), item))
    }

    /** Lets paired computers sense this phone nearby (a rotating Bluetooth token). */
    fun setWalkAway(on: Boolean): String? = act {
        node?.setSetting(SETTING_WALK_AWAY, on.toString())
        _state.update { it.copy(walkAway = on) }
        if (on) beacon.start() else beacon.stop()
    }

    fun walkAwayAllowed(): Boolean = beacon.allowed()

    // ---- Phase 7: reach from anywhere and shared folders

    fun reachInfo(): ReachInfo? = runCatching {
        Reducer.json.decodeFromString(ReachInfo.serializer(), node?.reachInfoJson() ?: return null)
    }.getOrNull()

    /** Lets paired computers reconnect to this phone away from home (VPN / typed addresses). */
    fun setReach(on: Boolean, extra: List<String> = reachInfo()?.extra.orEmpty()): String? = act {
        node?.setReach(on, Reducer.json.encodeToString(ListSerializer(String.serializer()), extra))
    }

    // ---- Stolen mode (this phone's side; the owner computer controls it)

    fun guardLocal(): LocalGuard? = runCatching {
        Reducer.json.decodeFromString(LocalGuard.serializer(), node?.guardLocalJson() ?: return null)
    }.getOrNull()

    /** The user's answer to "may <computer> protect this phone?". */
    fun answerGuardClaim(accept: Boolean) {
        val (from, _) = _state.value.guardClaim ?: return
        _state.update { it.copy(guardClaim = null) }
        act { node?.guardAccept(from, accept) }?.let { toast(it) }
        syncGuard()
    }

    /** The owner's offline answer typed on the stolen screen: true unlocks, false is wrong,
     *  null means wait (too many wrong answers). */
    fun guardTryCode(code: String): Boolean? = try {
        node?.guardTryCode(code).also { if (it == true) syncGuard() }
    } catch (e: DevtiException) {
        null
    }

    fun pushLocation(deviceId: String, fixJson: String) {
        io.launch { runCatching { node?.pushLocation(deviceId, fixJson) } }
    }

    /**
     * Makes the phone match stolen mode: tells the owner how well it's protected, and
     * locks down (stolen screen, device-owner policy, live location) or lets go.
     */
    fun syncGuard() {
        val n = node ?: return
        runCatching { n.guardSetReport(app.devti.android.guard.Guard.report(context)) }
        val g = guardLocal() ?: return
        if (g.state.on) {
            app.devti.android.guard.Guard.applyPolicy(context)
            val screen = app.devti.android.guard.StolenActivity.intent(context)
            runCatching { context.startActivity(screen) }
            // Android may refuse a background start; the full-screen notification can't be refused.
            if (!app.devti.android.guard.StolenScreen.visible) Alerts.stolenScreen(context, screen)
            runCatching { app.devti.android.guard.GuardService.start(context) }
        } else {
            app.devti.android.guard.Guard.liftPolicy(context)
            app.devti.android.guard.GuardService.stop(context)
            androidx.core.app.NotificationManagerCompat.from(context).cancel(Alerts.ID_GUARD)
        }
        _state.update { it.copy(guardVersion = it.guardVersion + 1) }
    }

    private var simSeen = false

    /** Stolen mode can turn itself on when the SIM card is taken out (the owner's choice). */
    private fun watchSim() {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context, i: Intent) {
                when (i.getStringExtra("ss")) {
                    "READY", "LOADED", "IMSI" -> simSeen = true
                    // Only a SIM that was there and is now gone counts (not a phone without one).
                    "ABSENT" -> if (simSeen) {
                        simSeen = false
                        runCatching { node?.guardSimRemoved() }
                    }
                }
            }
        }
        runCatching {
            ContextCompat.registerReceiver(
                context,
                receiver,
                IntentFilter("android.intent.action.SIM_STATE_CHANGED"),
                ContextCompat.RECEIVER_EXPORTED,
            )
        }
    }

    /** Asks a computer where it is (Find my device); the answer lands in [CoreState.locations]. */
    fun locate(deviceId: String): String? = act { node?.locate(deviceId) }

    fun anywhereInfo(): AnywhereInfo? = runCatching {
        Reducer.json.decodeFromString(AnywhereInfo.serializer(), node?.anywhereInfoJson() ?: return null)
    }.getOrNull()

    /** Connects from anywhere: mobile data, other Wi-Fi, through any router. */
    fun setAnywhere(on: Boolean): String? = act {
        node?.setAnywhere(on, anywhereInfo()?.customRelay.orEmpty())
    }

    /** Asks a computer for a folder listing; the answer lands in [CoreState.fsListings]. */
    fun fsList(deviceId: String, path: String): String? = act { node?.fsList(deviceId, path) }

    /** Downloads a shared file; it arrives like any received file (Downloads/Devti). */
    fun fsGet(deviceId: String, path: String): String? = act { node?.fsGet(deviceId, path) }

    fun sharedFolders(): List<SharedFolder> = runCatching {
        Reducer.json.decodeFromString(ListSerializer(SharedFolder.serializer()), node?.sharedFoldersJson() ?: "[]")
    }.getOrDefault(emptyList())

    /** Shares (or stops sharing) this phone's media and document folders, read-only. */
    fun setPhoneShared(on: Boolean): String? = act {
        val folders = if (on) PhoneFolders.defaults() else emptyList()
        node?.setSharedFolders(Reducer.json.encodeToString(ListSerializer(SharedFolder.serializer()), folders))
    }

    fun screenTarget(): JSONObject = screenTargetOf(node?.screenTargetJson())

    fun pushScreenFrame(width: Int, height: Int, jpeg: ByteArray) {
        runCatching { node?.pushScreenFrame(width.toUInt(), height.toUInt(), jpeg) }
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

    // ---- Phase 3: this phone controls a computer

    fun sendInput(deviceId: String, events: List<JsonObject>) {
        runCatching { node?.sendInput(deviceId, app.devti.android.core.state.Input.encode(events)) }
    }

    fun systemAction(deviceId: String, action: String, percent: Int? = null): String? =
        act { node?.systemAction(deviceId, app.devti.android.core.state.Input.system(action, percent)) }

    fun launchApp(deviceId: String, app: String): String? = act { node?.launchApp(deviceId, app) }

    fun runCommand(deviceId: String, command: String): String? = act { node?.runCommand(deviceId, command) }

    fun deckPress(deviceId: String, button: String): String? = act { node?.deckPress(deviceId, button) }

    fun requestCatalog(deviceId: String) {
        runCatching { node?.requestCatalog(deviceId) }
    }

    // ---- Phase 4: this phone shows a computer's screen

    fun watchScreen(deviceId: String): String? = act {
        _frames.value = null
        val m = context.resources.displayMetrics
        val width = maxOf(m.widthPixels, m.heightPixels).coerceIn(640, 1920)
        node?.screenStart(deviceId, width.toUInt(), 30.toUByte(), frameListener)
    }

    fun stopWatching(deviceId: String) {
        runCatching { node?.screenStop(deviceId) }
        _frames.value = null
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
private const val SETTING_WALK_AWAY = "walk_away"

/** The human sentence inside a core error. */
fun DevtiException.readable(): String = when (this) {
    is DevtiException.InvalidInput -> reason
    is DevtiException.Failed -> reason
    is DevtiException.Paused -> "Devti is paused on this phone. Resume it in Settings."
}
