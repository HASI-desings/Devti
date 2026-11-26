//! Phase 3/4 live screens: one device shares its screen as JPEG frames, another views.
//!
//! ```text
//! viewer                                   sharer
//!   |-- ScreenStart {max_width, fps} ------->|  needs `screen_view` on the sharer
//!   |                                        |  host starts capturing, pushes frames
//!   |<========= StreamOpen::Screen ==========|  (opened by the sharer)
//!   |<-- Frame {seq, jpeg, latency, fps} ----|
//!   |--- Ack {seq} ------------------------->|  at most 2 frames in flight
//!   |-- ScreenStop ------------------------->|
//! ```
//!
//! Adaptive quality: the sharer only ever sends the *latest* frame, and never more than
//! two unacknowledged ones, so a slow network lowers the frame rate instead of building
//! a queue. It also measures capture-to-ack latency and adjusts the JPEG quality and
//! width it asks the host's encoder for ([`ScreenTarget`]). Over the internet or a relay
//! it starts lower.
//!
//! A device can share two **sources** at once: its screen and its camera (a phone used
//! as a webcam while it's also mirrored). Each `(viewer, source)` pair is one share with
//! its own stream, and frames say which source they came from. Asking again for a
//! source already being shared only updates its settings; it never restarts or ends it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::link::{Link, LinkRecv as RecvStream, LinkSend as SendStream};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

use super::{Inner, NodeEvent};
use crate::error::{Error, Result};
use crate::model::DeviceId;
use crate::protocol::{ControlMsg, SCREEN_FRAME_LIMIT, ScreenMsg, StreamOpen, read_message, write_message};

const MAX_IN_FLIGHT: u64 = 2;

/// What the host's encoder should produce right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenTarget {
    pub max_width: u32,
    pub fps: u8,
    /// JPEG quality, 30–90.
    pub quality: u8,
}

impl Default for ScreenTarget {
    fn default() -> Self {
        Self { max_width: 1600, fps: 30, quality: 70 }
    }
}

/// An encoded frame pushed by the sharing host.
#[derive(Debug)]
pub struct EncodedFrame {
    pub w: u32,
    pub h: u32,
    pub jpeg: Vec<u8>,
    pub captured: Instant,
}

/// What can be shared.
pub const SOURCE_SCREEN: &str = "screen";
pub const SOURCE_CAMERA: &str = "camera";

fn normalize(source: &str) -> &'static str {
    if source == SOURCE_CAMERA { SOURCE_CAMERA } else { SOURCE_SCREEN }
}

/// A frame received from a sharing device.
#[derive(Clone, Debug)]
pub struct ViewerFrame {
    pub from: DeviceId,
    /// "screen" or "camera".
    pub source: &'static str,
    pub seq: u64,
    pub w: u32,
    pub h: u32,
    pub jpeg: Arc<Vec<u8>>,
    pub latency_ms: u32,
    pub fps: u8,
}

/// One running share: a generation number (so a finished task only ever removes its own
/// entry) and how to stop it.
struct Share {
    generation: u64,
    stop: watch::Sender<bool>,
}

