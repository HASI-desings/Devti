package app.devti.android

import app.devti.android.core.state.CoreState
import app.devti.android.core.state.PairingStage
import app.devti.android.core.state.Reducer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReducerTest {
    private val peer = """{"info":{"id":"AAAAAAAAAAAAAAAA","name":"Studio Laptop","kind":"laptop","platform":"linux",
        "os_version":"Linux","model":null,"app_version":"0.1.0","capabilities":["battery"]},
        "fingerprint":"ab","role":"trusted","permissions":{"screen_view":true,"control":true,"files":true,
        "clipboard":true,"notifications":true,"commands":true,"bluetooth":true},"paired_at":1,"last_seen":null,
        "online":true,"connection":null,"status":{"battery":{"percent":70,"charging":false}}}"""

    @Test
    fun pairingStagesFlow() {
        var s = CoreState(ready = true)
        s = Reducer.apply(s, """{"type":"pairing_started","pairing_id":5,"role":"joiner","peer":null}""")
        assertTrue(s.pairing is PairingStage.Connecting)
        s = Reducer.apply(
            s,
            """{"type":"pairing_verify","pairing_id":5,"role":"joiner","peer":{"id":"AAAAAAAAAAAAAAAA","name":"Studio Laptop"},"sas":"123 456"}""",
        )
        assertEquals("123 456", (s.pairing as PairingStage.Verify).sas)
        s = Reducer.apply(s, """{"type":"pairing_completed","pairing_id":5,"peer":$peer}""")
        assertTrue(s.pairing is PairingStage.Done)
        assertEquals("Studio Laptop", s.hub?.info?.name)
        assertEquals(70, s.hub?.status?.battery?.percent)
    }

    @Test
    fun removalByHubLeavesANotice() {
        var s = Reducer.apply(CoreState(ready = true), """{"type":"peer_changed","peer":$peer}""")
        assertNull(s.removedNotice)
        s = Reducer.apply(s, """{"type":"peer_removed","id":"AAAAAAAAAAAAAAAA","reason":"revoked_by_peer"}""")
        assertTrue(s.peers.isEmpty())
        assertNotNull(s.removedNotice)
    }

    @Test
    fun unknownAndBrokenEventsAreIgnored() {
        val s = CoreState(ready = true)
        assertEquals(s, Reducer.apply(s, "{"))
        assertEquals(s, Reducer.apply(s, """{"type":"hologram"}"""))
    }

    @Test
    fun transfersAreUpsertedNewestFirstWithRate() {
        fun ev(id: Long, state: String, done: Long, ts: Long, rate: Long = 0) =
            """{"type":"transfer","rate_bps":$rate,"transfer":{"id":$id,"device_id":"AAAAAAAAAAAAAAAA","device_name":"Studio Laptop",
            "incoming":true,"name":"a.bin","path":"/x/a.bin","size":100,"done":$done,"state":"$state","ts":$ts}}"""
        var s = Reducer.apply(CoreState(ready = true), ev(1, "active", 10, 5, rate = 2048))
        s = Reducer.apply(s, ev(2, "queued", 0, 9))
        s = Reducer.apply(s, ev(1, "done", 100, 5))
        assertEquals(listOf(2L, 1L), s.transfers.map { it.id })
        assertEquals(1f, s.transfers[1].fraction)
        assertTrue(s.transfers[1].finished)
        assertEquals(0L, s.transfers[1].rateBps)
    }

    @Test
    fun continuityEvents() {
        var s = CoreState(ready = true)
        s = Reducer.apply(s, """{"type":"dnd_changed","on":true,"by":"AAAAAAAAAAAAAAAA"}""")
        assertTrue(s.dnd)
        s = Reducer.apply(s, """{"type":"clipboard_received","from":"AAAAAAAAAAAAAAAA","from_name":"Studio Laptop","text":"hi","ts":3}""")
        assertEquals("hi", s.lastClipboard?.text)
        s = Reducer.apply(s, """{"type":"ring_requested","from":"AAAAAAAAAAAAAAAA","from_name":"Studio Laptop","on":true}""")
        assertEquals("Studio Laptop", s.ringing?.fromName)
        s = Reducer.apply(s, """{"type":"ring_requested","from":"AAAAAAAAAAAAAAAA","from_name":"Studio Laptop","on":false}""")
        assertNull(s.ringing)
    }

    @Test
    fun controlEvents() {
        var s = CoreState(ready = true)
        s = Reducer.apply(s, """{"type":"catalog_received","from":"AAAAAAAAAAAAAAAA","catalog":{"apps":[{"id":"firefox.desktop","name":"Firefox"}],
            "commands":[{"id":"b","name":"Backup","confirm":true}],"deck":[{"name":"Main","buttons":[{"id":"x","label":"Go"}]}],"system":["lock"]}}""")
        assertEquals("Firefox", s.catalogs["AAAAAAAAAAAAAAAA"]?.apps?.single()?.name)
        assertTrue(s.catalogs["AAAAAAAAAAAAAAAA"]?.commands?.single()?.confirm == true)
        s = Reducer.apply(s, """{"type":"run_finished","from":"AAAAAAAAAAAAAAAA","command":"b","ok":false,"output":"disk full"}""")
        assertEquals("disk full", s.runResults.first().output)
        s = Reducer.apply(s, """{"type":"screen_requested","from":"AAAAAAAAAAAAAAAA","from_name":"Studio Laptop","on":true}""")
        assertEquals("Studio Laptop", s.screenViewers["AAAAAAAAAAAAAAAA"])
        s = Reducer.apply(s, """{"type":"screen_stats","peer":"AAAAAAAAAAAAAAAA","sharing":true,"fps":28,"latency_ms":31,"kbps":5200}""")
        assertEquals(28, s.shareStats?.fps)
        s = Reducer.apply(s, """{"type":"screen_requested","from":"AAAAAAAAAAAAAAAA","from_name":"Studio Laptop","on":false}""")
        assertTrue(s.screenViewers.isEmpty())
        assertNull(s.shareStats)
    }
}
