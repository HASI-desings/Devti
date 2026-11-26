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
    /// A live screen (Phase 3): the sharing device opens it to the viewer; frames one
    /// way, acknowledgements back.
    Screen,
    /// Live audio (Phase 6): PCM chunks from the sender (`kind`: "relay" = what the
    /// sender is playing, "mic" = its microphone).
    Audio { kind: String, sample_rate: u32, channels: u8 },
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Pointer, keyboard, touch and media input for the receiver to perform (Phase 3).
    /// Needs the `control` permission.
    Input {
        events: Vec<InputEvent>,
    },
    /// Power, volume or brightness on the receiver. Power actions need `commands`,
    /// volume and brightness need `control`.
    System {
        action: SystemAction,
    },
    /// Open an app from the receiver's catalog (`commands`).
    Launch {
        app: String,
    },
    /// Run one of the receiver's saved commands by id (`commands`).
    Run {
        command: String,
    },
    /// Outcome of a `Run` (sent back to whoever asked).
    RunResult {
        command: String,
        ok: bool,
        output: String,
    },
    /// Press a shortcuts-deck button defined on the receiver (`commands`).
    DeckPress {
        button: String,
    },
    /// Asks for the receiver's apps, saved commands and shortcuts deck.
    CatalogRequest,
    Catalog {
        catalog: Catalog,
    },
    /// Asks the receiver to share its screen with the sender (`screen_view`).
    /// `source`: "screen" (default) or "camera" (the phone's camera as a webcam).
    ScreenStart {
        max_width: u32,
        fps: u8,
        #[serde(default = "screen_source")]
        source: String,
        /// For the camera: "front" or "back" (default: the sharer's choice).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facing: Option<String>,
    },
    /// The sender offers its camera to the receiver as a webcam; the receiver answers
    /// with `ScreenStart { source: "camera" }` (Phase 6).
    CameraOffer,
    // ---- Phase 6: communication (needs `notifications` on the phone; sending and
    // answering also need `commands`)
    SmsThreadsRequest,
    SmsThreads {
        threads: Vec<SmsThread>,
    },
    SmsMessagesRequest {
        thread: String,
    },
    SmsMessages {
        thread: String,
        messages: Vec<SmsMessage>,
    },
    SmsSend {
        to: Vec<String>,
        text: String,
    },
    /// A message arrived on (or was sent from) the phone.
    Sms {
        message: SmsMessage,
    },
    SmsResult {
        ok: bool,
        error: Option<String>,
    },
    /// The phone's call state changed.
    Call {
        call: CallInfo,
    },
    /// answer, decline, end, mute, unmute.
    CallAction {
        id: String,
        action: String,
    },
    /// Continue something on the receiver: open a link (at a media position) or show text
    /// (`clipboard` permission).
    Handoff {
        item: HandoffItem,
    },
    // ---- Phase 7: reach and shared folders
    /// Addresses the sender can be reached at away from the local network ("ip:port" or
    /// "host:port": VPN addresses such as Tailscale's, or ones the user typed). Sent after
    /// every handshake; an empty list means remote access is off. `relay` is the relay the
    /// sender's connect-from-anywhere endpoint is reachable through (a dialling hint).
    Reach {
        addrs: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay: Option<String>,
    },
    /// List a folder the receiver shares; "/" lists the shared folders (`browse`).
    FsList {
        path: String,
    },
    FsListing {
        path: String,
        entries: Vec<FsEntry>,
        /// More entries existed than one listing carries.
        #[serde(default)]
        truncated: bool,
        #[serde(default)]
        error: Option<String>,
    },
    /// Send me this shared file; it arrives as an ordinary file transfer (`browse`).
    FsGet {
        path: String,
    },
    /// Why an `FsGet` didn't start.
    FsError {
        path: String,
        error: String,
    },
    // ---- 1.1: find my device
    /// Where are you? (`locate` on the receiver)
    LocateRequest,
    /// The sender's location and addresses: an answer, or sent unasked by stolen mode.
    Location {
        location: crate::node::locate::Location,
    },
    /// Stops sharing `source` with the sender ("screen" or "camera"; none: both).
    ScreenStop {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
    /// The session is ending on purpose.
    Goodbye {
        reason: GoodbyeReason,
    },
    #[serde(other)]
    Unknown,
}