pub(crate) struct State {
    /// Latest frame from the host, per source (sharer side).
    latest_screen: watch::Sender<Option<Arc<EncodedFrame>>>,
    latest_camera: watch::Sender<Option<Arc<EncodedFrame>>>,
    /// Shares running for viewers: (viewer, source).
    sharing: Mutex<HashMap<(DeviceId, &'static str), Share>>,
    generation: std::sync::atomic::AtomicU64,
    /// What this device asked others to share with it (viewer side).
    viewing: Mutex<HashSet<(DeviceId, &'static str)>>,
    target: RwLock<ScreenTarget>,
    frames: broadcast::Sender<ViewerFrame>,
}

impl State {
    pub fn new() -> Self {
        Self {
            latest_screen: watch::channel(None).0,
            latest_camera: watch::channel(None).0,
            sharing: Mutex::new(HashMap::new()),
            generation: std::sync::atomic::AtomicU64::new(1),
            viewing: Mutex::new(HashSet::new()),
            target: RwLock::new(ScreenTarget::default()),
            frames: broadcast::channel(4).0,
        }
    }

    fn latest(&self, source: &str) -> &watch::Sender<Option<Arc<EncodedFrame>>> {
        if source == SOURCE_CAMERA { &self.latest_camera } else { &self.latest_screen }
    }
}

/// `ScreenStart` / `ScreenStop` from a peer.
pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, msg: ControlMsg) {
    let Some(perms) = inner.permissions_of(&peer) else { return };
    let name = inner.peer_name(&peer);
    match msg {
        ControlMsg::ScreenStart { max_width, fps, source, facing } => {
            if !perms.screen_view {
                return;
            }
            let source = normalize(&source);
            {
                let mut t = inner.screen.target.write();
                t.max_width = max_width.clamp(320, 3840);
                t.fps = fps.clamp(1, 60);
            }
            if inner.screen.sharing.lock().contains_key(&(peer, source)) {
                // Already sharing this: the viewer only changed its settings.
                return;
            }
            let conn = inner.peers.read().get(&peer).and_then(|p| p.session.as_ref().map(|s| s.conn.clone()));
            let Some(conn) = conn else { return };
            // Nobody is watching this source yet: forget its last picture, so nothing from
            // an earlier share reaches the new viewer before the host sends a fresh one.
            let others = inner.screen.sharing.lock().keys().any(|(_, s)| *s == source);
            if !others {
                inner.screen.latest(source).send_replace(None);
            }
            let generation = inner.screen.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (stop_tx, stop_rx) = watch::channel(false);
            inner.screen.sharing.lock().insert((peer, source), Share { generation, stop: stop_tx });
            inner.log(
                Some((&peer, &name)),
                if source == SOURCE_CAMERA { "camera_shared" } else { "screen_shared" },
                facing.as_deref().unwrap_or(""),
            );
            inner.emit(NodeEvent::ScreenRequested {
                from: peer,
                from_name: name,
                on: true,
                source: source.into(),
                facing,
            });
            let task_inner = inner.clone();
            inner.rt.spawn(async move {
                if let Err(e) = share(&task_inner, peer, source, conn, stop_rx).await {
                    tracing::debug!(peer = %peer, source, error = %e, "share ended");
                }
                finish_share(&task_inner, &peer, source, Some(generation));
            });
        }
        ControlMsg::ScreenStop { source } => match source {
            Some(s) => finish_share(inner, &peer, normalize(&s), None),
            None => {
                finish_share(inner, &peer, SOURCE_SCREEN, None);
                finish_share(inner, &peer, SOURCE_CAMERA, None);
            }
        },
        ControlMsg::CameraOffer => inner.emit(NodeEvent::CameraOffered { from: peer, from_name: name }),
        _ => {}
    }
}

/// Ends a share. With `generation`, only if it's still that share (a task finishing late
/// must not end a newer share of the same source).
fn finish_share(inner: &Arc<Inner>, peer: &DeviceId, source: &'static str, generation: Option<u64>) {
    let removed = {
        let mut sharing = inner.screen.sharing.lock();
        match sharing.get(&(*peer, source)) {
            Some(s) if generation.is_none_or(|g| g == s.generation) => sharing.remove(&(*peer, source)),
            _ => None,
        }
    };
    if let Some(share) = removed {
        let _ = share.stop.send(true);
        let name = inner.peer_name(peer);
        inner.emit(NodeEvent::ScreenRequested {
            from: *peer,
            from_name: name,
            on: false,
            source: source.into(),
            facing: None,
        });
    }
}

/// Session went down: stop both directions with that peer.
pub(crate) fn on_session_down(inner: &Arc<Inner>, peer: DeviceId) {
    finish_share(inner, &peer, SOURCE_SCREEN, None);
    finish_share(inner, &peer, SOURCE_CAMERA, None);
    for source in [SOURCE_SCREEN, SOURCE_CAMERA] {
        if inner.screen.viewing.lock().remove(&(peer, source)) {
            inner.emit(NodeEvent::ScreenEnded { peer, source: source.into() });
        }
    }
}

async fn share(
    inner: &Arc<Inner>,
    peer: DeviceId,
    source: &'static str,
    conn: Link,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    // Over the internet or a relay, start lighter; the ladder in `adapt` climbs from there.
    if matches!(conn.path().kind, crate::model::ConnectionKind::Relay | crate::model::ConnectionKind::Internet) {
        let mut t = inner.screen.target.write();
        t.max_width = t.max_width.min(1280);
        t.fps = t.fps.min(15);
        t.quality = t.quality.min(55);
    }
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(&mut send, &StreamOpen::Screen).await?;
    let mut frames = inner.screen.latest(source).subscribe();
    frames.mark_changed();
    let (ack_tx, mut acks) = tokio::sync::mpsc::unbounded_channel::<u64>();
    let reader = inner.rt.spawn(async move {
        while let Ok(Some(ScreenMsg::Ack { seq })) = read_message::<_, ScreenMsg>(&mut recv, 1024).await {
            if ack_tx.send(seq).is_err() {
                break;
            }
        }
    });

    let mut seq = 0u64;
    let mut acked = 0u64;
    let mut sent_at: VecDeque<(u64, Instant)> = VecDeque::new();
    let mut latency_ms = 0u32;
    let mut window = (Instant::now(), 0u32, 0u64); // (start, frames, bytes)
    let mut fps = 0u8;
    let result = loop {
        // Wait for room in flight, a new frame, or stop.
        tokio::select! {
            _ = stop.changed() => break Ok(()),
            Some(a) = acks.recv() => {
                acked = acked.max(a);
                while let Some(&(s, t)) = sent_at.front() {
                    if s > acked { break; }
                    if s == acked {
                        latency_ms = t.elapsed().as_millis().min(10_000) as u32;
                    }
                    sent_at.pop_front();
                }
                adapt(inner, latency_ms, fps);
                continue;
            }
            changed = frames.changed(), if seq - acked < MAX_IN_FLIGHT => {
                if changed.is_err() { break Ok(()); }
            }
        }
        let Some(frame) = frames.borrow_and_update().clone() else { continue };
        seq += 1;
        let captured_ms = frame.captured.elapsed().as_millis().min(10_000) as u32;
        let msg = ScreenMsg::Frame {
            seq,
            w: frame.w,
            h: frame.h,
            jpeg: serde_bytes::ByteBuf::from(frame.jpeg.clone()),
            latency_ms: latency_ms.max(captured_ms),
            fps,
            source: (source == SOURCE_CAMERA).then(|| SOURCE_CAMERA.to_string()),
        };
        sent_at.push_back((seq, frame.captured));
        if let Err(e) = write_message(&mut send, &msg).await {
            break Err(e);
        }
        window.1 += 1;
        window.2 += frame.jpeg.len() as u64;
        let elapsed = window.0.elapsed();
        if elapsed >= Duration::from_secs(1) {
            fps = (window.1 as f64 / elapsed.as_secs_f64()).round().min(120.0) as u8;
            let kbps = (window.2 * 8 / 1000) as u32 * 1000 / elapsed.as_millis().max(1) as u32;
            inner.emit(NodeEvent::ScreenStats { peer, sharing: true, fps, latency_ms, kbps });
            window = (Instant::now(), 0, 0);
        }
    };
    reader.abort();
    let _ = send.finish();
    result
}

/// Simple quality ladder: lower JPEG quality (then width) when latency grows, raise it
/// again when there is headroom.
fn adapt(inner: &Arc<Inner>, latency_ms: u32, fps: u8) {
    let mut t = inner.screen.target.write();
    if latency_ms > 120 {
        if t.quality > 40 {
            t.quality -= 5;
        } else if t.max_width > 960 {
            t.max_width = (t.max_width * 4 / 5).max(960);
        }
    } else if latency_ms < 50 && fps + 2 >= t.fps && t.quality < 85 {
        t.quality += 1;
    }
}

/// A sharer opened a screen stream to us (viewer side).
pub(crate) async fn receive(inner: Arc<Inner>, peer: DeviceId, mut send: SendStream, mut recv: RecvStream) {
    if !inner.screen.viewing.lock().iter().any(|(p, _)| *p == peer) {
        let _ = send.reset(0);
        return;
    }
    let mut window = (Instant::now(), 0u32, 0u64);
    // Which source this stream carries, learned from its first frame.
    let mut stream_source: Option<&'static str> = None;
    loop {
        match read_message::<_, ScreenMsg>(&mut recv, SCREEN_FRAME_LIMIT).await {
            Ok(Some(ScreenMsg::Frame { seq, w, h, jpeg, latency_ms, fps, source })) => {
                let source = normalize(source.as_deref().unwrap_or(SOURCE_SCREEN));
                if stream_source.is_none() {
                    if !inner.screen.viewing.lock().contains(&(peer, source)) {
                        let _ = send.reset(0);
                        return;
                    }
                    stream_source = Some(source);
                }
                let _ = write_message(&mut send, &ScreenMsg::Ack { seq }).await;
                window.1 += 1;
                window.2 += jpeg.len() as u64;
                let _ = inner.screen.frames.send(ViewerFrame {
                    from: peer,
                    source,
                    seq,
                    w: w.min(8192),
                    h: h.min(8192),
                    jpeg: Arc::new(jpeg.into_vec()),
                    latency_ms,
                    fps,
                });
                let elapsed = window.0.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    let got = (window.1 as f64 / elapsed.as_secs_f64()).round().min(120.0) as u8;
                    let kbps = (window.2 * 8 / 1000) as u32 * 1000 / elapsed.as_millis().max(1) as u32;
                    inner.emit(NodeEvent::ScreenStats { peer, sharing: false, fps: got, latency_ms, kbps });
                    window = (Instant::now(), 0, 0);
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    let source = stream_source.unwrap_or(SOURCE_SCREEN);
    if inner.screen.viewing.lock().remove(&(peer, source)) {
        inner.emit(NodeEvent::ScreenEnded { peer, source: source.into() });
    }
}

impl super::Node {
    /// Asks `peer` to share its screen with this device (needs `screen_view` there).
    pub fn screen_start(&self, peer: &DeviceId, max_width: u32, fps: u8) -> Result<()> {
        self.view(peer, max_width, fps, SOURCE_SCREEN, None)
    }

    /// Asks for `peer`'s camera (as a webcam, or to watch it). `facing`: "front", "back"
    /// or `None` for the phone's choice.
    pub fn camera_start(&self, peer: &DeviceId, max_width: u32, fps: u8) -> Result<()> {
        self.view(peer, max_width, fps, SOURCE_CAMERA, None)
    }

    /// Like `camera_start`, choosing the front or back camera.
    pub fn camera_start_facing(&self, peer: &DeviceId, max_width: u32, fps: u8, facing: Option<&str>) -> Result<()> {
        let facing = facing.filter(|f| *f == "front" || *f == "back").map(str::to_string);
        self.view(peer, max_width, fps, SOURCE_CAMERA, facing)
    }

    fn view(
        &self,
        peer: &DeviceId,
        max_width: u32,
        fps: u8,
        source: &'static str,
        facing: Option<String>,
    ) -> Result<()> {
        self.inner.screen.viewing.lock().insert((*peer, source));
        self.inner
            .send_to(peer, ControlMsg::ScreenStart { max_width, fps, source: source.to_string(), facing })
            .inspect_err(|_| {
                self.inner.screen.viewing.lock().remove(&(*peer, source));
            })
    }

    /// Offers this device's camera to `peer` as a webcam.
    pub fn camera_offer(&self, peer: &DeviceId) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::CameraOffer)
    }

    /// What each current viewer is watching ("screen" or "camera"), one entry per share.
    pub fn screen_sources(&self) -> Vec<(DeviceId, String)> {
        self.inner.screen.sharing.lock().keys().map(|(k, v)| (*k, v.to_string())).collect()
    }

    /// Stops viewing `peer`'s screen.
    pub fn screen_stop(&self, peer: &DeviceId) -> Result<()> {
        self.view_stop(peer, SOURCE_SCREEN)
    }

    /// Stops viewing `peer`'s camera.
    pub fn camera_stop(&self, peer: &DeviceId) -> Result<()> {
        self.view_stop(peer, SOURCE_CAMERA)
    }

    fn view_stop(&self, peer: &DeviceId, source: &'static str) -> Result<()> {
        self.inner.screen.viewing.lock().remove(&(*peer, source));
        self.inner.send_to(peer, ControlMsg::ScreenStop { source: Some(source.to_string()) })
    }

    /// Stops sharing this device's screen and camera with `peer` (the sharer's own stop button).
    pub fn screen_stop_sharing(&self, peer: &DeviceId) {
        finish_share(&self.inner, peer, SOURCE_SCREEN, None);
        finish_share(&self.inner, peer, SOURCE_CAMERA, None);
    }

    /// Stops sharing one source with `peer` (e.g. the camera failed to start).
    pub fn share_stop(&self, peer: &DeviceId, source: &str) {
        finish_share(&self.inner, peer, normalize(source), None);
    }

    /// Whether this device asked to view `peer`'s screen and hasn't stopped.
    pub fn is_viewing(&self, peer: &DeviceId) -> bool {
        self.inner.screen.viewing.lock().contains(&(*peer, SOURCE_SCREEN))
    }

    /// Whether this device asked for `peer`'s camera and hasn't stopped.
    pub fn is_viewing_camera(&self, peer: &DeviceId) -> bool {
        self.inner.screen.viewing.lock().contains(&(*peer, SOURCE_CAMERA))
    }

    /// Frames from devices this one is viewing.
    pub fn subscribe_frames(&self) -> broadcast::Receiver<ViewerFrame> {
        self.inner.screen.frames.subscribe()
    }

    /// Sharer side: the host hands over its newest encoded screen frame.
    pub fn push_screen_frame(&self, w: u32, h: u32, jpeg: Vec<u8>) -> Result<()> {
        self.push_frame(SOURCE_SCREEN, w, h, jpeg, Instant::now())
    }

    /// Like `push_screen_frame`, with the moment the picture was captured, so the latency
    /// viewers see includes capture and encoding (capture → shown → acknowledged).
    pub fn push_screen_frame_captured(&self, w: u32, h: u32, jpeg: Vec<u8>, captured: Instant) -> Result<()> {
        self.push_frame(SOURCE_SCREEN, w, h, jpeg, captured)
    }

    /// Sharer side: the newest camera frame.
    pub fn push_camera_frame(&self, w: u32, h: u32, jpeg: Vec<u8>) -> Result<()> {
        self.push_frame(SOURCE_CAMERA, w, h, jpeg, Instant::now())
    }

    fn push_frame(&self, source: &str, w: u32, h: u32, jpeg: Vec<u8>, captured: Instant) -> Result<()> {
        if jpeg.len() > SCREEN_FRAME_LIMIT - 1024 {
            return Err(Error::InvalidInput("frame too large".into()));
        }
        self.inner.screen.latest(source).send_replace(Some(Arc::new(EncodedFrame { w, h, jpeg, captured })));
        Ok(())
    }

    /// How many devices are watching this screen right now.
    pub fn screen_viewers(&self) -> Vec<DeviceId> {
        let mut v: Vec<DeviceId> =
            self.inner.screen.sharing.lock().keys().filter(|(_, s)| *s == SOURCE_SCREEN).map(|(d, _)| *d).collect();
        v.sort();
        v
    }

    /// Devices watching this device's camera right now.
    pub fn camera_viewers(&self) -> Vec<DeviceId> {
        let mut v: Vec<DeviceId> =
            self.inner.screen.sharing.lock().keys().filter(|(_, s)| *s == SOURCE_CAMERA).map(|(d, _)| *d).collect();
        v.sort();
        v
    }

    /// What the host's encoder should produce (adapted to the network).
    pub fn screen_target(&self) -> ScreenTarget {
        *self.inner.screen.target.read()
    }
}
