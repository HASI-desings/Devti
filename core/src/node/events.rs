//! Events and snapshots the node publishes to UI layers (Tauri, UniFFI, CLI).
//!
//! Everything here serialises to JSON with a `type` tag, so the desktop UI and the
//! mobile apps consume exactly the same shapes.

use serde::{Deserialize, Serialize};

use crate::discovery::DiscoveredDevice;
use crate::model::{ConnectionKind, DeviceId, DeviceInfo, DeviceStatus, Permissions, TrustRole};
use crate::protocol::PairAbortReason;

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
}
