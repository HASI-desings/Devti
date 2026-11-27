package app.devti.android.guard

import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Text
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.devti.android.DevtiApp
import app.devti.android.ui.components.QuietButton
import app.devti.android.ui.theme.DevtiTheme
import kotlinx.coroutines.delay

/**
 * The stolen screen: black, with the owner's message and contact. As device owner it's
 * pinned (lock task), so home, recents, notifications and the power menu are gone. The
 * only way out is the owner computer turning stolen mode off, or the owner's answer to
 * the question shown here.
 */
class StolenActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON)
        }
        // No screenshots of the owner's contact from inside, and the back button does nothing.
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() = Unit
        })
        val core = (application as DevtiApp).core
        setContent {
            DevtiTheme {
                val state by core.state.collectAsStateWithLifecycle()
                var guard by remember { mutableStateOf(core.guardLocal()) }
                // The owner can turn it off over the network at any moment.
                LaunchedEffect(state.guardVersion) {
                    guard = core.guardLocal()
                    if (guard?.state?.on != true) {
                        Guard.unpin(this@StolenActivity)
                        finish()
                    }
                }
                val g = guard ?: return@DevtiTheme
                var answer by remember { mutableStateOf("") }
                var note by remember { mutableStateOf<String?>(null) }
                var now by remember { mutableStateOf(System.currentTimeMillis()) }
                LaunchedEffect(Unit) {
                    while (true) {
                        delay(1000)
                        now = System.currentTimeMillis()
                    }
                }
                Column(
                    Modifier.fillMaxSize().background(Color.Black).safeDrawingPadding().padding(28.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text("⚠", color = Color(0xFFFF5470), fontSize = 54.sp)
                    Spacer(Modifier.height(12.dp))
                    Text(
                        g.state.message,
                        color = Color.White,
                        fontSize = 28.sp,
                        lineHeight = 34.sp,
                        fontWeight = FontWeight.Bold,
                        textAlign = TextAlign.Center,
                    )
                    if (g.state.contact.isNotBlank()) {
                        Spacer(Modifier.height(16.dp))
                        Text("Owner: ${g.state.contact}", color = Color(0xFFDDDDDD), fontSize = 18.sp, textAlign = TextAlign.Center)
                    }
                    Spacer(Modifier.height(12.dp))
                    Text(
                        "This phone is locked by its owner and is sharing its location.",
                        color = Color(0xFF9A9A9A),
                        fontSize = 14.sp,
                        textAlign = TextAlign.Center,
                    )
                    Spacer(Modifier.height(36.dp))
                    // The owner's offline unlock.
                    Text("Owner's unlock", color = Color(0xFF9A9A9A), fontSize = 13.sp)
                    Text(
                        "Question  ${g.challenge.chunked(4).joinToString(" ")}",
                        color = Color.White,
                        fontSize = 20.sp,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Spacer(Modifier.height(10.dp))
                    BasicTextField(
                        value = answer,
                        onValueChange = { v -> answer = v.filter { it.isDigit() }.take(10) },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword),
                        textStyle = TextStyle(color = Color.White, fontSize = 22.sp, textAlign = TextAlign.Center, letterSpacing = 4.sp),
                        cursorBrush = SolidColor(Color.White),
                        modifier = Modifier
                            .fillMaxWidth(0.8f)
                            .border(1.dp, Color(0xFF444444), RoundedCornerShape(12.dp))
                            .padding(14.dp),
                    )
                    Spacer(Modifier.height(10.dp))
                    val waitS = ((g.retryAt - now + 999) / 1000).coerceAtLeast(0)
                    QuietButton(
                        if (waitS > 0) "Try again in $waitS s" else "Unlock",
                        onClick = {
                            if (waitS > 0) return@QuietButton
                            val r = core.guardTryCode(answer)
                            note = when (r) {
                                true -> null
                                false -> "That's not the answer. A new question is shown."
                                null -> "Please wait before trying again."
                            }
                            answer = ""
                            guard = core.guardLocal()
                        },
                        modifier = Modifier.fillMaxWidth(0.8f),
                    )
                    note?.let {
                        Spacer(Modifier.height(8.dp))
                        Text(it, color = Color(0xFFFF8A9A), fontSize = 13.sp, textAlign = TextAlign.Center)
                    }
                    Spacer(Modifier.height(28.dp))
                    Text(
                        "Emergency? Press the power button once, then tap Emergency call on the lock screen.",
                        color = Color(0xFF777777),
                        fontSize = 12.sp,
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        StolenScreen.visible = true
        Guard.pin(this)
        // Started from here, the location service is allowed even without device owner.
        runCatching { GuardService.start(this) }
    }

    override fun onPause() {
        StolenScreen.visible = false
        super.onPause()
    }

    companion object {
        fun intent(context: Context): Intent =
            Intent(context, StolenActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
    }
}
