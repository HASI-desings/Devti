package app.devti.android.ui.browse

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.devti.android.continuity.Pure
import app.devti.android.core.DevtiCore
import app.devti.android.core.state.CoreState
import app.devti.android.core.state.FsEntry
import app.devti.android.ui.components.GhostButton
import app.devti.android.ui.theme.Devti
import app.devti.android.ui.theme.voiceFor

/**
 * Phase 7: browse what a computer shares and download from it. Folder taps go deeper,
 * the path row goes back up, and a file tap downloads it into Downloads/Devti.
 */
@Composable
fun Browse(core: DevtiCore, state: CoreState, onClose: () -> Unit) {
    val c = Devti.colors
    val context = LocalContext.current
    val online = state.peers.filter { it.online && it.info.kind != "phone" }
    var device by remember { mutableStateOf(online.firstOrNull()?.info?.id) }
    var path by remember { mutableStateOf("/") }
    var asked by remember { mutableLongStateOf(0L) }
    fun open(id: String, to: String) {
        device = id
        path = to
        asked = System.currentTimeMillis()
        core.fsList(id, to)?.let { android.widget.Toast.makeText(context, it, android.widget.Toast.LENGTH_SHORT).show() }
    }
    LaunchedEffect(device) { device?.let { if (state.fsListings["$it|$path"] == null) open(it, path) } }
    BackHandler {
        if (path == "/") onClose() else open(device ?: return@BackHandler, path.substringBeforeLast('/').ifEmpty { "/" })
    }
    val listing = device?.let { state.fsListings["$it|$path"] }
    val loading = device != null && (listing == null || listing.at < asked)

    Column(
        Modifier
            .fillMaxSize()
            .background(c.bg)
            .safeDrawingPadding()
            .padding(horizontal = 24.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Browse", style = Devti.type.hero, color = c.text, modifier = Modifier.weight(1f))
            GhostButton("Done", onClick = onClose)
        }
        if (online.isEmpty()) {
            Text("No computer is connected right now.", style = Devti.type.body, color = c.textDim)
            return@Column
        }
        // Which computer.
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            online.forEach { p ->
                val voice = c.voices[voiceFor(p.info.id)] ?: c.hub
                val selected = p.info.id == device
                Box(
                    Modifier
                        .clip(RoundedCornerShape(50))
                        .background(if (selected) voice.copy(alpha = 0.18f) else c.surface2)
                        .clickable(role = Role.Tab) { open(p.info.id, "/") }
                        .padding(horizontal = 16.dp, vertical = 10.dp),
                ) {
                    Text(p.info.name, style = Devti.type.label, color = if (selected) c.text else c.textDim)
                }
            }
        }
        // Where we are; each part goes back to it.
        val parts = path.split('/').filter { it.isNotEmpty() }
        Row(Modifier.horizontalScroll(rememberScrollState()), verticalAlignment = Alignment.CenterVertically) {
            Crumb("Shared folders", current = parts.isEmpty()) { device?.let { open(it, "/") } }
            parts.forEachIndexed { i, part ->
                Text(" / ", style = Devti.type.small, color = c.textFaint)
                Crumb(part, current = i == parts.lastIndex) { device?.let { open(it, "/" + parts.take(i + 1).joinToString("/")) } }
            }
        }
        when {
            loading -> Text("Asking…", style = Devti.type.body, color = c.textDim)
            listing?.error != null -> Text(listing.error, style = Devti.type.body, color = c.danger)
            listing != null && listing.entries.isEmpty() -> Text(
                if (path == "/") "This computer doesn't share any folders yet. On the computer: Files → Browse → Share a folder." else "This folder is empty.",
                style = Devti.type.body,
                color = c.textDim,
            )
            listing != null -> LazyColumn(Modifier.fillMaxWidth().weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                items(listing.entries, key = { it.name }) { e ->
                    EntryRow(e) {
                        val id = device ?: return@EntryRow
                        val child = path.trimEnd('/') + "/" + e.name
                        if (e.dir) {
                            open(id, child)
                        } else {
                            val err = core.fsGet(id, child)
                            android.widget.Toast.makeText(
                                context,
                                err ?: "Downloading ${e.name} — it goes to Downloads/Devti.",
                                android.widget.Toast.LENGTH_SHORT,
                            ).show()
                        }
                    }
                }
                if (listing.truncated) item { Text("Showing the first 2,000 items.", style = Devti.type.small, color = c.textFaint) }
            }
        }
    }
}

@Composable
private fun Crumb(text: String, current: Boolean, onClick: () -> Unit) {
    val c = Devti.colors
    Text(
        text,
        style = Devti.type.label,
        color = if (current) c.text else c.textDim,
        modifier = Modifier
            .clip(RoundedCornerShape(8.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 6.dp, vertical = 4.dp),
    )
}

@Composable
private fun EntryRow(e: FsEntry, onClick: () -> Unit) {
    val c = Devti.colors
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(c.surface)
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(10.dp)
                .clip(RoundedCornerShape(if (e.dir) 2.dp else 5.dp))
                .background(if (e.dir) c.text else c.textFaint),
        )
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(e.name, style = Devti.type.label, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                if (e.dir) "Folder" else Pure.bytes(e.size),
                style = Devti.type.small,
                color = c.textDim,
            )
        }
        Text(if (e.dir) "Open" else "Download", style = Devti.type.small, color = c.textDim)
    }
}
