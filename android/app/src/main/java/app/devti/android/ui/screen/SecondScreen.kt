package app.devti.android.ui.screen

import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.devti.android.DevtiApp
import app.devti.android.core.DevtiCore
import app.devti.android.core.state.CoreState
import app.devti.android.core.state.Input
import app.devti.android.ui.components.PrimaryButton
import app.devti.android.ui.remote.KeysPanel
import app.devti.android.ui.remote.computers
import app.devti.android.ui.theme.Devti
import app.devti.android.ui.theme.DevtiTheme
import app.devti.android.ui.theme.voiceFor
import kotlin.math.abs
import kotlin.math.hypot
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** The Screen tab: pick a computer and show its screen full-screen on this phone or tablet. */
@Composable
fun ScreenTab(state: CoreState) {
    val c = Devti.colors
    val context = LocalContext.current
    val list = computers(state)
    Column(
        Modifier.fillMaxSize().background(c.bg).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("Second screen", style = Devti.type.title, color = c.text)
        Text(
            "Show a computer's screen here and use it with your fingers: tap to click, hold to right-click, drag, " +
                "scroll with two fingers and pinch to zoom. The computer chooses what to show in Devti → Screens — the " +
                "whole screen, one window, or a virtual monitor to extend onto.",
            style = Devti.type.body,
            color = c.textDim,
        )
        if (list.isEmpty()) Text("Pair with a computer first.", style = Devti.type.body, color = c.textFaint)
        list.forEach { p ->
            val voice = c.voices[voiceFor(p.info.id)] ?: c.hub
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(18.dp))
                    .background(c.surface)
                    .border(1.dp, voice.copy(alpha = 0.4f), RoundedCornerShape(18.dp))
                    .padding(18.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(10.dp).clip(CircleShape).background(if (p.online) voice else c.textFaint))
                Spacer(Modifier.size(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(p.info.name, style = Devti.type.label, color = c.text)
                    Text(if (p.online) "Ready" else "Offline", style = Devti.type.small, color = c.textDim)
                }
                if (p.online) PrimaryButton("Show here", onClick = { ScreenViewerActivity.open(context, p.info.id, p.info.name) })
            }
        }
    }
}

/** Full-screen viewer; keeps the stream while the device rotates. */
class ScreenViewerActivity : ComponentActivity() {
    private val core get() = (application as DevtiApp).core
    private var device: String = ""

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        WindowCompat.getInsetsController(window, window.decorView).apply {
            hide(WindowInsetsCompat.Type.systemBars())
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
        device = intent.getStringExtra(EXTRA_DEVICE).orEmpty()
        val name = intent.getStringExtra(EXTRA_NAME).orEmpty()
        val error = core.watchScreen(device)
        setContent { DevtiTheme { Viewer(core, device, name, error, onClose = { finish() }) } }
    }

    override fun onDestroy() {
        if (isFinishing) core.stopWatching(device)
        super.onDestroy()
    }

    companion object {
        private const val EXTRA_DEVICE = "device"
        private const val EXTRA_NAME = "name"

        fun open(context: Context, device: String, name: String) {
            context.startActivity(
                Intent(context, ScreenViewerActivity::class.java).putExtra(EXTRA_DEVICE, device).putExtra(EXTRA_NAME, name),
            )
        }
    }
}

