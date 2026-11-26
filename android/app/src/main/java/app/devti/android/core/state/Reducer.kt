package app.devti.android.core.state

import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/**
 * Pure state machine: (state, event JSON) → new state. No Android types, so it is
 * unit-tested on the JVM against the real Rust core (android/jvm-check).
 */
object Reducer {
    val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
        coerceInputValues = true
    }

    const val MAX_TRANSFERS = 50

    /** Builds the initial state from the core's snapshot calls. */
    fun snapshot(
        localJson: String,
        peersJson: String,
        nearbyJson: String,
        transfersJson: String = "[]",
        dnd: Boolean = false,
    ): CoreState =
        CoreState(
            ready = true,
            local = json.decodeFromString(LocalSnapshot.serializer(), localJson),
            peers = sortPeers(json.decodeFromString(ListSerializer(PeerSnapshot.serializer()), peersJson)),
            nearby = json.decodeFromString(ListSerializer(DiscoveredDevice.serializer()), nearbyJson),
            transfers = runCatching { json.decodeFromString(ListSerializer(TransferRecord.serializer()), transfersJson) }
                .getOrDefault(emptyList())
                .sortedByDescending { it.ts }
                .take(MAX_TRANSFERS),
            dnd = dnd,
        )

    fun apply(state: CoreState, eventJson: String): CoreState {
        val obj = runCatching { json.parseToJsonElement(eventJson).jsonObject }.getOrNull() ?: return state
        return when (obj.string("type")) {
            "local_changed" -> state.copy(local = obj.decode("local", LocalSnapshot.serializer()) ?: state.local)
            "peer_changed" -> {
                val peer = obj.decode("peer", PeerSnapshot.serializer()) ?: return state
                state.copy(
                    peers = sortPeers(state.peers.filterNot { it.info.id == peer.info.id } + peer),
                    nearby = state.nearby.filterNot { it.id == peer.info.id },
                )
            }
            "peer_removed" -> {
                val id = obj.string("id") ?: return state
                val gone = state.peers.firstOrNull { it.info.id == id }
                state.copy(
                    peers = state.peers.filterNot { it.info.id == id },
                    removedNotice = if (obj.string("reason") == "revoked_by_peer" && gone != null) {
                        "${gone.info.name} removed this phone from its devices."
                    } else {
                        state.removedNotice
                    },
                )
            }
            "device_discovered" -> {
                val dev = obj.decode("device", DiscoveredDevice.serializer()) ?: return state
                if (state.peers.any { it.info.id == dev.id }) state
                else state.copy(nearby = (state.nearby.filterNot { it.id == dev.id } + dev).sortedBy { it.name })
            }
            "device_lost" -> state.copy(nearby = state.nearby.filterNot { it.id == obj.string("id") })
            "pairing_started" -> obj.long("pairing_id")?.let { state.copy(pairing = PairingStage.Connecting(it)) } ?: state
            "pairing_verify" -> {
                val id = obj.long("pairing_id") ?: return state
                val peer = obj.decode("peer", DeviceInfo.serializer()) ?: return state
                state.copy(pairing = PairingStage.Verify(id, peer, obj.string("sas") ?: ""))
            }
            "pairing_completed" -> {
                val id = obj.long("pairing_id") ?: return state
                val peer = obj.decode("peer", PeerSnapshot.serializer()) ?: return state
                state.copy(
                    pairing = PairingStage.Done(id, peer),
                    peers = sortPeers(state.peers.filterNot { it.info.id == peer.info.id } + peer),
                )
            }
            "pairing_failed" -> {
                val id = obj.long("pairing_id") ?: return state
                state.copy(pairing = PairingStage.Failed(id, obj.string("reason") ?: "other", obj.string("message") ?: ""))
            }
            "paused" -> {
                val paused = obj["paused"]?.jsonPrimitive?.booleanOrNull ?: return state
                state.copy(local = state.local?.copy(paused = paused))
            }
            "transfer" -> {
                val t = obj.decode("transfer", TransferRecord.serializer()) ?: return state
                val rated = t.copy(rateBps = obj.long("rate_bps") ?: 0)
                val list = state.transfers.filterNot { it.id == t.id } + rated
                state.copy(transfers = list.sortedByDescending { it.ts }.take(MAX_TRANSFERS))
            }
            "dnd_changed" -> state.copy(dnd = obj["on"]?.jsonPrimitive?.booleanOrNull ?: return state)
            "clipboard_received" -> state.copy(
                lastClipboard = ClipboardIn(obj.string("from_name") ?: "", obj.string("text") ?: return state, obj.long("ts") ?: 0),
            )
            "ring_requested" -> {
                val on = obj["on"]?.jsonPrimitive?.booleanOrNull ?: return state
                if (on) state.copy(ringing = RingIn(obj.string("from") ?: "", obj.string("from_name") ?: ""))
                else state.copy(ringing = null)
            }
            "location_received" -> {
                val from = obj.string("from") ?: return state
                val loc = obj["location"]?.let { runCatching { json.decodeFromJsonElement(DeviceLocation.serializer(), it) }.getOrNull() }
                    ?: return state
                state.copy(locations = state.locations + (from to loc))
            }
            "fs_listing" -> {
                val from = obj.string("from") ?: return state
                val path = obj.string("path") ?: return state
                val entries = (obj["entries"] as? kotlinx.serialization.json.JsonArray)
                    ?.mapNotNull { runCatching { json.decodeFromJsonElement(FsEntry.serializer(), it) }.getOrNull() }
                    .orEmpty()
                val listing = FsListing(
                    entries = entries,
                    truncated = obj["truncated"]?.jsonPrimitive?.booleanOrNull == true,
                    error = obj.string("error"),
                    at = System.currentTimeMillis(),
                )
                state.copy(fsListings = state.fsListings + ("$from|$path" to listing))
            }
            "catalog_received" -> {
                val from = obj.string("from") ?: return state
                val catalog = obj.decode("catalog", Catalog.serializer()) ?: return state
                state.copy(catalogs = state.catalogs + (from to catalog))
            }
            "run_finished" -> {
                val r = RunResult(
                    from = obj.string("from") ?: return state,
                    command = obj.string("command") ?: "",
                    ok = obj["ok"]?.jsonPrimitive?.booleanOrNull ?: false,
                    output = obj.string("output") ?: "",
                    ts = System.currentTimeMillis(),
                )
                state.copy(runResults = (listOf(r) + state.runResults).take(20))
            }
            "screen_requested" -> {
                val from = obj.string("from") ?: return state
                val on = obj["on"]?.jsonPrimitive?.booleanOrNull ?: return state
                val viewers = if (on) state.screenViewers + (from to (obj.string("from_name") ?: "")) else state.screenViewers - from
                state.copy(screenViewers = viewers, shareStats = if (viewers.isEmpty()) null else state.shareStats)
            }
            "screen_stats" -> {
                if (obj["sharing"]?.jsonPrimitive?.booleanOrNull != true) return state
                state.copy(
                    shareStats = ShareStats(
                        fps = obj.long("fps")?.toInt() ?: 0,
                        latencyMs = obj.long("latency_ms")?.toInt() ?: 0,
                        kbps = obj.long("kbps")?.toInt() ?: 0,
                    ),
                )
            }
            else -> state
        }
    }

    fun sortPeers(peers: List<PeerSnapshot>): List<PeerSnapshot> =
        peers.sortedWith(compareByDescending<PeerSnapshot> { it.online }.thenBy { it.pairedAt })

    private fun JsonObject.string(key: String): String? = this[key]?.jsonPrimitive?.contentOrNull

    private fun JsonObject.long(key: String): Long? = this[key]?.jsonPrimitive?.longOrNull

    private fun <T> JsonObject.decode(key: String, s: kotlinx.serialization.KSerializer<T>): T? =
        this[key]?.let { runCatching { json.decodeFromJsonElement(s, it) }.getOrNull() }
}

/** Quick check before handing scanned text to the core, which validates it fully. */
fun looksLikePairingLink(text: String): Boolean = text.trim().startsWith("devti://pair?")

/** "482913" or "482 913" → "482 913". */
fun formatCode(code: String): String {
    val d = code.filter { it.isDigit() }
    return if (d.length == 6) "${d.substring(0, 3)} ${d.substring(3)}" else code
}
