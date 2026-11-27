//! Live audio: one device streams sound to another: what it is playing ("relay", e.g. a
//! phone's sound in the headphones connected to the computer, or the computer's sound in
//! the phone's) or its microphone ("mic").
//!
//! * On the local network, chunks are raw interleaved s16le PCM: no codec delay.
//! * Over the internet or a relay, and when the receiver runs 1.1 or later, they are
//!   **Opus** packets of 20 ms (pure Rust, no C): about 128 kbit/s for stereo sound and
//!   48 kbit/s for a microphone, instead of 1.5 Mbit/s.
//!
//! The receiver always hands raw PCM to its host, whichever codec travelled, and its host
//! can smooth network jitter with [`JitterBuffer`]. The receiver needs the `bluetooth`
//! permission ("sound and accessories"). A small bounded queue on the sender drops audio
//! rather than letting delay build up.

use std::collections::VecDeque;
use std::sync::Arc;

use super::link::{LinkRecv as RecvStream, LinkSend as SendStream};
use tokio::sync::{broadcast, mpsc};

use super::{Inner, NodeEvent};
use crate::error::{Error, Result};
use crate::model::{ConnectionKind, DeviceId};
use crate::protocol::{AUDIO_CHUNK_LIMIT, AudioChunk, StreamOpen, read_message, write_message};

/// Audio received from another device (always PCM by the time it gets here).
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

/// Opus can encode these rates.
fn opus_rate(rate: u32) -> bool {
    matches!(rate, 8_000 | 12_000 | 16_000 | 24_000 | 48_000)
}

/// "1.1.0" or later understands Opus audio.
fn speaks_opus(app_version: &str) -> bool {
    let mut parts = app_version.split(['.', '-', ' ']).filter_map(|p| p.parse::<u32>().ok());
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    (major, minor) >= (1, 1)
}

/// Sender side: push PCM chunks; dropping it ends the stream.
pub struct AudioSender {
    tx: mpsc::Sender<Vec<u8>>,
    /// Whether the stream is compressed (for the UI).
    pub opus: bool,
}

impl AudioSender {
    /// Queues a chunk; if the network is behind, the chunk is dropped (no delay builds up).
    pub fn push(&self, pcm: Vec<u8>) -> bool {
        if pcm.is_empty() || pcm.len() > AUDIO_CHUNK_LIMIT - 64 {
            return false;
        }
        self.tx.try_send(pcm).is_ok()
    }

    /// The stream ended (the receiver stopped it, or the connection went).
    pub fn closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// Splits PCM into 20 ms frames and encodes each to one Opus packet.
struct Encoder {
    enc: opus_pure::OpusEncoder,
    frame: usize,
    channels: usize,
    pending: Vec<i16>,
    out: Vec<u8>,
}

impl Encoder {
    fn new(rate: u32, channels: u8, kind: &str) -> Option<Self> {
        let app = if kind == "mic" { opus_pure::Application::Voip } else { opus_pure::Application::Audio };
        let mut enc = opus_pure::OpusEncoder::new(rate as i32, channels as usize, app).ok()?;
        enc.bitrate_bps = match (kind, channels) {
            ("mic", _) => 48_000,
            (_, 1) => 96_000,
            _ => 128_000,
        };
        Some(Self {
            enc,
            frame: (rate / 50) as usize,
            channels: channels as usize,
            pending: Vec::new(),
            out: vec![0u8; opus_pure::MAX_PACKET_BYTES],
        })
    }

    /// Every complete 20 ms frame in `pcm` (plus what was left over) as packets.
    fn feed(&mut self, pcm: &[u8]) -> Vec<Vec<u8>> {
        self.pending.extend(pcm.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])));
        let need = self.frame * self.channels;
        let mut packets = Vec::new();
        while self.pending.len() >= need {
            let frame: Vec<i16> = self.pending.drain(..need).collect();
            if let Ok(n) = self.enc.encode_s16(&frame, self.frame, &mut self.out) {
                packets.push(self.out[..n].to_vec());
            }
        }
        packets
    }
}

struct Decoder {
    dec: opus_pure::OpusDecoder,
    frame: usize,
    out: Vec<i16>,
}

