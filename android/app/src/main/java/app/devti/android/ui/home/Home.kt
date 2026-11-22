package app.devti.android.ui.home

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.text.style.TextOverflow
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import app.devti.android.continuity.NotificationMirror
import app.devti.android.continuity.Pure
import app.devti.android.core.state.Permissions
import app.devti.android.core.state.TransferRecord
import androidx.compose.ui.unit.dp
import app.devti.android.core.DevtiCore
import app.devti.android.core.state.CoreState
import app.devti.android.core.state.PeerSnapshot
import app.devti.android.ui.components.BatteryBar
import app.devti.android.ui.components.GhostButton
import app.devti.android.ui.components.Prism
import app.devti.android.ui.components.PrimaryButton
import app.devti.android.ui.components.QuietButton
import app.devti.android.ui.onboarding.VoiceDot
import app.devti.android.ui.theme.Devti
import app.devti.android.ui.theme.voiceFor

/** What works on Android in this release — shown honestly in the app. */
private val WORKS_NOW = listOf(
    "Pairing with a QR code or 6-digit code, verified on both screens",
    "Encrypted connection that reconnects by itself",
    "Battery level, low-battery alerts and history on your computer",
    "Files both ways, any size, resuming after drops (share sheet or the button above)",
    "Clipboard both ways — Android lets Devti read it only while Devti is open",
    "Phone notifications on your computer, with actions, replies and dismiss",
    "Do Not Disturb everywhere at once, and find my phone / computer",
    "Touchpad, keyboard, media, power, apps, commands and presenter for your computer (Remote tab)",
    "Your computer's shortcuts deck (Deck tab)",
    "Your computer sees and controls this phone (screen sharing + Devti control)",
    "This phone or tablet as a touch screen for your computer (Screen tab)",
)

