package app.devti.android.core.state

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Kotlin mirror of the JSON emitted by Devti Core (core/src/node/events.rs).
// Same shapes the desktop UI reads — see docs/PROTOCOL.md § Host API.

@Serializable
data class DeviceInfo(
    val id: String,
    val name: String,
    val kind: String = "unknown",
    val platform: String = "other",
    @SerialName("os_version") val osVersion: String = "",
    val model: String? = null,
    @SerialName("app_version") val appVersion: String = "",
    val capabilities: List<String> = emptyList(),
)

@Serializable
data class BatteryState(val percent: Int, val charging: Boolean)

@Serializable
data class DeviceStatus(val battery: BatteryState? = null)

@Serializable
data class Permissions(
    @SerialName("screen_view") val screenView: Boolean,
    val control: Boolean,
    val files: Boolean,
    val clipboard: Boolean,
    val notifications: Boolean,
    val commands: Boolean,
    val bluetooth: Boolean,
    /** Browse and download from this device's shared folders (Phase 7). */
    val browse: Boolean = false,
)

@Serializable
data class ConnectionSnapshot(
    val kind: String,
    val address: String,
    @SerialName("rtt_ms") val rttMs: Int,
    val since: Long,
)

@Serializable
data class PeerSnapshot(
    val info: DeviceInfo,
    val fingerprint: String,
    val role: String,
    val permissions: Permissions,
    @SerialName("paired_at") val pairedAt: Long,
    @SerialName("last_seen") val lastSeen: Long? = null,
    val online: Boolean,
    val connection: ConnectionSnapshot? = null,
    val status: DeviceStatus = DeviceStatus(),
)

@Serializable
data class LocalSnapshot(
    val info: DeviceInfo,
    val fingerprint: String,
    val port: Int,
    val addresses: List<String> = emptyList(),
    val status: DeviceStatus = DeviceStatus(),
    val paused: Boolean = false,
    @SerialName("discovery_enabled") val discoveryEnabled: Boolean = true,
    @SerialName("identity_origin") val identityOrigin: String = "loaded",
    @SerialName("secret_backend") val secretBackend: String = "",
    @SerialName("secret_os_protected") val secretOsProtected: Boolean = true,
)

@Serializable
data class DiscoveredDevice(
    val id: String,
    val name: String,
    val kind: String = "unknown",
    val platform: String = "other",
    val addrs: List<String> = emptyList(),
    val fingerprint: String? = null,
    @SerialName("pairing_open") val pairingOpen: Boolean = false,
)

/** One file transfer, either direction (core `TransferRecord`). */
@Serializable
data class TransferRecord(
    val id: Long,
    @SerialName("device_id") val deviceId: String,
    @SerialName("device_name") val deviceName: String,
    val incoming: Boolean,
    val name: String,
    val path: String = "",
    val size: Long,
    val done: Long = 0,
    /** `queued`, `active`, `paused`, `done`, `failed`, `cancelled`. */
    val state: String,
    val ts: Long = 0,
    /** Bytes per second, from the latest progress event (not persisted by the core). */
    val rateBps: Long = 0,
) {
    val finished: Boolean get() = state == "done" || state == "failed" || state == "cancelled"
    val fraction: Float get() = if (size <= 0) (if (state == "done") 1f else 0f) else (done.toFloat() / size).coerceIn(0f, 1f)
}

@Serializable
data class NotifAction(val id: String, val label: String, val reply: Boolean = false)

/** A notification as Devti Core carries it (`NotificationInfo`). */
@Serializable
data class NotificationInfo(
    val key: String,
    @SerialName("app_id") val appId: String,
    @SerialName("app_name") val appName: String,
    val title: String = "",
    val text: String = "",
    val ts: Long,
    val actions: List<NotifAction> = emptyList(),
    @SerialName("icon_png") val iconPng: List<Int>? = null,
    val dismissable: Boolean = true,
)

@Serializable
data class MirroredNotification(
    val from: String,
    @SerialName("from_name") val fromName: String,
    val notif: NotificationInfo,
)

// ------------------------------------------------------------------ Phase 3

@Serializable
data class CatalogApp(val id: String, val name: String)

@Serializable
data class CatalogCommand(val id: String, val name: String, val confirm: Boolean = false)

@Serializable
data class DeckButton(val id: String, val label: String, val icon: String = "", val voice: String = "")

@Serializable
data class DeckPage(val name: String, val buttons: List<DeckButton> = emptyList())

/** What a device offers to remotes. */
@Serializable
data class Catalog(
    val apps: List<CatalogApp> = emptyList(),
    val commands: List<CatalogCommand> = emptyList(),
    val deck: List<DeckPage> = emptyList(),
    val system: List<String> = emptyList(),
)

/** Outcome of a command this phone ran on another device. */
data class RunResult(val from: String, val command: String, val ok: Boolean, val output: String, val ts: Long)

/** Live numbers while this phone's screen is being watched. */
data class ShareStats(val fps: Int, val latencyMs: Int, val kbps: Int)

// ------------------------------------------------------------------ Phase 6

@Serializable
data class SmsThread(
    val id: String,
    val addresses: List<String>,
    val names: List<String> = emptyList(),
    val snippet: String,
    val ts: Long,
    val unread: Int = 0,
)

@Serializable
data class SmsMessage(
    val id: String,
    val thread: String,
    val address: String,
    val name: String = "",
    val body: String,
    val ts: Long,
    val outgoing: Boolean,
)

@Serializable
data class CallInfo(
    val id: String,
    /** ringing, active, ended, missed. */
    val state: String,
    val number: String,
    val name: String = "",
    val incoming: Boolean,
    val ts: Long,
)