impl Decoder {
    fn new(rate: u32, channels: u8) -> Option<Self> {
        let dec = opus_pure::OpusDecoder::new(rate as i32, channels as usize).ok()?;
        // The largest Opus frame is 120 ms.
        let frame = (rate as usize * 120) / 1000;
        Some(Self { dec, frame, out: vec![0i16; frame * channels as usize] })
    }

    fn decode(&mut self, packet: &[u8], channels: u8) -> Option<Vec<u8>> {
        let n = self.dec.decode_s16(packet, self.frame, &mut self.out).ok()?;
        Some(self.out[..n * channels as usize].iter().flat_map(|s| s.to_le_bytes()).collect())
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn receive(
    inner: Arc<Inner>,
    peer: DeviceId,
    kind: String,
    sample_rate: u32,
    channels: u8,
    opus: bool,
    mut send: SendStream,
    mut recv: RecvStream,
) {
    let allowed = inner.permissions_of(&peer).is_some_and(|p| p.bluetooth);
    let valid = matches!(kind.as_str(), "relay" | "mic")
        && (8_000..=96_000).contains(&sample_rate)
        && (1..=2).contains(&channels)
        && (!opus || opus_rate(sample_rate));
    let mut decoder = if opus { Decoder::new(sample_rate, channels) } else { None };
    if !allowed || !valid || (opus && decoder.is_none()) {
        let _ = send.reset(0);
        let _ = recv.stop(0);
        return;
    }
    let name = inner.peer_name(&peer);
    inner.log(Some((&peer, &name)), "audio_received", &kind);
    inner.emit(NodeEvent::AudioStarted { from: peer, from_name: name, kind: kind.clone(), sample_rate, channels });
    while let Ok(Some(chunk)) = read_message::<_, AudioChunk>(&mut recv, AUDIO_CHUNK_LIMIT).await {
        let pcm = match decoder.as_mut() {
            Some(d) => match d.decode(&chunk.pcm, channels) {
                Some(p) => p,
                None => continue, // a damaged packet: skip it
            },
            None => chunk.pcm.into_vec(),
        };
        let _ = inner.audio.packets.send(AudioPacket {
            from: peer,
            kind: kind.clone(),
            sample_rate,
            channels,
            pcm: Arc::new(pcm),
        });
    }
    inner.emit(NodeEvent::AudioEnded { from: peer, kind });
}

impl super::Node {
    /// Starts streaming audio to `peer` ("relay" or "mic"). Opus is used automatically
    /// away from the local network when the peer understands it.
    pub fn start_audio(&self, peer: &DeviceId, kind: &str, sample_rate: u32, channels: u8) -> Result<AudioSender> {
        if !matches!(kind, "relay" | "mic") {
            return Err(Error::InvalidInput("audio kind must be relay or mic".into()));
        }
        let (conn, version) = self
            .inner
            .peers
            .read()
            .get(peer)
            .and_then(|p| p.session.as_ref().map(|s| (s.conn.clone(), p.record.info.app_version.clone())))
            .ok_or_else(|| Error::Connection("that device is offline".into()))?;
        let far = conn.path().kind != ConnectionKind::Lan;
        let opus = far && speaks_opus(&version) && opus_rate(sample_rate) && (1..=2).contains(&channels);
        self.start_audio_with(conn, kind, sample_rate, channels, opus)
    }

    fn start_audio_with(
        &self,
        conn: super::link::Link,
        kind: &str,
        sample_rate: u32,
        channels: u8,
        opus: bool,
    ) -> Result<AudioSender> {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
        let kind = kind.to_string();
        let mut encoder = if opus { Encoder::new(sample_rate, channels, &kind) } else { None };
        let opus = encoder.is_some();
        self.inner.rt.spawn(async move {
            let Ok((mut send, _recv)) = conn.open_bi().await else { return };
            let codec = opus.then(|| "opus".to_string());
            if write_message(&mut send, &StreamOpen::Audio { kind, sample_rate, channels, codec }).await.is_err() {
                return;
            }
            let mut seq = 0u64;
            while let Some(pcm) = rx.recv().await {
                let payloads = match encoder.as_mut() {
                    Some(e) => e.feed(&pcm),
                    None => vec![pcm],
                };
                for p in payloads {
                    seq += 1;
                    if write_message(&mut send, &AudioChunk { seq, pcm: serde_bytes::ByteBuf::from(p) }).await.is_err()
                    {
                        return;
                    }
                }
            }
            let _ = send.finish();
        });
        Ok(AudioSender { tx, opus })
    }

    /// Audio other devices stream to this one.
    pub fn subscribe_audio(&self) -> broadcast::Receiver<AudioPacket> {
        self.inner.audio.packets.subscribe()
    }
}

/// Smooths network jitter for a player that pulls samples at a steady rate.
///
/// It waits until `target` samples are buffered before playing, plays silence (and
/// waits again) when it runs dry, and drops the oldest audio when more than three times
/// the target has piled up, so delay can't creep up over a long session.
pub struct JitterBuffer {
    q: VecDeque<i16>,
    target: usize,
    playing: bool,
    /// Samples of silence played because audio was late (for diagnostics).
    pub underruns: u64,
}

impl JitterBuffer {
    /// `target_ms` of `channels`-channel audio at `rate`.
    pub fn new(rate: u32, channels: u8, target_ms: u32) -> Self {
        let target = (rate as usize * channels as usize * target_ms as usize) / 1000;
        Self { q: VecDeque::with_capacity(target * 4), target: target.max(1), playing: false, underruns: 0 }
    }

    pub fn push(&mut self, pcm: &[u8]) {
        self.q.extend(pcm.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])));
        let max = self.target * 3;
        if self.q.len() > max {
            let extra = self.q.len() - self.target;
            self.q.drain(..extra);
        }
    }

    /// Fills `out`; silence while buffering.
    pub fn pull(&mut self, out: &mut [i16]) {
        if !self.playing && self.q.len() >= self.target {
            self.playing = true;
        }
        for s in out.iter_mut() {
            *s = if self.playing {
                match self.q.pop_front() {
                    Some(v) => v,
                    None => {
                        self.playing = false;
                        self.underruns += 1;
                        0
                    }
                }
            } else {
                0
            };
        }
    }

    pub fn buffered(&self) -> usize {
        self.q.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_round_trip_keeps_the_sound_and_shrinks_it() {
        let (rate, ch) = (48_000u32, 2u8);
        let mut enc = Encoder::new(rate, ch, "relay").unwrap();
        let mut dec = Decoder::new(rate, ch).unwrap();
        // One second of a 440 Hz tone, sent as 10 ms chunks (frames don't line up).
        let tone: Vec<u8> = (0..rate as usize)
            .flat_map(|i| {
                let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12_000.0) as i16;
                [v, v]
            })
            .flat_map(i16::to_le_bytes)
            .collect();
        let (mut packets, mut bytes) = (0, 0);
        let mut out = Vec::new();
        for chunk in tone.chunks(rate as usize / 100 * 4) {
            for p in enc.feed(chunk) {
                packets += 1;
                bytes += p.len();
                out.extend(dec.decode(&p, ch).unwrap());
            }
        }
        assert_eq!(packets, 50, "20 ms packets");
        assert!(bytes < tone.len() / 8, "compressed: {bytes} of {} bytes", tone.len());
        // The tone survives: loud, and not clipped noise.
        let samples: Vec<i16> = out.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        let rms = (samples.iter().skip(4_800).map(|&s| (s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
        assert!(rms > 5_000.0 && rms < 12_000.0, "rms {rms}");
    }

    #[test]
    fn versions_that_speak_opus() {
        assert!(speaks_opus("1.1.0"));
        assert!(speaks_opus("2.0.0"));
        assert!(!speaks_opus("1.0.2"));
        assert!(!speaks_opus(""));
    }

    #[test]
    fn jitter_buffer_waits_then_plays_and_caps_delay() {
        let mut j = JitterBuffer::new(1_000, 1, 100); // target 100 samples
        let chunk: Vec<u8> = (1..=50i16).flat_map(i16::to_le_bytes).collect();
        let mut out = [0i16; 10];
        j.push(&chunk);
        j.pull(&mut out);
        assert_eq!(out, [0; 10], "still buffering at 50 of 100");
        j.push(&chunk);
        j.pull(&mut out);
        assert_eq!(out[0], 1, "playing once the target is reached");
        for _ in 0..10 {
            j.push(&chunk);
        }
        assert!(j.buffered() <= 300, "never more than 3× the target: {}", j.buffered());
        let mut big = vec![0i16; 1_000];
        j.pull(&mut big);
        assert!(j.underruns > 0, "ran dry and noted it");
    }
}
