package app.devti.android

import app.devti.android.comms.Messaging
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MessagingTest {
    @Test
    fun voiceMessageAppsAreRecognised() {
        for (p in listOf("com.whatsapp", "com.whatsapp.w4b", "com.instagram.android", "org.telegram.messenger", "org.telegram.plus", "com.facebook.orca")) {
            assertTrue(p, Messaging.isMessaging(p))
        }
    }

    @Test
    fun musicAppsAreNot() {
        for (p in listOf("com.spotify.music", "com.google.android.youtube", "com.google.android.apps.youtube.music", null)) {
            assertFalse(p ?: "null", Messaging.isMessaging(p))
        }
    }
}
