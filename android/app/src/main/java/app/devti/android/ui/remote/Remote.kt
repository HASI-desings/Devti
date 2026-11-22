package app.devti.android.ui.remote

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.devti.android.core.DevtiCore
import app.devti.android.core.state.Catalog
import app.devti.android.core.state.CoreState
import app.devti.android.core.state.Input
import app.devti.android.core.state.PeerSnapshot
import app.devti.android.ui.components.GhostButton
import app.devti.android.ui.theme.Devti
import app.devti.android.ui.theme.voiceFor
import kotlin.math.abs
import kotlin.math.hypot
import kotlinx.serialization.json.JsonObject

/** Hardware volume keys turn slides while the presenter is open (MainActivity routes them here). */
object PresenterKeys {
    @Volatile var onVolume: ((up: Boolean) -> Unit)? = null
}

private enum class Panel(val label: String) { Keys("Keyboard"), Media("Media"), Present("Present"), Apps("Apps"), Commands("Commands"), Power("Power") }

/** The computers this phone can control, online ones first. */
internal fun computers(state: CoreState): List<PeerSnapshot> =
    state.peers.filter { it.info.kind == "desktop" || it.info.kind == "laptop" }.ifEmpty { state.peers }

@Composable
fun Remote(core: DevtiCore, state: CoreState) {
    val c = Devti.colors
    var targetId by remember { mutableStateOf<String?>(null) }
    val list = computers(state)
    val target = list.firstOrNull { it.info.id == targetId } ?: list.firstOrNull()
    if (target == null) {
        Empty("Pair with a computer first. Then this screen becomes its touchpad, keyboard and remote.")
        return
    }
    val id = target.info.id
    val voice = c.voices[voiceFor(id)] ?: c.hub
    val catalog = state.catalogs[id]
    LaunchedEffect(id, target.online) { if (target.online) core.requestCatalog(id) }
    var panel by remember { mutableStateOf<Panel?>(null) }
    val allowedControl = target.online

    Column(Modifier.fillMaxSize().background(c.bg).padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Spacer(Modifier.height(8.dp))
        TargetPicker(list, target) { targetId = it }
        if (!allowedControl) {
            Text("${target.info.name} is offline. The remote works when both are on the same network.", style = Devti.type.small, color = c.textDim)
        }
        if (panel == null || panel == Panel.Keys) {
            Touchpad(
                modifier = Modifier.fillMaxWidth().weight(1f),
                accent = voice,
                enabled = allowedControl,
                send = { core.sendInput(id, it) },
            )
        }
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Panel.entries.forEach { p -> Chip(p.label, selected = panel == p) { panel = if (panel == p) null else p } }
        }
        when (panel) {
            Panel.Keys -> KeysPanel { core.sendInput(id, it) }
            Panel.Media -> MediaPanel(catalog, voice, send = { core.sendInput(id, it) }, system = { a, v -> core.systemAction(id, a, v) })
            Panel.Present -> Box(Modifier.weight(1f)) { PresentPanel(voice) { core.sendInput(id, it) } }
            Panel.Apps -> Box(Modifier.weight(1f)) { AppsPanel(catalog) { core.launchApp(id, it) } }
            Panel.Commands -> Box(Modifier.weight(1f)) { CommandsPanel(core, state, id, catalog) }
            Panel.Power -> PowerPanel(target.info.name, catalog) { a -> core.systemAction(id, a) }
            null -> {}
        }
        Spacer(Modifier.height(8.dp))
    }
}

@Composable
private fun TargetPicker(list: List<PeerSnapshot>, target: PeerSnapshot, onPick: (String) -> Unit) {
    val c = Devti.colors
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        list.forEach { p ->
            val voice = c.voices[voiceFor(p.info.id)] ?: c.hub
            val sel = p.info.id == target.info.id
            Row(
                Modifier
                    .clip(RoundedCornerShape(999.dp))
                    .background(if (sel) c.surface2 else Color.Transparent)
                    .border(1.dp, if (sel) voice else c.line, RoundedCornerShape(999.dp))
                    .clickable { onPick(p.info.id) }
                    .padding(horizontal = 14.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(8.dp).clip(CircleShape).background(if (p.online) voice else c.textFaint))
                Spacer(Modifier.size(8.dp))
                Text(p.info.name, style = Devti.type.label, color = c.text, maxLines = 1)
            }
        }
    }
}

