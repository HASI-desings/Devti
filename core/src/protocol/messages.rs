//! Message schemas for every stream type.
//!
//! All enums are internally tagged (`"t": "..."`) and end with an `Unknown` catch-all,
//! so a peer running a newer protocol revision can add message types without breaking
//! older peers: unknown messages are logged and skipped.

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use crate::model::{DeviceInfo, DeviceStatus};

/// Wire protocol revision. Bumped only for incompatible changes.
pub const PROTOCOL_VERSION: u16 = 1;
/// QUIC ALPN identifier.
pub const ALPN: &[u8] = b"devti/1";
/// Frame size limit on pairing streams (pre-authentication, so kept tiny).
pub const PAIR_FRAME_LIMIT: usize = 16 * 1024;
/// Frame size limit on the control stream.
pub const CONTROL_FRAME_LIMIT: usize = 1024 * 1024;

/// First frame of every bi-directional stream: declares the stream's purpose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stream", rename_all = "snake_case")]
pub enum StreamOpen {
    /// Pairing handshake (allowed from untrusted devices while a pairing window is open).
    Pair,
    /// The long-lived control channel of an authenticated session.
    Control,
    /// One file transfer (Phase 2): offer → answer → raw bytes → done.
    File,
    #[serde(other)]
    Unknown,
}

/// How the joining device obtained the pairing secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairMethod {
    /// 128-bit secret scanned from the QR code.
    Qr,
    /// 6-digit code typed by the user.
    Code,
}

/// Why a pairing attempt was aborted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairAbortReason {
    /// Key confirmation failed: wrong code/QR, or someone is in the middle.
    BadSecret,
    /// The user rejected the verification code.
    Rejected,
    /// No pairing window is open on the other side, or it expired.
    NotAccepting,
    /// Another pairing is already in progress.
    Busy,
    /// Protocol versions are incompatible.
    Incompatible,
    /// The claimed identity doesn't match the TLS certificate.
    IdentityMismatch,
    /// The user didn't decide in time.
    Timeout,
    #[serde(other)]
    Other,
}

impl PairAbortReason {
    /// Text shown in the UI when pairing fails for this reason.
    pub fn describe(&self) -> &'static str {
        match self {
            PairAbortReason::BadSecret => "The code didn't match. Check it and try again, or scan the QR code instead.",
            PairAbortReason::Rejected => "Pairing was declined on one of the devices.",
            PairAbortReason::NotAccepting => "The other device isn't accepting new devices right now.",
            PairAbortReason::Busy => "The other device is already pairing with someone else.",
            PairAbortReason::Incompatible => "The two devices run incompatible versions of Devti.",
            PairAbortReason::IdentityMismatch => "The other device's identity didn't check out.",
            PairAbortReason::Timeout => "Pairing timed out before both devices confirmed.",
            PairAbortReason::Other => "Pairing stopped unexpectedly.",
        }
    }
}

/// Messages on a pairing stream. See `docs/PROTOCOL.md` § Pairing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum PairMsg {
    /// Joiner → Hub: SPAKE2 message B plus the joiner's public info.
    Start { proto: u16, method: PairMethod, spake: ByteBuf, info: DeviceInfo },
    /// Hub → Joiner: SPAKE2 message A plus the hub's public info.
    Reply { spake: ByteBuf, info: DeviceInfo },
    /// Key-confirmation MAC (joiner sends first, then the hub).
    Confirm { mac: ByteBuf },
    /// The user's answer to "do the codes match?".
    Decision { accept: bool },
    /// Either side gives up.
    Abort { reason: PairAbortReason },
    /// Hub → Joiner: both accepted and the hub has stored the joiner as trusted. The
    /// joiner may now open the control stream.
    Done,
    #[serde(other)]
    Unknown,
}

/// Why a session is ending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoodbyeReason {
    /// The app is quitting or the device is going to sleep.
    Shutdown,
    /// The sender removed (revoked) the receiver from its trusted devices.
    Revoked,
    /// The sender paused the ecosystem (kill switch).
    Paused,
    #[serde(other)]
    Other,
}

