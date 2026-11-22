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
//! width it asks the host's encoder for ([`ScreenTarget`]).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use quinn::{RecvStream, SendStream, VarInt};
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

/// A frame received from a sharing device.
#[derive(Clone, Debug)]
pub struct ViewerFrame {
    pub from: DeviceId,
    pub seq: u64,
    pub w: u32,
    pub h: u32,
    pub jpeg: Arc<Vec<u8>>,
    pub latency_ms: u32,
    pub fps: u8,
}

pub(crate) struct State {
    /// Latest frame from the host (sharer side).
    latest: watch::Sender<Option<Arc<EncodedFrame>>>,
    /// Viewers this device is sharing with, and how to stop each.
    sharing: Mutex<HashMap<DeviceId, watch::Sender<bool>>>,
    /// Devices this device asked to share with it (viewer side).
    viewing: Mutex<HashSet<DeviceId>>,
    /// What each viewer asked for: "screen" or "camera".
    sources: Mutex<HashMap<DeviceId, String>>,
    target: RwLock<ScreenTarget>,
    frames: broadcast::Sender<ViewerFrame>,
}

impl State {
    pub fn new() -> Self {
        Self {
            latest: watch::channel(None).0,
            sharing: Mutex::new(HashMap::new()),
            viewing: Mutex::new(HashSet::new()),
            sources: Mutex::new(HashMap::new()),
            target: RwLock::new(ScreenTarget::default()),
            frames: broadcast::channel(4).0,
        }
    }
}

/// `ScreenStart` / `ScreenStop` from a peer.
pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, msg: ControlMsg) {
    let Some(perms) = inner.permissions_of(&peer) else { return };
    let name = inner.peer_name(&peer);
    match msg {
        ControlMsg::ScreenStart { max_width, fps, source } => {
            if !perms.screen_view {
                return;
            }
            let source = if source == "camera" { "camera".to_string() } else { "screen".to_string() };
            {
                let mut t = inner.screen.target.write();
                t.max_width = max_width.clamp(320, 3840);
                t.fps = fps.clamp(1, 60);
            }
            let conn = inner.peers.read().get(&peer).and_then(|p| p.session.as_ref().map(|s| s.conn.clone()));
            let Some(conn) = conn else { return };
            let (stop_tx, stop_rx) = watch::channel(false);
            if let Some(old) = inner.screen.sharing.lock().insert(peer, stop_tx) {
                let _ = old.send(true);
            }
            inner.log(Some((&peer, &name)), if source == "camera" { "camera_shared" } else { "screen_shared" }, "");
            inner.screen.sources.lock().insert(peer, source.clone());
            inner.emit(NodeEvent::ScreenRequested { from: peer, from_name: name, on: true, source });
            let task_inner = inner.clone();
            inner.rt.spawn(async move {
                if let Err(e) = share(&task_inner, peer, conn, stop_rx).await {
                    tracing::debug!(peer = %peer, error = %e, "screen share ended");
                }
                stop_sharing(&task_inner, &peer);
            });
        }
        ControlMsg::ScreenStop => stop_sharing(inner, &peer),
        ControlMsg::CameraOffer => inner.emit(NodeEvent::CameraOffered { from: peer, from_name: name }),
        _ => {}
    }
}

fn stop_sharing(inner: &Arc<Inner>, peer: &DeviceId) {
    if let Some(stop) = inner.screen.sharing.lock().remove(peer) {
        let _ = stop.send(true);
        let name = inner.peer_name(peer);
        let source = inner.screen.sources.lock().remove(peer).unwrap_or_else(|| "screen".into());
        inner.emit(NodeEvent::ScreenRequested { from: *peer, from_name: name, on: false, source });
    }
}

/// Session went down: stop both directions with that peer.
pub(crate) fn on_session_down(inner: &Arc<Inner>, peer: DeviceId) {
    stop_sharing(inner, &peer);
    if inner.screen.viewing.lock().remove(&peer) {
        inner.emit(NodeEvent::ScreenEnded { peer });
    }
}