@Serializable
data class HandoffItem(
    /** url, text, media. */
    val kind: String,
    val url: String = "",
    val title: String = "",
    val text: String = "",
    @SerialName("position_ms") val positionMs: Long? = null,
)

// ------------------------------------------------------------------ Phase 7

@Serializable
data class FsEntry(
    val name: String,
    val dir: Boolean,
    val size: Long = 0,
    val modified: Long = 0,
)

/** A listing of another device's shared folder, as it last arrived. */
data class FsListing(
    val entries: List<FsEntry>,
    val truncated: Boolean,
    val error: String?,
    val at: Long,
)

@Serializable
data class ReachAddress(
    val address: String,
    /** lan, vpn, internet or manual. */
    val kind: String,
    val `interface`: String = "",
)

@Serializable
data class ReachInfo(
    val enabled: Boolean,
    val port: Int,
    val addresses: List<ReachAddress>,
    val extra: List<String>,
)

/** Stolen mode as this phone sees it (`LocalGuard` from the core). */
@Serializable
data class GuardStateModel(
    val on: Boolean = false,
    val message: String = "",
    val contact: String = "",
    @SerialName("sim_lock") val simLock: Boolean = false,
    val reason: String = "",
    val since: Long = 0,
)

@Serializable
data class LocalGuard(
    val owner: String? = null,
    @SerialName("owner_name") val ownerName: String? = null,
    val state: GuardStateModel = GuardStateModel(),
    val challenge: String = "",
    val attempts: Int = 0,
    @SerialName("retry_at") val retryAt: Long = 0,
)

/** Where a device is (Find my device). */
@Serializable
data class DeviceLocation(
    val lat: Double? = null,
    val lon: Double? = null,
    @SerialName("accuracy_m") val accuracyM: Float? = null,
    val ts: Long = 0,
    val source: String = "none",
    @SerialName("public_ip") val publicIp: String? = null,
    @SerialName("local_ips") val localIps: List<String> = emptyList(),
    val network: String? = null,
    val battery: Int? = null,
    val error: String? = null,
    val live: Boolean = false,
)

/** Connecting from anywhere (built in since 1.1). */
@Serializable
data class AnywhereInfo(
    val enabled: Boolean,
    val running: Boolean,
    val online: Boolean,
    val relay: String? = null,
    @SerialName("custom_relay") val customRelay: String = "",
    @SerialName("public_ip") val publicIp: String? = null,
    val error: String? = null,
)

@Serializable
data class SharedFolder(val name: String, val path: String)

/** Text another device put on this phone's clipboard. */
data class ClipboardIn(val fromName: String, val text: String, val ts: Long)

/** Someone is ringing this phone (find my device). */
data class RingIn(val fromId: String, val fromName: String)

/** Where a pairing attempt is, as the phone's UI shows it. */
sealed interface PairingStage {
    val pairingId: Long

    data class Connecting(override val pairingId: Long) : PairingStage
    data class Verify(
        override val pairingId: Long,
        val peer: DeviceInfo,
        val sas: String,
        val confirmed: Boolean = false,
    ) : PairingStage
    data class Done(override val pairingId: Long, val peer: PeerSnapshot) : PairingStage
    data class Failed(override val pairingId: Long, val reason: String, val message: String) : PairingStage
}

/** Everything the phone UI renders. */
data class CoreState(
    val ready: Boolean = false,
    val error: String? = null,
    val local: LocalSnapshot? = null,
    val peers: List<PeerSnapshot> = emptyList(),
    val nearby: List<DiscoveredDevice> = emptyList(),
    val pairing: PairingStage? = null,
    /** Devices removed by the other side, to tell the user once. */
    val removedNotice: String? = null,
    /** Ecosystem-wide Do Not Disturb. */
    val dnd: Boolean = false,
    /** Newest first, at most [Reducer.MAX_TRANSFERS]. */
    val transfers: List<TransferRecord> = emptyList(),
    val lastClipboard: ClipboardIn? = null,
    val ringing: RingIn? = null,
    /** Device ids this phone is currently ringing. */
    val ringingOut: Set<String> = emptySet(),
    /** Files being copied out of other apps before they can be sent. */
    val preparing: Int = 0,
    /** Catalogs of other devices (their apps, commands, deck), by device id. */
    val catalogs: Map<String, Catalog> = emptyMap(),
    val runResults: List<RunResult> = emptyList(),
    /** Devices watching this phone's screen: id → name. */
    val screenViewers: Map<String, String> = emptyMap(),
    val shareStats: ShareStats? = null,
    /** Shared-folder listings by "deviceId|path" (Phase 7). */
    val fsListings: Map<String, FsListing> = emptyMap(),
    // ---- Phase 6 (phone-side; not from the core snapshot)
    /** "mic" or "relay" while this phone streams audio to a computer. */
    val audioSharing: Set<String> = emptySet(),
    /** This phone's camera is a computer's webcam right now. */
    val cameraSharing: Boolean = false,
    /** Advertising the walk-away beacon. */
    val walkAway: Boolean = false,
    /** Find my device: where each device last said it was, by device id. */
    val locations: Map<String, DeviceLocation> = emptyMap(),
    /** A computer asking to protect this phone: (device id, name). */
    val guardClaim: Pair<String, String>? = null,
    /** Bumped whenever stolen mode changes, so screens re-read it. */
    val guardVersion: Int = 0,
) {
    /** The Hub this phone belongs to: the first paired computer. */
    val hub: PeerSnapshot?
        get() = peers.firstOrNull { it.info.kind == "desktop" || it.info.kind == "laptop" } ?: peers.firstOrNull()
}
