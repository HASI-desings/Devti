//! Events and snapshots the node publishes to UI layers (Tauri, UniFFI, CLI).
//!
//! Everything here serialises to JSON with a `type` tag, so the desktop UI and the
//! mobile apps consume exactly the same shapes.

use serde::{Deserialize, Serialize};

use crate::discovery::DiscoveredDevice;
use crate::model::{ConnectionKind, DeviceId, DeviceInfo, DeviceStatus, Permissions, TrustRole};
use crate::protocol::{
    CallInfo, Catalog, HandoffItem, InputEvent, PairAbortReason, SmsMessage, SmsThread, SystemAction,
};
use crate::store::TransferRecord;

pub use super::continuity::MirroredNotification;

/// Live connection details for an online peer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionSnapshot {
    pub kind: ConnectionKind,
    pub address: String,
    pub rtt_ms: u32,
    /// Unix ms when this session started.
    pub since: i64,
}

/// Everything the UI needs to draw a paired device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerSnapshot {
    pub info: DeviceInfo,
    /// SHA-256 SPKI fingerprint, grouped hex (for manual verification).
    pub fingerprint: String,
    pub role: TrustRole,
    pub permissions: Permissions,
    pub paired_at: i64,
    pub last_seen: Option<i64>,
    pub online: bool,
    pub connection: Option<ConnectionSnapshot>,
    pub status: DeviceStatus,
}

/// This device as seen by its own UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSnapshot {
    pub info: DeviceInfo,
    pub fingerprint: String,
    pub port: u16,
    pub addresses: Vec<String>,
    pub status: DeviceStatus,
    pub paused: bool,
    pub discovery_enabled: bool,
    /// `"created"`, `"loaded"` or `"regenerated"` (the UI warns on the last one).
    pub identity_origin: String,
    pub secret_backend: String,
    pub secret_os_protected: bool,
}

/// An open pairing window, rendered by the Hub as a QR code plus a 6-digit code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingWindowInfo {
    pub id: u64,
    /// `devti://pair?...` link (QR payload).
    pub uri: String,
    /// Code for laptop ↔ laptop pairing, formatted `"482 913"`.
    pub code: String,
    pub expires_at: i64,
    pub addresses: Vec<String>,
}

/// Which side of a pairing this device plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingRole {
    Hub,
    Joiner,
}

/// Why a peer disappeared from the trusted list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    /// This user removed it.
    RevokedHere,
    /// The other device removed us, so we drop it too.
    RevokedByPeer,
}