/**
 * One finger moves the pointer (accelerated), tap = click, two-finger tap = right click,
 * two fingers drag = scroll, double-tap and hold = drag (button held).
 */
@Composable
private fun Touchpad(modifier: Modifier, accent: Color, enabled: Boolean, send: (List<JsonObject>) -> Unit) {
    val c = Devti.colors
    val haptics = LocalHapticFeedback.current
    var glow by remember { mutableFloatStateOf(0f) }
    Box(
        modifier
            .clip(RoundedCornerShape(28.dp))
            .background(
                Brush.radialGradient(
                    listOf(accent.copy(alpha = 0.10f + glow * 0.12f), c.surface),
                    radius = 900f,
                ),
            )
            .border(1.dp, accent.copy(alpha = 0.35f), RoundedCornerShape(28.dp))
            .semantics { contentDescription = "Touchpad" }
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                var lastTapUp = 0L
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    val start = down.uptimeMillis
                    val dragging = start - lastTapUp < 250
                    if (dragging) send(listOf(Input.button("left", true)))
                    var maxFingers = 1
                    var moved = 0f
                    var accX = 0f
                    var accY = 0f
                    var scrollAcc = 0f
                    var lastSend = start
                    var lastTime = start
                    glow = 1f
                    while (true) {
                        val event = awaitPointerEvent(PointerEventPass.Main)
                        val pressed = event.changes.filter { it.pressed }
                        maxFingers = maxOf(maxFingers, pressed.size)
                        if (pressed.isEmpty()) break
                        val ch = pressed.first()
                        val d = ch.position - ch.previousPosition
                        val dt = (ch.uptimeMillis - lastTime).coerceAtLeast(1)
                        lastTime = ch.uptimeMillis
                        moved += hypot(d.x, d.y)
                        if (pressed.size >= 2) {
                            scrollAcc += d.y / 28f
                            val lines = scrollAcc.toInt()
                            if (lines != 0) {
                                send(listOf(Input.scroll(0f, -lines.toFloat())))
                                scrollAcc -= lines
                            }
                        } else {
                            val speed = hypot(d.x, d.y) / dt
                            accX += Input.accelerate(d.x, speed) / density * 1.4f
                            accY += Input.accelerate(d.y, speed) / density * 1.4f
                            if (ch.uptimeMillis - lastSend >= 12 && (abs(accX) >= 1 || abs(accY) >= 1)) {
                                send(listOf(Input.move(accX, accY)))
                                accX = 0f
                                accY = 0f
                                lastSend = ch.uptimeMillis
                            }
                        }
                        event.changes.forEach { it.consume() }
                    }
                    glow = 0f
                    if (dragging) {
                        send(listOf(Input.button("left", false)))
                    } else if (moved < 18f) {
                        val now = android.os.SystemClock.uptimeMillis()
                        if (now - start < 260) {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            send(listOf(Input.click(if (maxFingers >= 3) "middle" else if (maxFingers == 2) "right" else "left")))
                            if (maxFingers == 1) lastTapUp = now
                        }
                    }
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Touchpad", style = Devti.type.heading, color = c.text.copy(alpha = 0.55f))
            Text(
                "Tap to click · two fingers to scroll or right-click · double-tap and hold to drag",
                style = Devti.type.small,
                color = c.textFaint,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

@Composable
internal fun KeysPanel(send: (List<JsonObject>) -> Unit) {
    val c = Devti.colors
    var buffer by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    LaunchedEffect(Unit) {
        focus.requestFocus()
        keyboard?.show()
    }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        // An invisible sink: whatever the phone's keyboard types is sent as text, and
        // deletions as backspaces, so every language and keyboard app works.
        BasicTextField(
            value = buffer,
            onValueChange = { next ->
                val common = buffer.commonPrefixWith(next).length
                val deleted = buffer.length - common
                val added = next.substring(common)
                val events = mutableListOf<JsonObject>()
                repeat(deleted.coerceAtMost(64)) { events += Input.key("backspace") }
                if (added.isNotEmpty()) events += Input.text(added)
                if (events.isNotEmpty()) send(events)
                buffer = if (next.length > 200) "" else next
            },
            modifier = Modifier
                .fillMaxWidth()
                .focusRequester(focus)
                .clip(RoundedCornerShape(14.dp))
                .background(c.surface2)
                .padding(14.dp),
            textStyle = TextStyle(color = c.text, fontSize = 16.sp),
            cursorBrush = SolidColor(c.text),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Send, autoCorrectEnabled = true),
            keyboardActions = KeyboardActions(onSend = { send(listOf(Input.key("enter"))) }),
            decorationBox = { inner ->
                if (buffer.isEmpty()) Text("Type here — it appears on the computer", style = Devti.type.body, color = c.textFaint)
                inner()
            },
        )
        val keys = listOf(
            "Esc" to Input.key("escape"), "Tab" to Input.key("tab"), "Enter" to Input.key("enter"), "⌫" to Input.key("backspace"),
            "←" to Input.key("left"), "↑" to Input.key("up"), "↓" to Input.key("down"), "→" to Input.key("right"),
            "Copy" to Input.key("c", "ctrl"), "Paste" to Input.key("v", "ctrl"), "Undo" to Input.key("z", "ctrl"), "Switch app" to Input.key("tab", "alt"),
        )
        keys.chunked(4).forEach { row ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                row.forEach { (label, ev) -> KeyCap(label, Modifier.weight(1f)) { send(listOf(ev)) } }
            }
        }
    }
}

