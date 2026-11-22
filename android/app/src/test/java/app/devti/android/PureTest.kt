package app.devti.android

import app.devti.android.continuity.Pure
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PureTest {
    @Test
    fun mirrorFilter() {
        assertTrue(Pure.shouldMirror("org.chat", "app.devti.android", ongoing = false, flags = 0))
        assertFalse(Pure.shouldMirror("app.devti.android", "app.devti.android", false, 0))
        assertFalse(Pure.shouldMirror("org.music", "app.devti.android", ongoing = true, flags = 0))
        assertFalse(Pure.shouldMirror("org.chat", "app.devti.android", false, 0x200)) // group summary
        assertFalse(Pure.shouldMirror("org.nav", "app.devti.android", false, 0x40)) // foreground service
        assertFalse(Pure.shouldMirror("com.android.systemui", "app.devti.android", false, 0))
    }

    @Test
    fun safeNames() {
        assertEquals("passwd", Pure.safeName("../../etc/passwd"))
        assertEquals("photo.jpg", Pure.safeName("C:\\Users\\x\\photo.jpg"))
        assertEquals("abcde.txt", Pure.safeName("a<b>c:d|e?.txt"))
        assertEquals("hidden", Pure.safeName("..hidden"))
        assertEquals("file", Pure.safeName(null))
        assertEquals("file", Pure.safeName("   "))
    }

    @Test
    fun extensionsAndSizes() {
        assertEquals("jpg", Pure.extension("photo.JPG"))
        assertNull(Pure.extension("README"))
        assertEquals("1.5 MB", Pure.bytes(1_572_864))
        assertEquals("820 KB", Pure.bytes(820 * 1024))
        assertEquals("12 B", Pure.bytes(12))
        assertEquals("Waiting to start", Pure.transferLine("queued", 0, 10, 0))
        assertEquals("1.0 MB of 2.0 MB · 512 KB/s · 2 s left", Pure.transferLine("active", 1 shl 20, 2 shl 20, 512 * 1024))
    }

    @Test
    fun inputJsonMatchesTheCore() {
        val json = app.devti.android.core.state.Input.encode(
            listOf(
                app.devti.android.core.state.Input.move(1.5f, -2f),
                app.devti.android.core.state.Input.key("tab", "alt"),
            ),
        )
        assertEquals("""[{"k":"move","dx":1.5,"dy":-2.0},{"k":"key","key":"tab","mods":["alt"]}]""", json)
        assertEquals("""{"a":"set_volume","percent":100}""", app.devti.android.core.state.Input.system("set_volume", 180))
        assertEquals(10f, app.devti.android.core.state.Input.accelerate(10f, 0.1f))
        assertTrue(app.devti.android.core.state.Input.accelerate(10f, 2f) > 20f)
    }
}
