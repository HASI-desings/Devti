import app.devti.android.core.state.Catalog
import app.devti.android.core.state.CatalogCommand
import app.devti.android.core.state.CallInfo
import app.devti.android.core.state.HandoffItem
import app.devti.android.core.state.ReachInfo
import app.devti.android.core.state.SharedFolder
import app.devti.android.core.state.SmsMessage
import app.devti.android.core.state.SmsThread
import kotlinx.serialization.builtins.ListSerializer
import app.devti.android.core.state.CoreState
import app.devti.android.core.state.Input
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
import uniffi.devti_ffi.FrameListener
import uniffi.devti_ffi.MobileConfig
import uniffi.devti_ffi.PlatformSecretStore
import uniffi.devti_ffi.coreVersion

/** A phone-side node driven exactly like the Android app drives it. */
private class Device(name: String, kind: String, platform: String) : EventListener, PlatformSecretStore {
    private val secrets = ConcurrentHashMap<String, ByteArray>()
    val state = AtomicReference(CoreState())
    val raw = java.util.concurrent.ConcurrentLinkedQueue<String>()
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
        raw.add(eventJson)
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

        // Phase 3 through the bindings: the phone reads the hub's catalog, sends touchpad
        // input built by the app's Input helper, and runs a saved command.
        hub.core.setCatalog(
            Reducer.json.encodeToString(
                Catalog.serializer(),
                Catalog(commands = listOf(CatalogCommand("hello", "Say hello")), system = listOf("lock", "set_volume")),
            ),
        )
        phone.core.requestCatalog(hubId)
        val cat = phone.waitFor("catalog") { it.catalogs[hubId] != null }.catalogs[hubId]!!
        assertEquals("Say hello", cat.commands.single().name)
        phone.core.sendInput(hubId, Input.encode(listOf(Input.move(3f, -2f), Input.click(), Input.key("tab", "alt"), Input.text("hé"))))
        val waitRaw = { what: String, pred: (String) -> Boolean ->
            val end = System.currentTimeMillis() + 10_000
            while (hub.raw.none(pred) && System.currentTimeMillis() < end) Thread.sleep(25)
            assertTrue(hub.raw.any(pred), what)
        }
        waitRaw("remote input") { it.contains("\"remote_input\"") && it.contains("\"hé\"") && it.contains("\"alt\"") }
        phone.core.systemAction(hubId, Input.system("set_volume", 140))
        waitRaw("volume clamped to 100") { it.contains("\"system_requested\"") && it.contains("\"percent\":100") }
        phone.core.runCommand(hubId, "hello")
        waitRaw("run requested") { it.contains("\"run_requested\"") }
        hub.core.reportRun(phoneId, "hello", true, "hi there")
        val result = phone.waitFor("run result") { it.runResults.isNotEmpty() }.runResults.first()
        assertEquals("hi there", result.output)

        // Phase 4 through the bindings: the phone shows the hub's screen.
        val shown = java.util.concurrent.LinkedBlockingQueue<Pair<Long, Int>>()
        phone.core.screenStart(hubId, 1280u, 30.toUByte(), object : FrameListener {
            override fun onFrame(deviceId: String, seq: ULong, width: UInt, height: UInt, latencyMs: UInt, fps: UByte, jpeg: ByteArray) {
                shown.add(seq.toLong() to jpeg.size)
            }
        })
        waitRaw("screen requested") { it.contains("\"screen_requested\"") && it.contains("\"on\":true") }
        val pusher = Thread {
            repeat(40) { i ->
                hub.core.pushScreenFrame(1280u, 800u, byteArrayOf(0xFF.toByte(), 0xD8.toByte(), i.toByte(), 0xFF.toByte(), 0xD9.toByte()))
                Thread.sleep(15)
            }
        }.apply { start() }
        val first = shown.poll(10, java.util.concurrent.TimeUnit.SECONDS)
        assertNotNull(first)
        assertEquals(5, first.second)
        pusher.join()
        phone.core.screenStop(hubId)
        waitRaw("screen stopped") { it.contains("\"screen_requested\"") && it.contains("\"on\":false") }