/// Everything that can happen inside a node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeEvent {
    /// This device's own info or state changed.
    LocalChanged {
        local: LocalSnapshot,
    },
    /// A paired device changed (came online, went offline, battery, rename, permissions…).
    PeerChanged {
        peer: PeerSnapshot,
    },
    /// A paired device was removed.
    PeerRemoved {
        id: DeviceId,
        reason: RemovalReason,
    },
    /// A Devti device appeared on the local network.
    DeviceDiscovered {
        device: DiscoveredDevice,
    },
    /// A previously discovered device left the network.
    DeviceLost {
        id: DeviceId,
    },
    /// The pairing window opened (`Some`) or closed (`None`).
    PairingWindowChanged {
        window: Option<PairingWindowInfo>,
    },
    /// A pairing attempt began. On the hub `peer` is the joiner; on the joiner it is
    /// unknown until the hub replies.
    PairingStarted {
        pairing_id: u64,
        role: PairingRole,
        peer: Option<DeviceInfo>,
    },
    /// Keys agreed — both screens now show `sas` and wait for the user.
    PairingVerify {
        pairing_id: u64,
        role: PairingRole,
        peer: DeviceInfo,
        sas: String,
    },
    PairingCompleted {
        pairing_id: u64,
        peer: PeerSnapshot,
    },
    PairingFailed {
        pairing_id: u64,
        reason: PairAbortReason,
        message: String,
    },
    /// Kill switch toggled.
    Paused {
        paused: bool,
    },
    /// Another device shared its clipboard; the host app should put `text` on the OS
    /// clipboard.
    ClipboardReceived {
        from: DeviceId,
        from_name: String,
        text: String,
        ts: i64,
    },
    /// Find my device: ring at full volume (`on`) or stop.
    RingRequested {
        from: DeviceId,
        from_name: String,
        on: bool,
    },
    /// A notification from another device appeared or changed. `dnd` says whether the
    /// ecosystem is in Do Not Disturb (show it silently).
    NotificationPosted {
        notification: MirroredNotification,
        dnd: bool,
    },
    NotificationRemoved {
        from: DeviceId,
        key: String,
    },
    /// Another device asks this one to run an action on one of *its* notifications.
    NotificationActionRequested {
        from: DeviceId,
        key: String,
        action: String,
        reply: Option<String>,
    },
    /// Another device dismissed one of this device's notifications.
    NotificationDismissRequested {
        from: DeviceId,
        key: String,
    },
    /// Ecosystem-wide Do Not Disturb changed (`by` = the device that changed it).
    DndChanged {
        on: bool,
        by: Option<DeviceId>,
    },
    /// A device dropped to the low-battery threshold while not charging.
    LowBattery {
        id: DeviceId,
        name: String,
        percent: u8,
    },
    /// A file transfer progressed or changed state.
    Transfer {
        transfer: TransferRecord,
        rate_bps: u64,
    },
    /// A remote sent input for this device to perform (Phase 3).
    RemoteInput {
        from: DeviceId,
        events: Vec<InputEvent>,
    },
    /// A remote asked for a power, volume or brightness action.
    SystemRequested {
        from: DeviceId,
        from_name: String,
        action: SystemAction,
    },
    /// A remote asked to open an app from this device's catalog.
    LaunchRequested {
        from: DeviceId,
        from_name: String,
        app: String,
    },
    /// A remote asked to run a saved command; answer with `Node::report_run`.
    RunRequested {
        from: DeviceId,
        from_name: String,
        command: String,
    },
    /// A command this device asked another to run has finished.
    RunFinished {
        from: DeviceId,
        command: String,
        ok: bool,
        output: String,
    },
    /// A remote pressed one of this device's deck buttons.
    DeckPressed {
        from: DeviceId,
        from_name: String,
        button: String,
    },
    /// Another device's apps, commands and deck.
    CatalogReceived {
        from: DeviceId,
        catalog: Catalog,
    },
    /// A device started (`on`) or stopped watching this device's screen or camera.
    ScreenRequested {
        from: DeviceId,
        from_name: String,
        on: bool,
        /// "screen" or "camera".
        #[serde(default)]
        source: String,
        /// For the camera: "front" or "back", when the viewer asked for one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facing: Option<String>,
    },
    /// A device offers its camera as a webcam for this one (Phase 6).
    CameraOffered {
        from: DeviceId,
        from_name: String,
    },
    /// About once a second while a screen is shared or viewed.
    ScreenStats {
        peer: DeviceId,
        sharing: bool,
        fps: u8,
        latency_ms: u32,
        kbps: u32,
    },
    /// A screen (or camera) this device was viewing stopped.
    ScreenEnded {
        peer: DeviceId,
        /// "screen" or "camera".
        #[serde(default)]
        source: String,
    },
    // ---- Phase 6: communication
    /// The phone's conversations (answer to `request_sms_threads`).
    SmsThreads {
        from: DeviceId,
        threads: Vec<SmsThread>,
    },
    SmsMessages {
        from: DeviceId,
        thread: String,
        messages: Vec<SmsMessage>,
    },
    /// A text arrived on (or was sent from) a phone.
    Sms {
        from: DeviceId,
        from_name: String,
        message: SmsMessage,
    },
    SmsResult {
        from: DeviceId,
        ok: bool,
        error: Option<String>,
    },
    /// Phone side: a computer asked for conversations / messages, or to send a text.
    SmsThreadsRequested {
        from: DeviceId,
    },
    SmsMessagesRequested {
        from: DeviceId,
        thread: String,
    },
    SmsSendRequested {
        from: DeviceId,
        to: Vec<String>,
        text: String,
    },
    /// A phone's call state changed.
    Call {
        from: DeviceId,
        from_name: String,
        call: CallInfo,
    },
    /// Phone side: a computer asked to answer, decline or end a call.
    CallActionRequested {
        from: DeviceId,
        id: String,
        action: String,
    },
    /// Something to continue here: a link (maybe at a media position) or text.
    Handoff {
        from: DeviceId,
        from_name: String,
        item: HandoffItem,
    },
    /// A device started streaming audio here ("relay" or "mic").
    AudioStarted {
        from: DeviceId,
        from_name: String,
        kind: String,
        sample_rate: u32,
        channels: u8,
    },
    AudioEnded {
        from: DeviceId,
        kind: String,
    },
    // ---- Phase 7
    /// A peer's remote addresses changed (see `Node::peer_reach`).
    ReachChanged {
        peer: DeviceId,
    },
    /// Connecting from anywhere started, stopped or came online (see `Node::anywhere_info`).
    AnywhereChanged,
    /// A listing of another device's shared folder.
    FsListing {
        from: DeviceId,
        path: String,
        entries: Vec<crate::protocol::FsEntry>,
        truncated: bool,
        error: Option<String>,
    },
    /// A download from another device's shared folder couldn't start.
    FsError {
        from: DeviceId,
        path: String,
        error: String,
    },
}
