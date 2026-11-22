//! Phase 6 on the Hub: audio and video other devices send here.
//!
//! * **Audio relay** ("relay"): the phone's sound plays on this computer's current
//!   output — including Bluetooth earbuds connected to it.
//! * **Phone as microphone** ("mic"): on Linux the PCM goes into a virtual
//!   "Devti phone microphone" source (a PipeWire/PulseAudio null sink + remapped
//!   monitor) that any app can pick. On Windows there is no free virtual-mic driver, so
//!   it plays to "CABLE Input" when VB-Audio's free Virtual Cable is installed, or the
//!   default output otherwise, and says so.
//! * **Phone as webcam**: on Linux, frames go to a v4l2loopback device as YUYV, so
//!   browsers and video apps see a camera. Without the module loaded, the UI explains
//!   the one command needed.

use std::collections::HashMap;
use std::sync::Arc;

use devti_core::model::DeviceId;
use devti_core::{Node, NodeEvent};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Debug, Serialize, Default)]
pub struct MediaStatus {
    /// Audio streams playing here: (device, kind).
    pub audio: Vec<(String, String)>,
    /// Devices whose camera this computer is using.
    pub cameras: Vec<String>,
    /// Where camera frames go, or why they can't.
    pub webcam: String,
    pub mic: String,
}

pub struct Media {
    node: Node,
    app: AppHandle,
    players: Mutex<HashMap<(DeviceId, String), player::Player>>,
    cameras: Mutex<HashMap<DeviceId, webcam::Sink>>,
    status: Mutex<MediaStatus>,
}