fn screen_source() -> String {
    "screen".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsThread {
    pub id: String,
    /// Phone numbers or addresses in the conversation.
    pub addresses: Vec<String>,
    /// Contact names when the phone knows them.
    #[serde(default)]
    pub names: Vec<String>,
    pub snippet: String,
    pub ts: i64,
    #[serde(default)]
    pub unread: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsMessage {
    pub id: String,
    pub thread: String,
    pub address: String,
    #[serde(default)]
    pub name: String,
    pub body: String,
    pub ts: i64,
    /// Sent from the phone (true) or received (false).
    pub outgoing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallInfo {
    pub id: String,
    /// ringing, active, ended, missed.
    pub state: String,
    pub number: String,
    #[serde(default)]
    pub name: String,
    pub incoming: bool,
    pub ts: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffItem {
    /// url, text, media.
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
    /// Media position to continue from.
    #[serde(default)]
    pub position_ms: Option<u64>,
}

impl HandoffItem {
    /// Only web links and plain text travel; anything else (file:, javascript:, intent:)
    /// is refused so a handoff can't be used to run something on the receiver.
    pub fn is_safe(&self) -> bool {
        let url_ok = self.url.is_empty() || self.url.starts_with("https://") || self.url.starts_with("http://");
        url_ok && self.url.len() <= 4096 && self.title.len() <= 512 && self.text.len() <= 64 * 1024
    }
}

/// One entry of a shared-folder listing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsEntry {
    pub name: String,
    pub dir: bool,
    #[serde(default)]
    pub size: u64,
    /// Unix milliseconds; 0 when unknown.
    #[serde(default)]
    pub modified: i64,
}

/// One chunk on an audio stream: interleaved signed 16-bit little-endian PCM.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioChunk {
    pub seq: u64,
    pub pcm: ByteBuf,
}

/// Audio chunks are small (≤ 100 ms of 48 kHz stereo).
pub const AUDIO_CHUNK_LIMIT: usize = 64 * 1024;

/// Mouse buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// One input action. Coordinates in `MoveTo`/`Tap`/`Swipe` are fractions (0..1) of the
/// receiver's screen, so the sender never needs its resolution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum InputEvent {
    /// Relative pointer motion in device-independent pixels (touchpad).
    Move {
        dx: f32,
        dy: f32,
    },
    /// Absolute pointer position (mirror window).
    MoveTo {
        x: f32,
        y: f32,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    Click {
        button: MouseButton,
        count: u8,
    },
    /// Scroll in lines (positive `dy` = down).
    Scroll {
        dx: f32,
        dy: f32,
    },
    /// Text to type as-is.
    Text {
        text: String,
    },
    /// A named key with modifiers, e.g. `{"key":"tab","mods":["alt"]}`.
    Key {
        key: String,
        mods: Vec<String>,
    },
    /// play_pause, next, previous, stop, volume_up, volume_down, mute.
    Media {
        action: String,
    },
    /// Touch screens (Android): tap, long press and swipe at fractions of the screen.
    Tap {
        x: f32,
        y: f32,
    },
    LongPress {
        x: f32,
        y: f32,
    },
    Swipe {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        ms: u32,
    },
    /// back, home, recents, notifications, quick_settings, lock_screen, screenshot.
    Nav {
        action: String,
    },
    #[serde(other)]
    Unknown,
}

impl InputEvent {
    /// Drops values that make no sense (NaN, huge moves, very long text).
    pub fn sanitized(self) -> Option<Self> {
        fn ok(v: f32, max: f32) -> bool {
            v.is_finite() && v.abs() <= max
        }
        fn frac(v: f32) -> bool {
            v.is_finite() && (-0.01..=1.01).contains(&v)
        }
        let keep = match &self {
            InputEvent::Move { dx, dy } => ok(*dx, 4000.0) && ok(*dy, 4000.0),
            InputEvent::MoveTo { x, y } | InputEvent::Tap { x, y } | InputEvent::LongPress { x, y } => {
                frac(*x) && frac(*y)
            }
            InputEvent::Swipe { x1, y1, x2, y2, ms } => {
                frac(*x1) && frac(*y1) && frac(*x2) && frac(*y2) && *ms <= 10_000
            }
            InputEvent::Scroll { dx, dy } => ok(*dx, 200.0) && ok(*dy, 200.0),
            InputEvent::Click { count, .. } => *count <= 3,
            InputEvent::Text { text } => text.len() <= 4096,
            InputEvent::Key { key, mods } => key.len() <= 32 && mods.len() <= 4,
            InputEvent::Media { action } | InputEvent::Nav { action } => action.len() <= 32,
            InputEvent::Button { .. } => true,
            InputEvent::Unknown => false,
        };
        keep.then_some(self)
    }
}

/// System actions a remote can ask for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "a", rename_all = "snake_case")]
pub enum SystemAction {
    Lock,
    Sleep,
    Shutdown,
    Restart,
    SignOut,
    SetVolume {
        percent: u8,
    },
    SetBrightness {
        percent: u8,
    },
    #[serde(other)]
    Unknown,
}

impl SystemAction {
    /// Power actions need `commands`; volume and brightness only `control`.
    pub fn is_power(&self) -> bool {
        matches!(
            self,
            SystemAction::Lock
                | SystemAction::Sleep
                | SystemAction::Shutdown
                | SystemAction::Restart
                | SystemAction::SignOut
        )
    }
}

/// What a device offers to remotes and decks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub apps: Vec<CatalogApp>,
    #[serde(default)]
    pub commands: Vec<CatalogCommand>,
    #[serde(default)]
    pub deck: Vec<DeckPage>,
    /// Which `SystemAction`s work on this device (e.g. no brightness on a desktop monitor).
    #[serde(default)]
    pub system: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogApp {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCommand {
    pub id: String,
    pub name: String,
    /// Ask "are you sure?" on the remote before running.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckPage {
    pub name: String,
    pub buttons: Vec<DeckButton>,
}

/// One shortcuts-deck button. The action stays on the device that defined it; remotes
/// only see the label, icon and colour and send `DeckPress { button: id }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckButton {
    pub id: String,
    pub label: String,
    /// One of the built-in icon names (the UI falls back to the label's first letter).
    #[serde(default)]
    pub icon: String,
    /// One of the six voices.
    #[serde(default)]
    pub voice: String,
}

impl Catalog {
    /// Caps sizes so a hostile peer can't flood a remote's UI.
    pub fn sanitized(mut self) -> Self {
        fn cap(s: &str, n: usize) -> String {
            s.chars().filter(|c| !c.is_control()).take(n).collect()
        }
        self.apps.truncate(500);
        for a in &mut self.apps {
            a.id = cap(&a.id, 512);
            a.name = cap(&a.name, 80);
        }
        self.commands.truncate(100);
        for c in &mut self.commands {
            c.id = cap(&c.id, 64);
            c.name = cap(&c.name, 80);
        }
        self.deck.truncate(12);
        for p in &mut self.deck {
            p.name = cap(&p.name, 40);
            p.buttons.truncate(48);
            for b in &mut p.buttons {
                b.id = cap(&b.id, 64);
                b.label = cap(&b.label, 40);
                b.icon = cap(&b.icon, 32);
                b.voice = cap(&b.voice, 16);
            }
        }
        self.system.truncate(16);
        self
    }
}

/// Messages on a screen stream. Frames go sharer → viewer, acks viewer → sharer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ScreenMsg {
    /// One JPEG frame. `latency_ms` and `fps` are the sharer's latest measurements, so
    /// the viewer can show them.
    Frame {
        seq: u64,
        w: u32,
        h: u32,
        jpeg: ByteBuf,
        latency_ms: u32,
        fps: u8,
        /// "screen" or "camera" (older sharers send none: "screen").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
    Ack {
        seq: u64,
    },
    #[serde(other)]
    Unknown,
}

/// Screen frames can be large (a 1080p JPEG at high quality).
pub const SCREEN_FRAME_LIMIT: usize = 8 * 1024 * 1024;

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
            ControlMsg::Reach {
                addrs: vec!["100.101.102.103:47810".into(), "home.example.org:47810".into()],
                relay: Some("https://relay.example.org/".into()),
            },
            ControlMsg::FsList { path: "/Documents/2026".into() },
            ControlMsg::FsListing {
                path: "/Documents".into(),
                entries: vec![FsEntry { name: "plan.pdf".into(), dir: false, size: 1234, modified: 1_700_000_000_000 }],
                truncated: false,
                error: None,
            },
            ControlMsg::FsGet { path: "/Documents/plan.pdf".into() },
            ControlMsg::FsError { path: "/x".into(), error: "gone".into() },
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