@Composable
fun Home(core: DevtiCore, state: CoreState, onPairAnother: () -> Unit) {
    val c = Devti.colors
    val hub = state.hub
    val local = state.local
    var renaming by remember { mutableStateOf(false) }
    var forget by remember { mutableStateOf<PeerSnapshot?>(null) }
    var access by remember { mutableStateOf<PeerSnapshot?>(null) }

    Column(
        Modifier
            .fillMaxSize()
            .background(c.bg)
            .safeDrawingPadding()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        state.removedNotice?.let {
            Notice(it, onDismiss = core::dismissRemovedNotice)
        }

        // The Hub, drawn as the prism; it lights up in this phone's voice when connected.
        val myVoice = c.voices[voiceFor(local?.info?.id ?: "")] ?: c.hub
        Box(Modifier.fillMaxWidth().padding(top = 16.dp), contentAlignment = Alignment.Center) {
            Prism(150.dp, if (hub?.online == true) listOf(myVoice) else emptyList(), label = "Your computer")
        }
        Text(hub?.info?.name ?: "No computer yet", style = Devti.type.hero, color = c.text)
        Text(
            when {
                local?.paused == true -> "Devti is paused on this phone."
                hub == null -> "Pair with your computer to join its ecosystem."
                hub.online -> "Connected" + (hub.connection?.let { ", ${it.rttMs} ms away" } ?: "")
                else -> "Waiting for ${hub.info.name}. Both devices need to be on the same network."
            },
            style = Devti.type.body,
            color = c.textDim,
        )
        if (hub == null) PrimaryButton("Pair with a computer", onClick = onPairAnother, modifier = Modifier.fillMaxWidth())

        if (hub != null) {
            QuickActions(core, state, hub)
            Transfers(core, state.transfers, state.preparing)
            ContinuitySettings(core, state)
        }

        // This phone.
        Card {
            Row(verticalAlignment = Alignment.CenterVertically) {
                VoiceDot(myVoice)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(local?.info?.name ?: "This phone", style = Devti.type.label, color = c.text)
                    Text("This phone", style = Devti.type.small, color = c.textDim)
                }
                local?.status?.battery?.let { BatteryBar(it.percent, myVoice) }
            }
            GhostButton("Rename", onClick = { renaming = true })
        }

        // Other paired devices.
        if (state.peers.isNotEmpty()) {
            Text("Paired", style = Devti.type.heading, color = c.text)
            state.peers.forEach { p ->
                val voice = c.voices[voiceFor(p.info.id)] ?: c.hub
                Card {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        VoiceDot(if (p.online) voice else c.textFaint)
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(p.info.name, style = Devti.type.label, color = c.text)
                            Text(if (p.online) "Connected" else "Offline", style = Devti.type.small, color = c.textDim)
                        }
                        p.status.battery?.let { BatteryBar(it.percent, voice) }
                    }
                    Row {
                        GhostButton("What it can do here", onClick = { access = p })
                        GhostButton("Remove", onClick = { forget = p })
                    }
                }
            }
            QuietButton("Pair another computer", onClick = onPairAnother, modifier = Modifier.fillMaxWidth())
        }

        // Honest scope for this release.
        Text("What works on Android today", style = Devti.type.heading, color = c.text)
        Card {
            WORKS_NOW.forEach { Text("• $it", style = Devti.type.small, color = c.textDim) }
            Text(
                "Calls, SMS and Bluetooth accessories arrive in the next Devti updates.",
                style = Devti.type.small,
                color = c.textFaint,
            )
        }

        QuietButton(
            if (local?.paused == true) "Resume Devti" else "Pause Devti",
            onClick = { core.setPaused(local?.paused != true) },
            modifier = Modifier.fillMaxWidth(),
            danger = local?.paused != true,
        )
        Spacer(Modifier.height(24.dp))
    }

    if (renaming) {
        var name by remember { mutableStateOf(local?.info?.name ?: "") }
        AlertDialog(
            onDismissRequest = { renaming = false },
            title = { Text("Rename this phone") },
            text = {
                OutlinedTextField(value = name, onValueChange = { name = it.take(48) }, singleLine = true, label = { Text("Name") })
            },
            confirmButton = { TextButton(onClick = { core.rename(name); renaming = false }) { Text("Save") } },
            dismissButton = { TextButton(onClick = { renaming = false }) { Text("Cancel") } },
        )
    }
    access?.let { p ->
        val live = state.peers.firstOrNull { it.info.id == p.info.id } ?: p
        AccessDialog(live, onChange = { core.setPermissions(live.info.id, it) }, onClose = { access = null })
    }
    forget?.let { p ->
        AlertDialog(
            onDismissRequest = { forget = null },
            title = { Text("Remove ${p.info.name}?") },
            text = { Text("It will be disconnected and forgotten on both devices. You can pair again later.") },
            confirmButton = { TextButton(onClick = { core.forget(p.info.id); forget = null }) { Text("Remove") } },
            dismissButton = { TextButton(onClick = { forget = null }) { Text("Keep") } },
        )
    }
}

@Composable
private fun Card(content: @Composable () -> Unit) {
    val c = Devti.colors
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .background(c.surface)
            .border(1.dp, c.line, RoundedCornerShape(18.dp))
            .padding(18.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) { content() }
}

@Composable
private fun Notice(text: String, onDismiss: () -> Unit) {
    val c = Devti.colors
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(c.surface2)
            .padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, style = Devti.type.small, color = c.text, modifier = Modifier.weight(1f))
        GhostButton("OK", onClick = onDismiss)
    }
}

// ------------------------------------------------------------------ Phase 2

@Composable
private fun QuickActions(core: DevtiCore, state: CoreState, hub: PeerSnapshot) {
    val c = Devti.colors
    val context = LocalContext.current
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        core.sendFiles(hub.info.id, uris)
    }
    val ringingOut = hub.info.id in state.ringingOut
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Action(if (ringingOut) "Stop ringing" else "Find ${hub.info.name}", Modifier.weight(1f), active = ringingOut, enabled = hub.online) {
            core.ring(hub.info.id, !ringingOut)?.let { toast(context, it) }
        }
        Action("Send files", Modifier.weight(1f)) { pick.launch(arrayOf("*/*")) }
        Action("Send clipboard", Modifier.weight(1f), enabled = hub.online) {
            toast(context, if (core.captureClipboard(force = true)) "Sent to ${hub.info.name}" else "Nothing new to send — copy some text first")
        }
    }
    state.lastClipboard?.let { clip ->
        Card {
            Text("From ${clip.fromName}'s clipboard", style = Devti.type.small, color = c.textDim)
            Text(clip.text, style = Devti.type.body, color = c.text, maxLines = 3, overflow = TextOverflow.Ellipsis)
            Text("Already on this phone's clipboard — just paste.", style = Devti.type.small, color = c.textFaint)
        }
    }
}

