import app.devti.android.core.state.CoreState
import app.devti.android.core.state.NotifAction
import app.devti.android.core.state.NotificationInfo
import app.devti.android.core.state.Permissions
import app.devti.android.core.state.PairingStage
import app.devti.android.core.state.Reducer
import app.devti.android.core.state.formatCode
import app.devti.android.core.state.looksLikePairingLink
import java.nio.file.Files
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicReference
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import uniffi.devti_ffi.DevtiException
import uniffi.devti_ffi.DevtiMobile
import uniffi.devti_ffi.EventListener
import uniffi.devti_ffi.MobileConfig
import uniffi.devti_ffi.PlatformSecretStore
import uniffi.devti_ffi.coreVersion

/** A phone-side node driven exactly like the Android app drives it. */
private class Device(name: String, kind: String, platform: String) : EventListener, PlatformSecretStore {
    private val secrets = ConcurrentHashMap<String, ByteArray>()
    val state = AtomicReference(CoreState())
    val core: DevtiMobile = DevtiMobile(
        MobileConfig(
            dataDir = Files.createTempDirectory("devti-jvm").toString(),
            deviceName = name,
            kind = kind,
            platform = platform,
            osVersion = "test",
            model = null,
            appVersion = "0.1.0",
            discovery = false,
        ),
        this,
        this,
    )

    init {
        state.set(Reducer.snapshot(core.localJson(), core.peersJson(), core.nearbyJson(), core.transfersJson(), core.dnd()))
    }

    override fun onEvent(eventJson: String) {
        state.updateAndGet { Reducer.apply(it, eventJson) }
    }

    override fun get(key: String): ByteArray? = secrets[key]
    override fun set(key: String, value: ByteArray) { secrets[key] = value }
    override fun delete(key: String) { secrets.remove(key) }
    override fun backendName(): String = "test keystore"

    fun waitFor(what: String, check: (CoreState) -> Boolean): CoreState {
        val end = System.currentTimeMillis() + 15_000
        while (System.currentTimeMillis() < end) {
            val s = state.get()
            if (check(s)) return s
            Thread.sleep(25)
        }
        error("timed out waiting for $what; state=${state.get()}")
    }
}

class CoreBridgeTest {
    @Test
    fun helpers() {
        assertEquals("482 913", formatCode("482913"))
        assertTrue(looksLikePairingLink(" devti://pair?v=1&x"))
        assertTrue(!looksLikePairingLink("https://example.com"))
        assertTrue(coreVersion().isNotEmpty())
    }

    @Test
    fun reducerIgnoresGarbageAndUnknownEvents() {
        val s = CoreState()
        assertEquals(s, Reducer.apply(s, "not json"))
        assertEquals(s, Reducer.apply(s, """{"type":"from_the_future","x":1}"""))
        assertEquals(s, Reducer.apply(s, """{"type":"peer_changed","peer":{"broken":true}}"""))
    }

    @Test
    fun phoneLikeNodePairsWithHubLikeNodeThroughTheBindings() {
        val hub = Device("Studio Laptop", "laptop", "linux")
        val phone = Device("Pixel 8", "phone", "android")
        assertEquals("test keystore", phone.state.get().local?.secretBackend)

        // The phone joins exactly as if it had scanned the Hub's QR code.
        val window = Reducer.json.parseToJsonElement(hub.core.openPairingWindowJson()).jsonObject
        val uri = window["uri"]!!.jsonPrimitive.content
        val id = phone.core.pairWithUri(uri)
        val verifying = phone.waitFor("verify") { it.pairing is PairingStage.Verify }
        val v = verifying.pairing as PairingStage.Verify
        assertEquals(id.toLong(), v.pairingId)
        assertEquals("Studio Laptop", v.peer.name)
        assertEquals(7, v.sas.length)

        val hubVerify = hub.waitFor("hub verify") { it.pairing is PairingStage.Verify }.pairing as PairingStage.Verify
        assertEquals(v.sas, hubVerify.sas)
        hub.core.confirmPairing(hubVerify.pairingId.toULong(), true)
        phone.core.confirmPairing(id, true)

        val paired = phone.waitFor("online hub") { s -> s.peers.any { it.online } }
        assertEquals("Studio Laptop", paired.hub?.info?.name)
        assertTrue(paired.pairing is PairingStage.Done)

        // Battery from the phone reaches the hub.
        phone.core.setBattery(57u, true)
        val hubSees = hub.waitFor("battery") { s -> s.peers.any { it.status.battery?.percent == 57 } }
        assertEquals(true, hubSees.peers.first().status.battery?.charging)

        // Phase 2 through the bindings: clipboard, notifications, a file, permissions.
        val hubId = paired.hub!!.info.id
        val phoneId = phone.state.get().local!!.info.id
        hub.core.sendClipboard("from the laptop")
        assertEquals("from the laptop", phone.waitFor("clipboard") { it.lastClipboard != null }.lastClipboard?.text)

        val notif = NotificationInfo(key = "k1", appId = "org.chat", appName = "Chat", title = "Ana", text = "Lunch?", ts = 1,
            actions = listOf(NotifAction("0", "Reply", reply = true)), iconPng = listOf(137, 80, 78, 71))
        phone.core.publishNotification(Reducer.json.encodeToString(NotificationInfo.serializer(), notif))
        val end = System.currentTimeMillis() + 10_000
        while (!hub.core.notificationsJson().contains("Lunch?") && System.currentTimeMillis() < end) Thread.sleep(25)
        assertTrue(hub.core.notificationsJson().contains("Lunch?"))

        val downloads = Files.createTempDirectory("devti-dl")
        phone.core.setDownloadsDir(downloads.toString())
        val payload = Files.createTempFile("devti", ".bin").toFile().apply { writeBytes(ByteArray(300_000) { (it % 251).toByte() }) }
        hub.core.sendFile(phoneId, payload.absolutePath)
        val got = phone.waitFor("file") { s -> s.transfers.any { it.incoming && it.state == "done" } }.transfers.first()
        assertEquals(300_000L, got.size)
        assertTrue(java.io.File(got.path).readBytes().contentEquals(payload.readBytes()))

        // The phone takes clipboard rights away from the hub; the hub's next clipboard is ignored.
        val perms = phone.state.get().peers.first().permissions.copy(clipboard = false)
        phone.core.setPermissions(hubId, Reducer.json.encodeToString(Permissions.serializer(), perms))
        hub.core.sendClipboard("should not arrive")
        Thread.sleep(800)
        assertEquals("from the laptop", phone.state.get().lastClipboard?.text)

        // Errors arrive as typed exceptions with a readable reason.
        val bad = assertFailsWith<DevtiException.InvalidInput> { phone.core.pairWithUri("nope") }
        assertTrue(bad.reason.isNotEmpty())

        // The hub removes the phone; the phone learns about it.
        hub.core.revoke(phone.state.get().local!!.info.id)
        val removed = phone.waitFor("removed") { it.peers.isEmpty() }
        assertNotNull(removed.removedNotice)

        phone.core.shutdown()
        hub.core.shutdown()
    }
}