@Composable
private fun MediaPanel(catalog: Catalog?, voice: Color, send: (List<JsonObject>) -> Unit, system: (String, Int?) -> Unit) {
    val c = Devti.colors
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            KeyCap("⏮", Modifier.weight(1f)) { send(listOf(Input.media("previous"))) }
            KeyCap("⏯", Modifier.weight(1.4f), accent = voice) { send(listOf(Input.media("play_pause"))) }
            KeyCap("⏭", Modifier.weight(1f)) { send(listOf(Input.media("next"))) }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            KeyCap("Volume −", Modifier.weight(1f)) { send(listOf(Input.media("volume_down"))) }
            KeyCap("Mute", Modifier.weight(1f)) { send(listOf(Input.media("mute"))) }
            KeyCap("Volume +", Modifier.weight(1f)) { send(listOf(Input.media("volume_up"))) }
        }
        if (catalog?.system?.contains("set_volume") == true) LevelSlider("Volume", voice) { system("set_volume", it) }
        if (catalog?.system?.contains("set_brightness") == true) LevelSlider("Brightness", voice) { system("set_brightness", it) }
        if (catalog == null) Text("Waiting for the computer…", style = Devti.type.small, color = c.textFaint)
    }
}

@Composable
private fun LevelSlider(label: String, voice: Color, onSet: (Int) -> Unit) {
    val c = Devti.colors
    var value by remember { mutableFloatStateOf(50f) }
    Column {
        Text("$label · ${value.toInt()} %", style = Devti.type.small, color = c.textDim)
        Slider(
            value = value,
            onValueChange = { value = it },
            onValueChangeFinished = { onSet(value.toInt()) },
            valueRange = 0f..100f,
            colors = SliderDefaults.colors(thumbColor = voice, activeTrackColor = voice, inactiveTrackColor = c.surface3),
        )
    }
}

@Composable
private fun PresentPanel(voice: Color, send: (List<JsonObject>) -> Unit) {
    val c = Devti.colors
    DisposableEffect(Unit) {
        PresenterKeys.onVolume = { up -> send(listOf(Input.key(if (up) "left" else "right"))) }
        onDispose { PresenterKeys.onVolume = null }
    }
    Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(Modifier.fillMaxWidth().weight(1f), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            BigKey("‹ Back", Modifier.weight(1f).fillMaxSize(), c.surface2) { send(listOf(Input.key("left"))) }
            BigKey("Next ›", Modifier.weight(1.6f).fillMaxSize(), voice.copy(alpha = 0.28f)) { send(listOf(Input.key("right"))) }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            KeyCap("Start", Modifier.weight(1f)) { send(listOf(Input.key("f5"))) }
            KeyCap("Black screen", Modifier.weight(1.4f)) { send(listOf(Input.key("b"))) }
            KeyCap("End", Modifier.weight(1f)) { send(listOf(Input.key("escape"))) }
        }
        Text("The phone's volume buttons also turn slides.", style = Devti.type.small, color = c.textFaint)
    }
}