@Composable
private fun Action(label: String, modifier: Modifier, active: Boolean = false, enabled: Boolean = true, onClick: () -> Unit) {
    val c = Devti.colors
    Box(
        modifier
            .height(64.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(if (active) c.text else c.surface)
            .border(1.dp, c.line, RoundedCornerShape(16.dp))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = 10.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            label,
            style = Devti.type.label,
            color = when {
                !enabled -> c.textFaint
                active -> c.bg
                else -> c.text
            },
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun Transfers(core: DevtiCore, transfers: List<TransferRecord>, preparing: Int) {
    if (transfers.isEmpty() && preparing == 0) return
    val c = Devti.colors
    val context = LocalContext.current
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("Files", style = Devti.type.heading, color = c.text, modifier = Modifier.weight(1f))
        if (transfers.any { it.finished }) GhostButton("Clear finished", onClick = { core.clearFinishedTransfers() })
    }
    if (preparing > 0) Text("Getting $preparing file${if (preparing == 1) "" else "s"} ready…", style = Devti.type.small, color = c.textDim)
    transfers.take(8).forEach { t ->
        val voice = c.voices[voiceFor(t.deviceId)] ?: c.hub
        Card {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(t.name, style = Devti.type.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        (if (t.incoming) "from " else "to ") + t.deviceName + " · " + Pure.transferLine(t.state, t.done, t.size, t.rateBps),
                        style = Devti.type.small,
                        color = if (t.state == "failed") c.danger else c.textDim,
                    )
                }
            }
            if (!t.finished || t.state == "done") {
                Box(Modifier.fillMaxWidth().height(4.dp).clip(RoundedCornerShape(2.dp)).background(c.surface3)) {
                    Box(Modifier.fillMaxWidth(t.fraction).height(4.dp).background(if (t.state == "done") c.ok else voice))
                }
            }
            Row {
                when {
                    t.state == "done" && t.incoming -> GhostButton("Open", onClick = { open(context, core, t) })
                    t.state == "failed" || t.state == "cancelled" -> GhostButton("Try again", onClick = { core.retryTransfer(t.id) })
                    !t.finished -> GhostButton("Cancel", onClick = { core.cancelTransfer(t.id) })
                }
            }
        }
    }
}

private fun open(context: android.content.Context, core: DevtiCore, t: TransferRecord) {
    val where = core.files.published(t.id)
    if (where == null) {
        toast(context, "Still moving it to Downloads — try again in a moment")
        return
    }
    val view = Intent(Intent.ACTION_VIEW).setDataAndType(where.uri, where.mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    runCatching { context.startActivity(Intent.createChooser(view, t.name)) }
        .onFailure { toast(context, "No app on this phone can open ${t.name}. It's in Downloads/Devti.") }
}

@Composable
private fun ContinuitySettings(core: DevtiCore, state: CoreState) {
    val c = Devti.colors
    val context = LocalContext.current
    var access by remember { mutableStateOf(NotificationMirror.hasAccess(context)) }
    var mirror by remember { mutableStateOf(core.mirrorNotifications) }
    var clip by remember { mutableStateOf(core.clipboardSync) }
    // Re-check notification access when the user comes back from Android's settings.
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val obs = LifecycleEventObserver { _, e -> if (e == Lifecycle.Event.ON_RESUME) access = NotificationMirror.hasAccess(context) }
        owner.lifecycle.addObserver(obs)
        onDispose { owner.lifecycle.removeObserver(obs) }
    }

    ThisPhoneOnComputer(core, state)

    Text("Everywhere", style = Devti.type.heading, color = c.text)
    Card {
        Toggle(
            "Do Not Disturb, everywhere",
            if (access) "Silences this phone and every connected device together." else "Silences your other devices. Allow notification access to include this phone.",
            state.dnd,
        ) { core.setDnd(it) }
    }
    Card {
        Toggle("Show this phone's notifications on my computer", "Reply, act and dismiss from the computer — it happens here too.", mirror && access) {
            if (!access) {
                context.startActivity(NotificationMirror.settingsIntent(context))
            } else {
                mirror = it
                core.setMirrorNotifications(it)
            }
        }
        if (!access) {
            Text(
                "Android asks you to turn on Notification access for Devti. Devti only sends notifications to your own paired devices, encrypted.",
                style = Devti.type.small,
                color = c.textFaint,
            )
            GhostButton("Allow notification access", onClick = { context.startActivity(NotificationMirror.settingsIntent(context)) })
        }
    }
    Card {
        Toggle(
            "Share the clipboard",
            "Copy on the computer, paste here — instantly. From this phone, Android only lets Devti read what you copied while Devti is open, so open it or tap “Send clipboard”.",
            clip,
        ) {
            clip = it
            core.setClipboardSync(it)
        }
    }
}

