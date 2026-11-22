//! Phase 6 live audio: one device streams PCM to another — what it is playing ("relay",
//! e.g. phone audio out through earbuds connected to the Hub) or its microphone ("mic",
//! the phone as the computer's microphone).
//!
//! Interleaved s16le chunks travel on an `audio` stream opened by the sender. The
//! receiver needs the `bluetooth` permission ("Use Bluetooth accessories" covers
//! sharing audio devices). A small bounded queue on the sender drops the oldest audio
//! rather than letting delay build up.

use std::sync::Arc;

use quinn::{RecvStream, SendStream, VarInt};
use tokio::sync::{broadcast, mpsc};

use super::{Inner, NodeEvent};
use crate::error::{Error, Result};
use crate::model::DeviceId;
use crate::protocol::{AUDIO_CHUNK_LIMIT, AudioChunk, StreamOpen, read_message, write_message};

/// Audio received from another device.
#[derive(Clone, Debug)]
pub struct AudioPacket {
    pub from: DeviceId,
    pub kind: String,
    pub sample_rate: u32,
    pub channels: u8,
    pub pcm: Arc<Vec<u8>>,
}

pub(crate) struct State {
    packets: broadcast::Sender<AudioPacket>,
}

impl State {
    pub fn new() -> Self {
        Self { packets: broadcast::channel(64).0 }
    }
}

/// Sender side: push PCM chunks; dropping it ends the stream.
pub struct AudioSender {
    tx: mpsc::Sender<Vec<u8>>,
}

impl AudioSender {
    /// Queues a chunk; if the network is behind, the chunk is dropped (no delay builds up).
    pub fn push(&self, pcm: Vec<u8>) -> bool {
        if pcm.is_empty() || pcm.len() > AUDIO_CHUNK_LIMIT - 64 {
            return false;
        }
        self.tx.try_send(pcm).is_ok()
    }
}

pub(crate) async fn receive(
    inner: Arc<Inner>,
    peer: DeviceId,
    kind: String,
    sample_rate: u32,
    channels: u8,
    mut send: SendStream,
    mut recv: RecvStream,
) {
    let allowed = inner.permissions_of(&peer).is_some_and(|p| p.bluetooth);
    let valid = matches!(kind.as_str(), "relay" | "mic")
        && (8_000..=96_000).contains(&sample_rate)
        && (1..=2).contains(&channels);
    if !allowed || !valid {
        let _ = send.reset(VarInt::from_u32(0));
        let _ = recv.stop(VarInt::from_u32(0));
        return;
    }
    let name = inner.peer_name(&peer);
    inner.log(Some((&peer, &name)), "audio_received", &kind);
    inner.emit(NodeEvent::AudioStarted { from: peer, from_name: name, kind: kind.clone(), sample_rate, channels });
    while let Ok(Some(chunk)) = read_message::<_, AudioChunk>(&mut recv, AUDIO_CHUNK_LIMIT).await {
        let _ = inner.audio.packets.send(AudioPacket {
            from: peer,
            kind: kind.clone(),
            sample_rate,
            channels,
            pcm: Arc::new(chunk.pcm.into_vec()),
        });
    }
    inner.emit(NodeEvent::AudioEnded { from: peer, kind });
}

impl super::Node {
    /// Starts streaming audio to `peer` ("relay" or "mic").
    pub fn start_audio(&self, peer: &DeviceId, kind: &str, sample_rate: u32, channels: u8) -> Result<AudioSender> {
        if !matches!(kind, "relay" | "mic") {
            return Err(Error::InvalidInput("audio kind must be relay or mic".into()));
        }
        let conn = self
            .inner
            .peers
            .read()
            .get(peer)
            .and_then(|p| p.session.as_ref().map(|s| s.conn.clone()))
            .ok_or_else(|| Error::Connection("that device is offline".into()))?;
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
        let kind = kind.to_string();
        self.inner.rt.spawn(async move {
            let Ok((mut send, _recv)) = conn.open_bi().await else { return };
            if write_message(&mut send, &StreamOpen::Audio { kind, sample_rate, channels }).await.is_err() {
                return;
            }
            let mut seq = 0u64;
            while let Some(pcm) = rx.recv().await {
                seq += 1;
                if write_message(&mut send, &AudioChunk { seq, pcm: serde_bytes::ByteBuf::from(pcm) }).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });
        Ok(AudioSender { tx })
    }

    /// Audio other devices stream to this one.
    pub fn subscribe_audio(&self) -> broadcast::Receiver<AudioPacket> {
        self.inner.audio.packets.subscribe()
    }
}
