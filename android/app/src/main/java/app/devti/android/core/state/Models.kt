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
) {
    /** The Hub this phone belongs to: the first paired computer. */
    val hub: PeerSnapshot?
        get() = peers.firstOrNull { it.info.kind == "desktop" || it.info.kind == "laptop" } ?: peers.firstOrNull()
}