async fn share(
    inner: &Arc<Inner>,
    peer: DeviceId,
    conn: quinn::Connection,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(&mut send, &StreamOpen::Screen).await?;
    let mut frames = inner.screen.latest.subscribe();
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
    if !inner.screen.viewing.lock().contains(&peer) {
        let _ = send.reset(VarInt::from_u32(0));
        return;
    }
    let mut window = (Instant::now(), 0u32, 0u64);
    loop {
        match read_message::<_, ScreenMsg>(&mut recv, SCREEN_FRAME_LIMIT).await {
            Ok(Some(ScreenMsg::Frame { seq, w, h, jpeg, latency_ms, fps })) => {
                let _ = write_message(&mut send, &ScreenMsg::Ack { seq }).await;
                window.1 += 1;
                window.2 += jpeg.len() as u64;
                let _ = inner.screen.frames.send(ViewerFrame {
                    from: peer,
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
    if inner.screen.viewing.lock().remove(&peer) {
        inner.emit(NodeEvent::ScreenEnded { peer });
    }
}

impl super::Node {
    /// Asks `peer` to share its screen with this device (needs `screen_view` there).
    pub fn screen_start(&self, peer: &DeviceId, max_width: u32, fps: u8) -> Result<()> {
        self.view(peer, max_width, fps, "screen")
    }

    /// Accepts `peer`'s camera as a webcam (after a `CameraOffered` event).
    pub fn camera_start(&self, peer: &DeviceId, max_width: u32, fps: u8) -> Result<()> {
        self.view(peer, max_width, fps, "camera")
    }

    fn view(&self, peer: &DeviceId, max_width: u32, fps: u8, source: &str) -> Result<()> {
        self.inner.screen.viewing.lock().insert(*peer);
        self.inner.send_to(peer, ControlMsg::ScreenStart { max_width, fps, source: source.to_string() }).inspect_err(
            |_| {
                self.inner.screen.viewing.lock().remove(peer);
            },
        )
    }

    /// Offers this device's camera to `peer` as a webcam.
    pub fn camera_offer(&self, peer: &DeviceId) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::CameraOffer)
    }

    /// What each current viewer asked for ("screen" or "camera").
    pub fn screen_sources(&self) -> Vec<(DeviceId, String)> {
        self.inner.screen.sources.lock().iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    /// Stops viewing `peer`'s screen.
    pub fn screen_stop(&self, peer: &DeviceId) -> Result<()> {
        self.inner.screen.viewing.lock().remove(peer);
        self.inner.send_to(peer, ControlMsg::ScreenStop)
    }

    /// Stops sharing this device's screen with `peer` (the sharer's own stop button).
    pub fn screen_stop_sharing(&self, peer: &DeviceId) {
        stop_sharing(&self.inner, peer);
    }

    /// Whether this device asked to view `peer`'s screen and hasn't stopped.
    pub fn is_viewing(&self, peer: &DeviceId) -> bool {
        self.inner.screen.viewing.lock().contains(peer)
    }

    /// Frames from devices this one is viewing.
    pub fn subscribe_frames(&self) -> broadcast::Receiver<ViewerFrame> {
        self.inner.screen.frames.subscribe()
    }

    /// Sharer side: the host hands over its newest encoded frame.
    pub fn push_screen_frame(&self, w: u32, h: u32, jpeg: Vec<u8>) -> Result<()> {
        self.push_screen_frame_captured(w, h, jpeg, Instant::now())
    }

    /// Like `push_screen_frame`, with the moment the picture was captured, so the latency
    /// viewers see includes capture and encoding (capture → shown → acknowledged).
    pub fn push_screen_frame_captured(&self, w: u32, h: u32, jpeg: Vec<u8>, captured: Instant) -> Result<()> {
        if jpeg.len() > SCREEN_FRAME_LIMIT - 1024 {
            return Err(Error::InvalidInput("frame too large".into()));
        }
        self.inner.screen.latest.send_replace(Some(Arc::new(EncodedFrame { w, h, jpeg, captured })));
        Ok(())
    }

    /// How many devices are watching this screen right now.
    pub fn screen_viewers(&self) -> Vec<DeviceId> {
        self.inner.screen.sharing.lock().keys().copied().collect()
    }

    /// What the host's encoder should produce (adapted to the network).
    pub fn screen_target(&self) -> ScreenTarget {
        *self.inner.screen.target.read()
    }
}
