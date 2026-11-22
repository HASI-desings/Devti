package app.devti.android.ui

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.edit
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.devti.android.core.DevtiCore
import app.devti.android.core.DevtiService
import app.devti.android.ui.components.Prism
import app.devti.android.ui.home.Home
import app.devti.android.ui.onboarding.Onboarding
import app.devti.android.ui.theme.Devti

/** Chooses between first-run setup and the home screen. */
@Composable
fun DevtiRoot(core: DevtiCore) {
    val context = LocalContext.current
    val prefs = remember { context.getSharedPreferences("devti_ui", Context.MODE_PRIVATE) }
    val state by core.state.collectAsStateWithLifecycle()
    var onboarded by remember { mutableStateOf(prefs.getBoolean("onboarded", false)) }
    var pairing by remember { mutableStateOf(false) }
    val c = Devti.colors

    when {
        state.error != null -> Column(
            Modifier.fillMaxSize().background(c.bg).safeDrawingPadding().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Prism(96.dp, emptyList(), breathe = false)
            Text("Devti couldn't start", style = Devti.type.title, color = c.text)
            Text(state.error ?: "", style = Devti.type.body, color = c.textDim)
        }
        !state.ready -> Box(Modifier.fillMaxSize().background(c.bg), contentAlignment = Alignment.Center) {
            Prism(96.dp, emptyList(), label = "Starting Devti")
        }
        !onboarded || pairing -> Onboarding(core, state, skipWelcome = onboarded, onFinished = {
            prefs.edit { putBoolean("onboarded", true) }
            onboarded = true
            pairing = false
            DevtiService.start(context)
        })
        else -> Shell(core, state, onPairAnother = {
            core.clearPairing()
            pairing = true
        })
    }
}

private enum class Tab(val label: String) { Home("Home"), Remote("Remote"), Deck("Deck") }

/** Home, the remote for the computer, and its shortcuts deck. */
@Composable
private fun Shell(core: DevtiCore, state: app.devti.android.core.state.CoreState, onPairAnother: () -> Unit) {
    val c = Devti.colors
    var tab by remember { mutableStateOf(Tab.Home) }
    Column(Modifier.fillMaxSize().background(c.bg)) {
        Box(Modifier.weight(1f).statusBarsPadding().consumeWindowInsets(WindowInsets.navigationBars)) {
            when (tab) {
                Tab.Home -> Home(core, state, onPairAnother)
                Tab.Remote -> app.devti.android.ui.remote.Remote(core, state)
                Tab.Deck -> app.devti.android.ui.remote.Deck(core, state)
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .background(c.surface)
                .navigationBarsPadding()
                .padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
        ) {
            Tab.entries.forEach { t ->
                val sel = t == tab
                Column(
                    Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(14.dp))
                        .background(if (sel) c.surface2 else androidx.compose.ui.graphics.Color.Transparent)
                        .clickable { tab = t }
                        .padding(vertical = 10.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Box(Modifier.size(if (sel) 8.dp else 6.dp).clip(CircleShape).background(if (sel) c.text else c.textFaint))
                    Text(t.label, style = Devti.type.label, color = if (sel) c.text else c.textDim, modifier = Modifier.padding(top = 4.dp))
                }
            }
        }
    }
}
