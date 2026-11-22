package app.devti.android.ui

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Parcelable
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.devti.android.DevtiApp
import app.devti.android.core.state.PeerSnapshot
import app.devti.android.ui.components.GhostButton
import app.devti.android.ui.onboarding.VoiceDot
import app.devti.android.ui.theme.Devti
import app.devti.android.ui.theme.DevtiTheme
import app.devti.android.ui.theme.voiceFor

/**
 * "Send with Devti" in Android's share sheet. Files go to the chosen device; plain text
 * (a link, a note) goes to its clipboard. With one online computer it sends straight
 * away; otherwise a small sheet asks which device.
 */
class ShareActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val core = (application as DevtiApp).core
        core.start()
        val uris = sharedUris(intent)
        val text = intent.getStringExtra(Intent.EXTRA_TEXT)?.takeIf { uris.isEmpty() }
        if (uris.isEmpty() && text.isNullOrBlank()) return finish()

        fun send(p: PeerSnapshot) {
            if (uris.isNotEmpty()) {
                core.sendFiles(p.info.id, uris)
                Toast.makeText(this, if (uris.size == 1) "Sending to ${p.info.name}" else "Sending ${uris.size} files to ${p.info.name}", Toast.LENGTH_SHORT).show()
            } else {
                val err = core.sendText(text!!)
                Toast.makeText(this, err ?: "On ${p.info.name}'s clipboard", Toast.LENGTH_SHORT).show()
            }
            finish()
        }

        val online = core.state.value.peers.filter { it.online }
        // Text goes to every connected device's clipboard, so there is nothing to choose.
        if (online.size == 1 || (text != null && online.isNotEmpty())) return send(online.first())

        setContent {
            DevtiTheme {
                val state by core.state.collectAsStateWithLifecycle()
                val c = Devti.colors
                Box(Modifier.fillMaxSize().clickable { finish() }, contentAlignment = Alignment.BottomCenter) {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp))
                            .background(c.surface)
                            .clickable(enabled = false) {}
                            .navigationBarsPadding()
                            .padding(24.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Text(if (uris.size > 1) "Send ${uris.size} files to…" else "Send to…", style = Devti.type.heading, color = c.text)
                        if (state.peers.isEmpty()) {
                            Text("Pair this phone with a computer first.", style = Devti.type.body, color = c.textDim)
                        }
                        state.peers.forEach { p ->
                            val voice = c.voices[voiceFor(p.info.id)] ?: c.hub
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(14.dp))
                                    .background(c.surface2)
                                    .clickable { send(p) }
                                    .padding(16.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                VoiceDot(if (p.online) voice else c.textFaint)
                                Spacer(Modifier.width(12.dp))
                                Column(Modifier.weight(1f)) {
                                    Text(p.info.name, style = Devti.type.label, color = c.text)
                                    Text(
                                        if (p.online) "Connected" else "Offline — it will be sent when it's back",
                                        style = Devti.type.small,
                                        color = c.textDim,
                                    )
                                }
                            }
                        }
                        GhostButton("Cancel", onClick = { finish() })
                    }
                }
            }
        }
    }

    private fun sharedUris(intent: Intent): List<Uri> = when (intent.action) {
        Intent.ACTION_SEND -> listOfNotNull(parcelable(intent, Intent.EXTRA_STREAM))
        Intent.ACTION_SEND_MULTIPLE -> parcelables(intent, Intent.EXTRA_STREAM)
        else -> emptyList()
    }

    @Suppress("DEPRECATION")
    private fun parcelable(intent: Intent, key: String): Uri? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) intent.getParcelableExtra(key, Uri::class.java)
        else intent.getParcelableExtra<Parcelable>(key) as? Uri

    @Suppress("DEPRECATION")
    private fun parcelables(intent: Intent, key: String): List<Uri> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) intent.getParcelableArrayListExtra(key, Uri::class.java).orEmpty()
        else intent.getParcelableArrayListExtra<Parcelable>(key).orEmpty().filterIsInstance<Uri>()
}