impl Media {
    pub fn start(app: &AppHandle, node: Node) -> Arc<Self> {
        let me = Arc::new(Self {
            node: node.clone(),
            app: app.clone(),
            players: Mutex::new(HashMap::new()),
            cameras: Mutex::new(HashMap::new()),
            status: Mutex::new(MediaStatus {
                webcam: webcam::describe(),
                mic: player::describe_mic(),
                ..Default::default()
            }),
        });
        // Audio packets → players.
        let m = me.clone();
        let mut audio = node.subscribe_audio();
        tauri::async_runtime::spawn(async move {
            loop {
                match audio.recv().await {
                    Ok(p) => {
                        if let Some(pl) = m.players.lock().get_mut(&(p.from, p.kind.clone())) {
                            pl.write(&p.pcm);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        });
        // Camera frames → webcam sinks.
        let m = me.clone();
        let mut frames = node.subscribe_frames();
        tauri::async_runtime::spawn(async move {
            loop {
                match frames.recv().await {
                    Ok(f) => {
                        if let Some(sink) = m.cameras.lock().get_mut(&f.from) {
                            sink.frame(&f.jpeg, f.w, f.h);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        });
        me
    }

    pub fn status(&self) -> MediaStatus {
        let mut s = self.status.lock().clone();
        s.audio = self.players.lock().keys().map(|(d, k)| (d.to_string(), k.clone())).collect();
        s.cameras = self.cameras.lock().keys().map(|d| d.to_string()).collect();
        s
    }

    fn publish(&self) {
        let _ = self.app.emit("devti://media", self.status());
    }

    /// Use a phone's camera as this computer's webcam.
    pub fn use_camera(&self, peer: &DeviceId) -> Result<(), String> {
        let sink = webcam::Sink::open()?;
        self.node.camera_start(peer, 1280, 30).map_err(|e| e.to_string())?;
        self.cameras.lock().insert(*peer, sink);
        self.publish();
        Ok(())
    }

    pub fn stop_camera(&self, peer: &DeviceId) {
        self.cameras.lock().remove(peer);
        let _ = self.node.screen_stop(peer);
        self.publish();
    }

    pub fn on_node_event(&self, ev: &NodeEvent) {
        match ev {
            NodeEvent::AudioStarted { from, kind, sample_rate, channels, .. } => {
                match player::Player::open(kind, *sample_rate, *channels) {
                    Ok(p) => {
                        self.players.lock().insert((*from, kind.clone()), p);
                    }
                    Err(e) => {
                        let _ = self.app.emit("devti://notice", format!("Couldn't play the audio: {e}"));
                    }
                }
                self.publish();
            }
            NodeEvent::AudioEnded { from, kind } => {
                self.players.lock().remove(&(*from, kind.clone()));
                self.publish();
            }
            NodeEvent::ScreenEnded { peer } => {
                if self.cameras.lock().remove(peer).is_some() {
                    self.publish();
                }
            }
            NodeEvent::CameraOffered { from, from_name } => {
                let msg = match self.use_camera(from) {
                    Ok(()) => format!("Using {from_name}'s camera as a webcam."),
                    Err(e) => format!("{from_name} offered its camera: {e}"),
                };
                let _ = self.app.emit("devti://notice", msg);
            }
            _ => {}
        }
    }
}

pub fn app_state(app: &AppHandle) -> Option<Arc<Media>> {
    app.try_state::<Arc<Media>>().map(|s| s.inner().clone())
}

/// JPEG → YUYV 4:2:2 (what v4l2loopback consumers expect).
pub fn rgb_to_yuyv(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 2);
    for y in 0..h {
        for x in (0..w).step_by(2) {
            let px = |xx: usize| {
                let i = (y * w + xx.min(w - 1)) * 3;
                (rgb[i] as f32, rgb[i + 1] as f32, rgb[i + 2] as f32)
            };
            let (r0, g0, b0) = px(x);
            let (r1, g1, b1) = px(x + 1);
            let yy = |r: f32, g: f32, b: f32| (0.257 * r + 0.504 * g + 0.098 * b + 16.0).clamp(0.0, 255.0) as u8;
            let (r, g, b) = ((r0 + r1) / 2.0, (g0 + g1) / 2.0, (b0 + b1) / 2.0);
            let u = (-0.148 * r - 0.291 * g + 0.439 * b + 128.0).clamp(0.0, 255.0) as u8;
            let v = (0.439 * r - 0.368 * g - 0.071 * b + 128.0).clamp(0.0, 255.0) as u8;
            out.extend_from_slice(&[yy(r0, g0, b0), u, yy(r1, g1, b1), v]);
        }
    }
    out
}

pub fn decode_jpeg(jpeg: &[u8]) -> Option<(Vec<u8>, usize, usize)> {
    use zune_jpeg::JpegDecoder;
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let mut d = JpegDecoder::new_with_options(
        ZCursor::new(jpeg),
        DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB),
    );
    let pixels = d.decode().ok()?;
    let info = d.info()?;
    Some((pixels, info.width as usize, info.height as usize))
}

// ------------------------------------------------------------------ audio

#[cfg(target_os = "linux")]
mod player {
    use std::io::Write;
    use std::process::{Child, Command, Stdio};

    pub struct Player {
        child: Child,
    }

    const MIC_SINK: &str = "devti_mic";

    /// Creates the virtual microphone once (PipeWire or PulseAudio).
    fn ensure_virtual_mic() -> Result<(), String> {
        let sinks =
            Command::new("pactl").args(["list", "short", "sinks"]).output().map_err(|_| "pactl isn't installed")?;
        if String::from_utf8_lossy(&sinks.stdout).contains(MIC_SINK) {
            return Ok(());
        }
        let ok = Command::new("pactl")
            .args([
                "load-module",
                "module-null-sink",
                &format!("sink_name={MIC_SINK}"),
                "sink_properties=device.description=Devti-phone-output",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
            && Command::new("pactl")
                .args([
                    "load-module",
                    "module-remap-source",
                    &format!("master={MIC_SINK}.monitor"),
                    "source_name=devti_mic_source",
                    "source_properties=device.description=Devti-phone-microphone",
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        if ok { Ok(()) } else { Err("couldn't create the virtual microphone".into()) }
    }

    pub fn describe_mic() -> String {
        if Command::new("pactl").arg("info").output().is_ok() {
            "Shows up as “Devti-phone-microphone” in any app's microphone list.".into()
        } else {
            "Needs PipeWire or PulseAudio (pactl).".into()
        }
    }

    impl Player {
        pub fn open(kind: &str, rate: u32, channels: u8) -> Result<Self, String> {
            let mut cmd = Command::new("pacat");
            cmd.args([
                "--playback",
                "--format=s16le",
                &format!("--rate={rate}"),
                &format!("--channels={channels}"),
                "--latency-msec=80",
            ]);
            if kind == "mic" {
                ensure_virtual_mic()?;
                cmd.arg(format!("--device={MIC_SINK}"));
            }
            let child = cmd
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "pacat isn't installed (pulseaudio-utils)".to_string())?;
            Ok(Self { child })
        }

        pub fn write(&mut self, pcm: &[u8]) {
            if let Some(stdin) = self.child.stdin.as_mut() {
                let _ = stdin.write_all(pcm);
            }
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod player {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    pub struct Player {
        buf: Arc<Mutex<VecDeque<i16>>>,
        _stream: cpal::Stream,
    }

    pub fn describe_mic() -> String {
        "Windows has no free virtual microphone; install VB-Audio's free Virtual Cable and Devti plays the phone's mic into “CABLE Input” (pick “CABLE Output” as the microphone).".into()
    }

    impl Player {
        pub fn open(kind: &str, rate: u32, channels: u8) -> Result<Self, String> {
            let host = cpal::default_host();
            let device = if kind == "mic" {
                host.output_devices()
                    .ok()
                    .and_then(|mut it| {
                        it.find(|d| d.description().map(|n| n.name().contains("CABLE Input")).unwrap_or(false))
                    })
                    .or_else(|| host.default_output_device())
            } else {
                host.default_output_device()
            }
            .ok_or("no audio output")?;
            let config = cpal::StreamConfig {
                channels: channels as u16,
                sample_rate: rate,
                buffer_size: cpal::BufferSize::Default,
            };
            let buf: Arc<Mutex<VecDeque<i16>>> = Arc::new(Mutex::new(VecDeque::new()));
            let b = buf.clone();
            let stream = device
                .build_output_stream(
                    config,
                    move |out: &mut [i16], _| {
                        let mut q = b.lock().unwrap();
                        for s in out.iter_mut() {
                            *s = q.pop_front().unwrap_or(0);
                        }
                    },
                    |e| tracing::warn!(error = %e, "audio output error"),
                    None,
                )
                .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            Ok(Self { buf, _stream: stream })
        }

        pub fn write(&mut self, pcm: &[u8]) {
            let mut q = self.buf.lock().unwrap();
            for c in pcm.chunks_exact(2) {
                q.push_back(i16::from_le_bytes([c[0], c[1]]));
            }
            // Keep at most ~0.5 s queued: late audio is worse than a skip.
            let max = 48_000;
            while q.len() > max {
                q.pop_front();
            }
        }
    }
}

// ----------------------------------------------------------------- webcam

#[cfg(target_os = "linux")]
mod webcam {
    use std::fs::File;
    use std::io::Write;
    use std::os::fd::AsRawFd;

    pub struct Sink {
        file: File,
        size: Option<(usize, usize)>,
    }

    /// The first v4l2loopback device (by its card name).
    fn find() -> Option<String> {
        let rd = std::fs::read_dir("/sys/class/video4linux").ok()?;
        let mut found = None;
        for e in rd.flatten() {
            let name = std::fs::read_to_string(e.path().join("name")).unwrap_or_default().to_lowercase();
            if name.contains("devti") || name.contains("loopback") || name.contains("dummy") {
                let dev = format!("/dev/{}", e.file_name().to_string_lossy());
                if name.contains("devti") {
                    return Some(dev);
                }
                found.get_or_insert(dev);
            }
        }
        found
    }

    pub fn describe() -> String {
        match find() {
            Some(d) => format!("Webcam ready ({d})."),
            None => "To use a phone as a webcam, load v4l2loopback once: sudo modprobe v4l2loopback card_label=\"Devti Camera\" exclusive_caps=1".into(),
        }
    }

    #[repr(C)]
    struct PixFormat {
        width: u32,
        height: u32,
        pixelformat: u32,
        field: u32,
        bytesperline: u32,
        sizeimage: u32,
        colorspace: u32,
        priv_: u32,
        flags: u32,
        enc: u32,
        quantization: u32,
        xfer_func: u32,
    }

    #[repr(C)]
    struct Format {
        kind: u32,
        _pad: u32,
        pix: PixFormat,
        _rest: [u8; 200 - std::mem::size_of::<PixFormat>()],
    }

    const VIDIOC_S_FMT: libc::c_ulong = 0xC0D0_5605;
    const OUTPUT: u32 = 2;
    const YUYV: u32 = u32::from_le_bytes(*b"YUYV");

    impl Sink {
        pub fn open() -> Result<Self, String> {
            let path = find().ok_or_else(describe)?;
            let file =
                std::fs::OpenOptions::new().write(true).open(&path).map_err(|e| format!("can't open {path}: {e}"))?;
            Ok(Self { file, size: None })
        }

        fn set_format(&mut self, w: usize, h: usize) -> bool {
            let mut f = Format {
                kind: OUTPUT,
                _pad: 0,
                pix: PixFormat {
                    width: w as u32,
                    height: h as u32,
                    pixelformat: YUYV,
                    field: 1,
                    bytesperline: (w * 2) as u32,
                    sizeimage: (w * h * 2) as u32,
                    colorspace: 8,
                    priv_: 0,
                    flags: 0,
                    enc: 0,
                    quantization: 0,
                    xfer_func: 0,
                },
                _rest: [0; 200 - std::mem::size_of::<PixFormat>()],
            };
            // SAFETY: VIDIOC_S_FMT with a correctly sized v4l2_format on our own fd.
            unsafe { libc::ioctl(self.file.as_raw_fd(), VIDIOC_S_FMT, &mut f as *mut Format) == 0 }
        }

        pub fn frame(&mut self, jpeg: &[u8], _w: u32, _h: u32) {
            let Some((rgb, w, h)) = super::decode_jpeg(jpeg) else { return };
            let w = w & !1;
            if self.size != Some((w, h)) {
                if !self.set_format(w, h) {
                    return;
                }
                self.size = Some((w, h));
            }
            let yuyv = super::rgb_to_yuyv(&rgb, w, h);
            let _ = self.file.write_all(&yuyv);
        }
    }

    #[test]
    fn v4l2_format_is_the_kernel_size() {
        assert_eq!(std::mem::size_of::<Format>(), 208);
    }
}

#[cfg(not(target_os = "linux"))]
mod webcam {
    pub struct Sink;

    pub fn describe() -> String {
        "Windows has no free virtual-camera driver. Open the phone camera in Devti and capture that window with OBS (Window Capture → Start Virtual Camera).".into()
    }

    impl Sink {
        pub fn open() -> Result<Self, String> {
            Err(describe())
        }

        pub fn frame(&mut self, _jpeg: &[u8], _w: u32, _h: u32) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_of_white_and_black() {
        let white = vec![255u8; 2 * 3];
        let y = rgb_to_yuyv(&white, 2, 1);
        assert_eq!(y.len(), 4);
        assert!(y[0] >= 234 && y[2] >= 234, "bright Y {y:?}");
        assert!((126..=130).contains(&y[1]) && (126..=130).contains(&y[3]), "neutral chroma {y:?}");
        let black = vec![0u8; 2 * 3];
        assert_eq!(rgb_to_yuyv(&black, 2, 1)[0], 16);
    }
}
