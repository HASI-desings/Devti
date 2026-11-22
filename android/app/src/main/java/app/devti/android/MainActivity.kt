package app.devti.android

import android.content.ClipboardManager
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import app.devti.android.core.DevtiService
import app.devti.android.core.state.looksLikePairingLink
import app.devti.android.ui.DevtiRoot
import app.devti.android.ui.theme.DevtiTheme

class MainActivity : ComponentActivity() {
    private val core get() = (application as DevtiApp).core

    /** Android only lets the focused app read the clipboard, so watch it while Devti is open. */
    private val clipListener = ClipboardManager.OnPrimaryClipChangedListener { core.captureClipboard() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        core.start()
        if (getSharedPreferences("devti_ui", MODE_PRIVATE).getBoolean("onboarded", false)) {
            DevtiService.start(this)
        }
        handleLink(intent)
        setContent { DevtiTheme { DevtiRoot(core) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleLink(intent)
    }

    override fun onResume() {
        super.onResume()
        core.reconnectNow()
        core.clipboard.addListener(clipListener)
    }

    override fun onPause() {
        core.clipboard.removeListener(clipListener)
        super.onPause()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        // The first moment Android allows reading what was copied in another app.
        if (hasFocus) core.captureClipboard()
    }

    /** A devti://pair link (from the system camera app) starts pairing directly. */
    private fun handleLink(intent: Intent?) {
        val link = intent?.data?.toString() ?: return
        if (looksLikePairingLink(link)) core.pairWithUri(link)
    }
}