/// Messages on the control stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ControlMsg {
    /// First message from each side after the stream opens.
    Hello {
        proto: u16,
        info: DeviceInfo,
        status: DeviceStatus,
    },
    /// Live status changed (battery, …).
    Status {
        status: DeviceStatus,
    },
    /// Public info changed (rename, new capability).
    Info {
        info: DeviceInfo,
    },
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    /// Clipboard text copied on the sender (capped at [`CLIPBOARD_MAX_BYTES`]).
    Clipboard {
        text: String,
        ts: i64,
    },
    /// Find my device: start (`on: true`) or stop ringing at full volume.
    Ring {
        on: bool,
    },
    /// A notification appeared (or changed) on the sender.
    Notification {
        notif: NotificationInfo,
    },
    /// A notification disappeared on the sender.
    NotificationRemoved {
        key: String,
    },
    /// Asks the owner of a notification to run one of its actions (with reply text when
    /// the action takes one).
    NotificationAction {
        key: String,
        action: String,
        reply: Option<String>,
    },
    /// Asks the owner of a notification to dismiss it.
    NotificationDismiss {
        key: String,
    },
    /// Ecosystem-wide Do Not Disturb changed on the sender.
    Dnd {
        on: bool,
    },
    /// The session is ending on purpose.
    Goodbye {
        reason: GoodbyeReason,
    },
    #[serde(other)]
    Unknown,
}

/// Largest clipboard text sent between devices.
pub const CLIPBOARD_MAX_BYTES: usize = 256 * 1024;
/// Largest notification icon (PNG) carried in a notification.
pub const NOTIFICATION_ICON_MAX_BYTES: usize = 64 * 1024;

/// One action button on a mirrored notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifAction {
    pub id: String,
    pub label: String,
    /// The action takes typed text (quick reply).
    #[serde(default)]
    pub reply: bool,
}

/// A notification mirrored from another device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationInfo {
    /// Stable key on the owning device.
    pub key: String,
    pub app_id: String,
    pub app_name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
    pub ts: i64,
    #[serde(default)]
    pub actions: Vec<NotifAction>,
    /// App icon as PNG, if the device could provide one.
    #[serde(default)]
    pub icon_png: Option<serde_bytes::ByteBuf>,
    /// Can be dismissed from other devices.
    #[serde(default = "yes")]
    pub dismissable: bool,
}

fn yes() -> bool {
    true
}

impl NotificationInfo {
    /// Caps every peer-controlled field so a hostile device can't flood the UI.
    pub fn sanitized(mut self) -> Self {
        fn cap(s: &str, n: usize) -> String {
            s.chars().filter(|c| !c.is_control() || *c == '\n').take(n).collect()
        }
        self.key = cap(&self.key, 256);
        self.app_id = cap(&self.app_id, 128);
        self.app_name = cap(&self.app_name, 64);
        self.title = cap(&self.title, 256);
        self.text = cap(&self.text, 4096);
        self.actions.truncate(5);
        for a in &mut self.actions {
            a.id = cap(&a.id, 64);
            a.label = cap(&a.label, 48);
        }
        if self.icon_png.as_ref().is_some_and(|i| i.len() > NOTIFICATION_ICON_MAX_BYTES) {
            self.icon_png = None;
        }
        self
    }
}

/// Messages on a `file` stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FileMsg {
    /// Sender → receiver. `id` stays the same across retries so the receiver can resume.
    Offer { id: u64, name: String, size: u64, mime: Option<String> },
    /// Receiver → sender: start at `offset` (bytes already received), or decline.
    Answer { accept: bool, offset: u64, reason: Option<String> },
    /// Sender → receiver after the raw bytes: SHA-256 of the whole file.
    Done { sha256: serde_bytes::ByteBuf },
    /// Receiver → sender: the file was verified and saved.
    Saved,
    #[serde(other)]
    Unknown,
}