@Composable
private fun Toggle(title: String, detail: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    val c = Devti.colors
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.clickable { onChange(!checked) }) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(title, style = Devti.type.label, color = c.text)
            Text(detail, style = Devti.type.small, color = c.textDim)
        }
        Switch(
            checked = checked,
            onCheckedChange = onChange,
            colors = SwitchDefaults.colors(checkedTrackColor = c.text, checkedThumbColor = c.bg, uncheckedTrackColor = c.surface3),
        )
    }
}

@Composable
private fun AccessDialog(p: PeerSnapshot, onChange: (Permissions) -> Unit, onClose: () -> Unit) {
    val perms = p.permissions
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("What ${p.info.name} can do on this phone") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Toggle("Send files", "Files it sends are saved in Downloads/Devti.", perms.files) { onChange(perms.copy(files = it)) }
                Toggle("Clipboard", "Put text on this phone's clipboard.", perms.clipboard) { onChange(perms.copy(clipboard = it)) }
                Toggle("Notifications", "Act on, reply to and dismiss this phone's notifications.", perms.notifications) { onChange(perms.copy(notifications = it)) }
                Toggle("Ring this phone", "Find my phone, at full volume.", perms.commands) { onChange(perms.copy(commands = it)) }
            }
        },
        confirmButton = { TextButton(onClick = onClose) { Text("Done") } },
    )
}

private fun toast(context: android.content.Context, text: String) =
    android.widget.Toast.makeText(context, text, android.widget.Toast.LENGTH_SHORT).show()

/** Phase 3: whether a computer can see and operate this phone, and who is watching now. */
@Composable
private fun ThisPhoneOnComputer(core: DevtiCore, state: CoreState) {
    val c = Devti.colors
    val context = LocalContext.current
    var control by remember { mutableStateOf(app.devti.android.control.ControlAccessibility.enabled(context)) }
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val obs = LifecycleEventObserver { _, e ->
            if (e == Lifecycle.Event.ON_RESUME) control = app.devti.android.control.ControlAccessibility.enabled(context)
        }
        owner.lifecycle.addObserver(obs)
        onDispose { owner.lifecycle.removeObserver(obs) }
    }
    Text("This phone on your computer", style = Devti.type.heading, color = c.text)
    Card {
        if (state.screenViewers.isNotEmpty()) {
            val who = state.screenViewers.values.joinToString()
            Text("$who can see this screen now", style = Devti.type.label, color = c.text)
            state.shareStats?.let { Text("${it.fps} fps · ${it.latencyMs} ms", style = Devti.type.small, color = c.textDim) }
            GhostButton("Stop sharing", onClick = { core.stopSharingScreen() })
        } else {
            Text("Mirror and control", style = Devti.type.label, color = c.text)
            Text(
                "Open this phone from Devti on your computer to see its screen there. Android asks you to allow screen sharing each time.",
                style = Devti.type.small,
                color = c.textDim,
            )
        }
        Toggle(
            "Let the computer tap and type here",
            if (control) "On. Turn it off in Android's Accessibility settings." else "Needs “Devti control” turned on in Android's Accessibility settings.",
            control,
        ) { context.startActivity(app.devti.android.control.ControlAccessibility.settingsIntent()) }
    }
}
