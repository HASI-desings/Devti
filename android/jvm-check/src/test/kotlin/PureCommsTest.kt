import app.devti.android.continuity.Pure
import app.devti.android.core.state.CallInfo
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** Phase 6 decisions the Android app makes, checked on the JVM. */
class PureCommsTest {
    @Test
    fun sharedTextIsALinkOnlyWhenItLooksLikeOne() {
        assertEquals("https://example.org/a?b=1", Pure.sharedLink("https://example.org/a?b=1"))
        assertEquals("https://youtu.be/abc", Pure.sharedLink("Great talk https://youtu.be/abc"))
        assertEquals("https://example.org/x", Pure.sharedLink("(see https://example.org/x)."))
        assertNull(Pure.sharedLink("just a note"))
        assertNull(Pure.sharedLink("two https://a.org and https://b.org"))
        assertNull(Pure.sharedLink("ftp://example.org/file"))
        assertNull(Pure.sharedLink("x".repeat(200) + " https://example.org"))
    }

    @Test
    fun youtubeResumesAtTheSameSecond() {
        assertEquals("https://youtu.be/abc?t=61s", Pure.mediaUrl("https://youtu.be/abc", 61_500))
        assertEquals("https://youtu.be/abc?t=61s", Pure.mediaUrl("https://youtu.be/abc?t=30", 61_000))
        assertEquals("https://www.youtube.com/watch?v=x&t=5s", Pure.mediaUrl("https://www.youtube.com/watch?v=x&t=1s", 5_000))
        assertEquals("https://example.org/v", Pure.mediaUrl("https://example.org/v", 61_000))
        assertEquals("https://youtu.be/abc", Pure.mediaUrl("https://youtu.be/abc", null))
    }

    @Test
    fun callStatesFoldIntoOneCall() {
        val ringing = Pure.nextCall(null, "RINGING", null, "", 10)!!
        assertEquals("ringing", ringing.state)
        // Android 9+ repeats the change with the number: same call, now with the number.
        val withNumber = Pure.nextCall(ringing, "RINGING", "+15550100", "Ana", 11)!!
        assertEquals(ringing.id, withNumber.id)
        assertEquals("+15550100", withNumber.number)
        assertEquals("Ana", withNumber.name)
        val active = Pure.nextCall(withNumber, "OFFHOOK", null, "", 12)!!
        assertEquals("active", active.state)
        assertEquals("Ana", active.name)
        assertEquals("ended", Pure.nextCall(active, "IDLE", null, "", 13)!!.state)
        assertEquals("missed", Pure.nextCall(withNumber, "IDLE", null, "", 13)!!.state)
        assertNull(Pure.nextCall(null, "IDLE", null, "", 13))
        val outgoing: CallInfo = Pure.nextCall(null, "OFFHOOK", "+15550199", "", 20)!!
        assertEquals(false, outgoing.incoming)
    }

    @Test
    fun nv21RotationKeepsEveryPixel() {
        // 4×2 frame: luma 0..7, chroma pairs (V,U) = (100,200), (101,201).
        val w = 4
        val h = 2
        val frame = byteArrayOf(0, 1, 2, 3, 4, 5, 6, 7, 100, 200.toByte(), 101, 201.toByte())
        // 90° clockwise → 2×4: rows read bottom-to-top of each source column.
        val r90 = Pure.rotateNv21(frame, w, h, 90)
        assertContentEquals(byteArrayOf(4, 0, 5, 1, 6, 2, 7, 3), r90.copyOfRange(0, 8))
        // Chroma is 2×1 → 1×2: the two pairs stack in order.
        assertContentEquals(byteArrayOf(100, 200.toByte(), 101, 201.toByte()), r90.copyOfRange(8, 12))
        val r180 = Pure.rotateNv21(frame, w, h, 180)
        assertContentEquals(byteArrayOf(7, 6, 5, 4, 3, 2, 1, 0, 101, 201.toByte(), 100, 200.toByte()), r180)
        val r270 = Pure.rotateNv21(frame, w, h, 270)
        assertContentEquals(byteArrayOf(3, 7, 2, 6, 1, 5, 0, 4), r270.copyOfRange(0, 8))
        // Four quarter turns are the identity.
        var f = frame
        var dims = w to h
        repeat(4) {
            f = Pure.rotateNv21(f, dims.first, dims.second, 90)
            dims = dims.second to dims.first
        }
        assertContentEquals(frame, f)
        assertContentEquals(frame, Pure.rotateNv21(frame, w, h, 360))
    }
}
