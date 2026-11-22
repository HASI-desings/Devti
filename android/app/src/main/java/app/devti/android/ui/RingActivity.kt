package app.devti.android.ui

import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.devti.android.DevtiApp
import app.devti.android.ui.components.Prism
import app.devti.android.ui.components.PrimaryButton
import app.devti.android.ui.theme.Devti
import app.devti.android.ui.theme.DevtiTheme
import app.devti.android.ui.theme.voiceFor

/**
 * Full-screen "find my phone": shows over the lock screen while the phone rings, with
 * one big "I found it". Closes itself when the ringing stops for any reason.
 */
class RingActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON)
        }
        val core = (application as DevtiApp).core
        setContent {
            DevtiTheme {
                val state by core.state.collectAsStateWithLifecycle()
                val ringing = state.ringing
                LaunchedEffect(ringing) { if (ringing == null) finish() }
                val c = Devti.colors
                val voice = c.voices[voiceFor(ringing?.fromId ?: "")] ?: c.hub
                val wave by rememberInfiniteTransition(label = "ring").animateFloat(
                    0f, 1f, infiniteRepeatable(tween(1600), RepeatMode.Restart), label = "wave",
                )
                Column(
                    Modifier.fillMaxSize().background(c.bgDeep).safeDrawingPadding().padding(28.dp),
                    verticalArrangement = Arrangement.spacedBy(20.dp, Alignment.CenterVertically),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        // Sound rings leaving the prism in the caller's voice.
                        Canvas(Modifier.size(260.dp)) {
                            for (i in 0..2) {
                                val t = (wave + i / 3f) % 1f
                                drawCircle(voice.copy(alpha = (1f - t) * 0.55f), radius = size.minDimension / 2 * (0.35f + 0.65f * t), style = Stroke(3.dp.toPx()))
                            }
                        }
                        Prism(140.dp, listOf(voice), label = "Ringing")
                    }
                    Text("Here I am!", style = Devti.type.hero, color = c.text)
                    Text("${ringing?.fromName ?: "Your computer"} is looking for this phone.", style = Devti.type.body, color = c.textDim)
                    PrimaryButton("I found it", onClick = { core.stopRinging() }, modifier = Modifier.fillMaxWidth())
                }
            }
        }
    }
}