/// QUIC application close codes.
pub mod close_code {
    pub const NORMAL: u32 = 0;
    pub const UNTRUSTED: u32 = 1;
    pub const DUPLICATE: u32 = 2;
    pub const REVOKED: u32 = 3;
    pub const PAUSED: u32 = 4;
    pub const SHUTDOWN: u32 = 5;
    pub const PROTOCOL: u32 = 6;
    pub const PAIRING_FINISHED: u32 = 7;
    pub const PAIRING_FAILED: u32 = 8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BatteryState, DeviceId, DeviceKind, Platform};
    use crate::protocol::frame::{decode, encode};

    fn info() -> DeviceInfo {
        DeviceInfo {
            id: DeviceId::from_bytes([3; 10]),
            name: "Desk".into(),
            kind: DeviceKind::Desktop,
            platform: Platform::Linux,
            os_version: "Linux".into(),
            model: None,
            app_version: "0.1.0".into(),
            capabilities: vec![],
        }
    }

    #[test]
    fn every_control_message_roundtrips() {
        let msgs = vec![
            ControlMsg::Hello {
                proto: PROTOCOL_VERSION,
                info: info(),
                status: DeviceStatus { battery: Some(BatteryState::new(50, true)) },
            },
            ControlMsg::Status { status: DeviceStatus::default() },
            ControlMsg::Info { info: info() },
            ControlMsg::Ping { nonce: 1 },
            ControlMsg::Pong { nonce: 1 },
            ControlMsg::Goodbye { reason: GoodbyeReason::Revoked },
        ];
        for m in msgs {
            let f = encode(&m).unwrap();
            let back: ControlMsg = decode(&f[4..]).unwrap();
            assert_eq!(back, m);
        }
    }

    #[test]
    fn every_pair_message_roundtrips() {
        let msgs = vec![
            PairMsg::Start { proto: 1, method: PairMethod::Code, spake: ByteBuf::from(vec![1, 2, 3]), info: info() },
            PairMsg::Reply { spake: ByteBuf::from(vec![4]), info: info() },
            PairMsg::Confirm { mac: ByteBuf::from(vec![0; 32]) },
            PairMsg::Decision { accept: true },
            PairMsg::Abort { reason: PairAbortReason::Busy },
            PairMsg::Done,
        ];
        for m in msgs {
            let f = encode(&m).unwrap();
            let back: PairMsg = decode(&f[4..]).unwrap();
            assert_eq!(back, m);
        }
        for s in [StreamOpen::Pair, StreamOpen::Control] {
            let f = encode(&s).unwrap();
            assert_eq!(decode::<StreamOpen>(&f[4..]).unwrap(), s);
        }
    }

    #[test]
    fn unknown_messages_from_newer_peers_are_tolerated() {
        #[derive(Serialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum Future {
            HoloCall { room: String, depth: u32 },
        }
        let f = encode(&Future::HoloCall { room: "x".into(), depth: 3 }).unwrap();
        assert_eq!(decode::<ControlMsg>(&f[4..]).unwrap(), ControlMsg::Unknown);
        assert_eq!(decode::<PairMsg>(&f[4..]).unwrap(), PairMsg::Unknown);

        #[derive(Serialize)]
        #[serde(tag = "stream", rename_all = "snake_case")]
        enum FutureStream {
            Hologram,
        }
        let f = encode(&FutureStream::Hologram).unwrap();
        assert_eq!(decode::<StreamOpen>(&f[4..]).unwrap(), StreamOpen::Unknown);
    }

    #[test]
    fn unknown_enum_values_inside_messages_are_tolerated() {
        let reason: GoodbyeReason = serde_json::from_str("\"meteor\"").unwrap();
        assert_eq!(reason, GoodbyeReason::Other);
        let abort: PairAbortReason = serde_json::from_str("\"meteor\"").unwrap();
        assert_eq!(abort, PairAbortReason::Other);
    }

    #[test]
    fn abort_reasons_have_descriptions() {
        for r in [
            PairAbortReason::BadSecret,
            PairAbortReason::Rejected,
            PairAbortReason::NotAccepting,
            PairAbortReason::Busy,
            PairAbortReason::Incompatible,
            PairAbortReason::IdentityMismatch,
            PairAbortReason::Timeout,
            PairAbortReason::Other,
        ] {
            assert!(r.describe().ends_with('.'));
        }
    }
}