        // Phase 6 through the bindings, encoded with the app's own models: a new text, the
        // conversation list, a ringing call, "continue on computer", the walk-away token,
        // the phone's microphone and its camera offered as a webcam.
        val waitPhone = { what: String, pred: (String) -> Boolean ->
            val end = System.currentTimeMillis() + 10_000
            while (phone.raw.none(pred) && System.currentTimeMillis() < end) Thread.sleep(25)
            assertTrue(phone.raw.any(pred), what)
        }
        val j = Reducer.json
        // Handoffs ride the clipboard permission, which the phone took away above.
        phone.core.setPermissions(hubId, j.encodeToString(Permissions.serializer(), perms.copy(clipboard = true)))
        phone.core.publishSms(j.encodeToString(SmsMessage.serializer(), SmsMessage("m1", "7", "+15550100", "Ana", "On my way", 1_700_000_000_000, outgoing = false)))
        waitRaw("sms") { it.contains("\"type\":\"sms\"") && it.contains("On my way") && it.contains("\"thread\":\"7\"") }
        val threads = listOf(SmsThread("7", listOf("+15550100"), listOf("Ana"), "On my way", 1_700_000_000_000, unread = 1))
        phone.core.answerSmsThreads(hubId, j.encodeToString(ListSerializer(SmsThread.serializer()), threads))
        waitRaw("sms threads") { it.contains("\"sms_threads\"") && it.contains("\"unread\":1") }
        phone.core.reportSmsResult(hubId, false, "No signal")
        waitRaw("sms result") { it.contains("\"sms_result\"") && it.contains("No signal") }
        phone.core.publishCall(j.encodeToString(CallInfo.serializer(), CallInfo("c1", "ringing", "+15550100", "Ana", incoming = true, ts = 1)))
        waitRaw("call") { it.contains("\"type\":\"call\"") && it.contains("\"ringing\"") }
        phone.core.handoff(hubId, j.encodeToString(HandoffItem.serializer(), HandoffItem("url", url = "https://example.org/a", title = "A")))
        waitRaw("handoff to hub") { it.contains("\"handoff\"") && it.contains("https://example.org/a") }
        hub.core.handoff(phoneId, j.encodeToString(HandoffItem.serializer(), HandoffItem("media", url = "https://youtu.be/x", positionMs = 61_000)))
        waitPhone("handoff to phone") { it.contains("\"handoff\"") && it.contains("\"position_ms\":61000") }
        // Only http(s) links are ever handed over.
        assertFailsWith<DevtiException> { phone.core.handoff(hubId, j.encodeToString(HandoffItem.serializer(), HandoffItem("url", url = "file:///etc/passwd"))) }
        assertEquals(8, phone.core.proximityToken().size)

        val mic = phone.core.startAudio(hubId, "mic", 48_000u, 1.toUByte())
        waitRaw("audio started") { it.contains("\"audio_started\"") && it.contains("\"mic\"") }
        repeat(5) { mic.push(ByteArray(1920)) }
        mic.stop()
        waitRaw("audio ended") { it.contains("\"audio_ended\"") }
        phone.core.cameraOffer(hubId)
        waitRaw("camera offered") { it.contains("\"camera_offered\"") }

        // Phase 7 through the bindings: the hub shares a folder; the phone lists it (into the
        // reducer's state) and downloads from it; remote addresses travel to the phone.
        val sharedDir = Files.createTempDirectory("devti-shared").toFile()
        java.io.File(sharedDir, "notes.txt").writeText("hello from the hub")
        hub.core.setSharedFolders(j.encodeToString(ListSerializer(SharedFolder.serializer()), listOf(SharedFolder("Notes", sharedDir.absolutePath))))
        phone.core.fsList(hubId, "/Notes")
        val listing = phone.waitFor("listing") { it.fsListings["$hubId|/Notes"] != null }.fsListings.getValue("$hubId|/Notes")
        assertEquals("notes.txt", listing.entries.single().name)
        assertEquals(18L, listing.entries.single().size)
        phone.core.fsList(hubId, "/Notes/../..")
        assertNotNull(phone.waitFor("refusal") { it.fsListings["$hubId|/Notes/../.."] != null }.fsListings.getValue("$hubId|/Notes/../..").error)
        phone.core.fsGet(hubId, "/Notes/notes.txt")
        val dl = phone.waitFor("download") { s -> s.transfers.any { it.name == "notes.txt" && it.state == "done" } }
            .transfers.first { it.name == "notes.txt" }
        assertEquals("hello from the hub", java.io.File(dl.path).readText())
        hub.core.setReach(true, "[\"home.example.org:47810\"]")
        waitPhone("reach") { it.contains("\"reach_changed\"") }
        val reach = j.decodeFromString(ReachInfo.serializer(), hub.core.reachInfoJson())
        assertTrue(reach.enabled)
        assertTrue(reach.addresses.any { it.kind == "manual" && it.address == "home.example.org:47810" })
        assertFailsWith<DevtiException> { hub.core.setReach(true, "[\"not an address\"]") }

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