@Composable
private fun AppsPanel(catalog: Catalog?, launch: (String) -> Unit) {
    val c = Devti.colors
    var query by remember { mutableStateOf("") }
    val apps = catalog?.apps.orEmpty().filter { query.isBlank() || it.name.contains(query, ignoreCase = true) }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        BasicTextField(
            value = query,
            onValueChange = { query = it },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.surface2).padding(12.dp),
            textStyle = TextStyle(color = c.text, fontSize = 15.sp),
            cursorBrush = SolidColor(c.text),
            decorationBox = { inner ->
                if (query.isEmpty()) Text("Search apps", style = Devti.type.body, color = c.textFaint)
                inner()
            },
        )
        if (catalog == null) Text("Asking the computer for its apps…", style = Devti.type.small, color = c.textFaint)
        else if (apps.isEmpty()) Text("No apps to show. It needs “Run saved commands” allowed for this phone.", style = Devti.type.small, color = c.textFaint)
        Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            apps.forEach { a ->
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable { launch(a.id) }.padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(34.dp).clip(RoundedCornerShape(10.dp)).background(c.surface2), contentAlignment = Alignment.Center) {
                        Text(a.name.take(1).uppercase(), style = Devti.type.label, color = c.text)
                    }
                    Spacer(Modifier.size(12.dp))
                    Text(a.name, style = Devti.type.body, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

@Composable
private fun CommandsPanel(core: DevtiCore, state: CoreState, id: String, catalog: Catalog?) {
    val c = Devti.colors
    var confirm by remember { mutableStateOf<Pair<String, String>?>(null) }
    val results = state.runResults.filter { it.from == id }.take(3)
    Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        val commands = catalog?.commands.orEmpty()
        if (commands.isEmpty()) {
            Text("No saved commands on this computer yet. Add them in Devti → Control → Saved commands.", style = Devti.type.small, color = c.textDim)
        }
        commands.forEach { cmd ->
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.surface).padding(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(cmd.name, style = Devti.type.label, color = c.text, modifier = Modifier.weight(1f))
                GhostButton("Run", onClick = { if (cmd.confirm) confirm = cmd.id to cmd.name else core.runCommand(id, cmd.id) })
            }
        }
        results.forEach { r ->
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(c.bgDeep).padding(12.dp)) {
                Text(
                    (commands.firstOrNull { it.id == r.command }?.name ?: r.command) + if (r.ok) " · done" else " · failed",
                    style = Devti.type.small,
                    color = if (r.ok) c.ok else c.danger,
                )
                if (r.output.isNotBlank()) Text(r.output, style = Devti.type.small, color = c.textDim, maxLines = 6, overflow = TextOverflow.Ellipsis)
            }
        }
    }
    confirm?.let { (cmdId, name) ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text("Run “$name”?") },
            confirmButton = { TextButton(onClick = { core.runCommand(id, cmdId); confirm = null }) { Text("Run") } },
            dismissButton = { TextButton(onClick = { confirm = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun PowerPanel(name: String, catalog: Catalog?, act: (String) -> Unit) {
    val c = Devti.colors
    var confirm by remember { mutableStateOf<Pair<String, String>?>(null) }
    val all = listOf("lock" to "Lock", "sleep" to "Sleep", "restart" to "Restart", "shutdown" to "Shut down", "sign_out" to "Sign out")
    val available = all.filter { catalog?.system?.contains(it.first) == true }
    if (available.isEmpty()) {
        Text("Power actions need “Run saved commands” allowed for this phone on $name.", style = Devti.type.small, color = c.textDim)
    }
    available.chunked(3).forEach { row ->
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            row.forEach { (a, label) ->
                KeyCap(label, Modifier.weight(1f)) {
                    if (a == "lock" || a == "sleep") act(a) else confirm = a to label
                }
            }
        }
    }
    confirm?.let { (a, label) ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text("$label $name?") },
            text = { Text("Unsaved work on it may be lost.") },
            confirmButton = { TextButton(onClick = { act(a); confirm = null }) { Text(label) } },
            dismissButton = { TextButton(onClick = { confirm = null }) { Text("Cancel") } },
        )
    }
}