@Composable
private fun Viewer(core: DevtiCore, device: String, name: String, error: String?, onClose: () -> Unit) {
    val c = Devti.colors
    val frame by core.frames.collectAsStateWithLifecycle()
    var image by remember { mutableStateOf<ImageBitmap?>(null) }
    var keyboard by remember { mutableStateOf(false) }
    var chrome by remember { mutableStateOf(true) }
    // Where the image sits on screen; written while drawing, read by the touch handler.
    val drawn = remember { arrayOf(Rect.Zero) }
    LaunchedEffect(frame) {
        val f = frame ?: return@LaunchedEffect
        val bmp = withContext(Dispatchers.Default) { BitmapFactory.decodeByteArray(f.jpeg, 0, f.jpeg.size) }
        if (bmp != null) image = bmp.asImageBitmap()
    }
    val send: (List<JsonObject>) -> Unit = { core.sendInput(device, it) }

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        Canvas(
            Modifier
                .fillMaxSize()
                .pointerInput(device) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        val start = down.position
                        val t0 = down.uptimeMillis
                        var fingers = 1
                        var dragging = false
                        var lastSpan = 0f
                        var scrollAcc = 0f
                        var lastPos = start
                        fun frac(o: Offset): Pair<Float, Float> {
                            val r = drawn[0]
                            if (r.width <= 0f) return 0f to 0f
                            return ((o.x - r.left) / r.width).coerceIn(0f, 1f) to ((o.y - r.top) / r.height).coerceIn(0f, 1f)
                        }
                        fun moveTo(o: Offset) = frac(o).let { (x, y) -> buildJsonObject { put("k", "move_to"); put("x", x); put("y", y) } }
                        while (true) {
                            val ev = awaitPointerEvent()
                            val pressed = ev.changes.filter { it.pressed }
                            if (pressed.isEmpty()) break
                            fingers = maxOf(fingers, pressed.size)
                            if (pressed.size >= 2) {
                                val a = pressed[0].position
                                val b = pressed[1].position
                                val span = hypot(a.x - b.x, a.y - b.y)
                                val dy = (pressed[0].position.y - pressed[0].previousPosition.y)
                                if (lastSpan > 0f && abs(span - lastSpan) > 24f) {
                                    // Pinch: zoom in or out the way browsers and editors do.
                                    send(listOf(Input.key(if (span > lastSpan) "=" else "-", "ctrl")))
                                    lastSpan = span
                                } else if (lastSpan == 0f) {
                                    lastSpan = span
                                }
                                scrollAcc += dy / 40f
                                val lines = scrollAcc.toInt()
                                if (lines != 0) {
                                    send(listOf(Input.scroll(0f, -lines.toFloat())))
                                    scrollAcc -= lines
                                }
                            } else if (fingers == 1) {
                                val pos = pressed[0].position
                                if (!dragging && hypot(pos.x - start.x, pos.y - start.y) > viewConfiguration.touchSlop) {
                                    dragging = true
                                    send(listOf(moveTo(start), Input.button("left", true)))
                                }
                                if (dragging && hypot(pos.x - lastPos.x, pos.y - lastPos.y) > 2f) {
                                    send(listOf(moveTo(pos)))
                                    lastPos = pos
                                }
                            }
                            ev.changes.forEach { it.consume() }
                        }
                        val held = android.os.SystemClock.uptimeMillis() - t0
                        when {
                            dragging -> send(listOf(moveTo(lastPos), Input.button("left", false)))
                            fingers >= 2 -> {}
                            held > 550 -> send(listOf(moveTo(start), Input.click("right")))
                            else -> {
                                send(listOf(moveTo(start), Input.click("left")))
                                chrome = !chrome
                            }
                        }
                    }
                },
        ) {
            val img = image ?: return@Canvas
            val scale = minOf(size.width / img.width, size.height / img.height)
            val w = img.width * scale
            val h = img.height * scale
            val left = (size.width - w) / 2f
            val top = (size.height - h) / 2f
            drawn[0] = Rect(left, top, left + w, top + h)
            drawImage(img, dstOffset = IntOffset(left.toInt(), top.toInt()), dstSize = IntSize(w.toInt(), h.toInt()))
        }

        if (image == null) {
            Column(Modifier.align(Alignment.Center).padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    error ?: "Waiting for $name… It needs Devti running and “See this computer's screen” allowed for this phone.",
                    style = Devti.type.body,
                    color = c.textDim,
                    textAlign = TextAlign.Center,
                )
            }
        }

        if (chrome || image == null) {
            Row(
                Modifier.align(Alignment.TopCenter).safeDrawingPadding().padding(12.dp)
                    .clip(RoundedCornerShape(999.dp)).background(Color.Black.copy(alpha = 0.55f)).padding(horizontal = 14.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                Text(name, style = Devti.type.label, color = Color.White)
                frame?.let { Text("${it.fps} fps · ${it.latencyMs} ms", style = Devti.type.small, color = Color.White.copy(alpha = 0.7f)) }
                Text(
                    if (keyboard) "Hide keyboard" else "Keyboard",
                    style = Devti.type.label,
                    color = Color.White,
                    modifier = Modifier.clickable { keyboard = !keyboard },
                )
                Text("Close", style = Devti.type.label, color = Color.White, modifier = Modifier.clickable(onClick = onClose))
            }
        }
        if (keyboard) {
            Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().background(c.bg).safeDrawingPadding().padding(12.dp)) {
                KeysPanel(send)
            }
        }
    }
}