// ------------------------------------------------------------------- deck

@Composable
fun Deck(core: DevtiCore, state: CoreState) {
    val c = Devti.colors
    val haptics = LocalHapticFeedback.current
    val target = computers(state).firstOrNull()
    if (target == null) {
        Empty("Pair with a computer first. Its shortcuts deck appears here.")
        return
    }
    val id = target.info.id
    LaunchedEffect(id, target.online) { if (target.online) core.requestCatalog(id) }
    val deck = state.catalogs[id]?.deck.orEmpty()
    var page by remember { mutableStateOf(0) }
    Column(Modifier.fillMaxSize().background(c.bgDeep).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("${target.info.name}'s deck", style = Devti.type.title, color = c.text)
        if (deck.isEmpty()) {
            Text(
                if (target.online) "Design buttons in Devti on the computer: Control → Shortcuts deck. They show up here."
                else "${target.info.name} is offline.",
                style = Devti.type.body,
                color = c.textDim,
            )
            return@Column
        }
        if (deck.size > 1) {
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                deck.forEachIndexed { i, p -> Chip(p.name, selected = i == page) { page = i } }
            }
        }
        val buttons = deck.getOrNull(page)?.buttons.orEmpty()
        Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            buttons.chunked(3).forEach { row ->
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    row.forEach { b ->
                        val voice = c.voices[b.voice] ?: c.hub
                        Box(
                            Modifier
                                .weight(1f)
                                .aspectRatio(1f)
                                .clip(RoundedCornerShape(22.dp))
                                .background(Brush.radialGradient(listOf(voice.copy(alpha = 0.42f), voice.copy(alpha = 0.10f)), radius = 320f))
                                .border(1.dp, voice.copy(alpha = 0.55f), RoundedCornerShape(22.dp))
                                .clickable(enabled = target.online) {
                                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                    core.deckPress(id, b.id)
                                }
                                .padding(10.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                Box(Modifier.size(34.dp).clip(CircleShape).background(voice), contentAlignment = Alignment.Center) {
                                    Text(b.label.take(1).uppercase(), style = Devti.type.label, color = c.bg)
                                }
                                Spacer(Modifier.size(8.dp))
                                Text(b.label, style = Devti.type.small, color = c.text, textAlign = TextAlign.Center, maxLines = 2)
                            }
                        }
                    }
                    repeat(3 - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- pieces

@Composable
private fun Chip(label: String, selected: Boolean, onClick: () -> Unit) {
    val c = Devti.colors
    Box(
        Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(if (selected) c.text else c.surface)
            .border(1.dp, c.line, RoundedCornerShape(999.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 8.dp),
    ) { Text(label, style = Devti.type.label, color = if (selected) c.bg else c.text) }
}

@Composable
private fun KeyCap(label: String, modifier: Modifier, accent: Color? = null, onClick: () -> Unit) {
    val c = Devti.colors
    val haptics = LocalHapticFeedback.current
    Box(
        modifier
            .height(52.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(accent?.copy(alpha = 0.25f) ?: c.surface)
            .border(1.dp, accent ?: c.line, RoundedCornerShape(14.dp))
            .clickable {
                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                onClick()
            },
        contentAlignment = Alignment.Center,
    ) { Text(label, style = Devti.type.label, color = c.text, maxLines = 1) }
}

@Composable
private fun BigKey(label: String, modifier: Modifier, bg: Color, onClick: () -> Unit) {
    val c = Devti.colors
    val haptics = LocalHapticFeedback.current
    Box(
        modifier
            .clip(RoundedCornerShape(24.dp))
            .background(bg)
            .border(1.dp, c.line, RoundedCornerShape(24.dp))
            .clickable {
                haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                onClick()
            },
        contentAlignment = Alignment.Center,
    ) { Text(label, style = Devti.type.title, color = c.text) }
}

@Composable
private fun Empty(text: String) {
    val c = Devti.colors
    Box(Modifier.fillMaxSize().background(c.bg).padding(32.dp), contentAlignment = Alignment.Center) {
        Text(text, style = Devti.type.body, color = c.textDim, textAlign = TextAlign.Center)
    }
}
