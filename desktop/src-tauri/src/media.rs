//! Phase 6 on the Hub: audio and video other devices send here.
//!
//! * **Audio relay** ("relay"): the phone's sound plays on this computer's current
//!   output — including Bluetooth earbuds connected to it.
//! * **Phone as microphone** ("mic"): on Linux the PCM goes into a virtual
//!   "Devti phone microphone" source (a PipeWire/PulseAudio null sink + remapped
//!   monitor) that any app can pick. On Windows there is no free virtual-mic driver, so
//!   it plays to "CABLE Input" when VB-Audio's free Virtual Cable is installed, or the
//!   default output otherwise, and says so.
//! * **Phone camera**: always shown live in Devti's camera view (front or back). On
//!   Linux the frames also go to a v4l2loopback device as YUYV, so browsers and video
//!   apps see a "Devti Camera". The Hub can set the module up with one password prompt.
//!   On Windows the camera view can be captured by OBS's virtual camera.

use std::collections::HashMap;
use std::sync::Arc;

use devti_core::model::DeviceId;
use devti_core::{Node, NodeEvent};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// A phone camera this computer is using.
#[derive(Clone, Debug, Serialize)]
pub struct CameraState {
    pub device: String,
    /// Frames are arriving (false: waiting for the phone to allow it).
    pub live: bool,
    /// "front", "back", or null for the phone's choice.
    pub facing: Option<String>,
    /// Frames also go to the virtual webcam other apps see.
    pub webcam: bool,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct MediaStatus {
    /// Audio streams playing here: (device, kind).
    pub audio: Vec<(String, String)>,
    /// Phone cameras in use.
    pub cameras: Vec<CameraState>,
    /// Where camera frames go, or why they can't.
    pub webcam: String,
    /// A virtual webcam is available to other apps.
    pub webcam_ready: bool,
    /// Devti can set the virtual webcam up (Linux, not set up yet).
    pub webcam_setup: bool,
    pub mic: String,
    /// Recordings in progress.
    pub recordings: Vec<crate::recorder::RecordingState>,
    /// Where recordings are saved.
    pub recordings_folder: String,
}

struct Camera {
    sink: Option<webcam::Sink>,
    live: bool,
    facing: Option<String>,
    started: std::time::Instant,
}

pub struct Media {
    node: Node,
    app: AppHandle,
    players: Mutex<HashMap<(DeviceId, String), player::Player>>,
    cameras: Mutex<HashMap<DeviceId, Camera>>,
    recordings: Mutex<HashMap<(DeviceId, String), crate::recorder::Recording>>,
    status: Mutex<MediaStatus>,
}

impl Media {
    pub fn start(app: &AppHandle, node: Node) -> Arc<Self> {
        let me = Arc::new(Self {
            node: node.clone(),
            app: app.clone(),
            players: Mutex::new(HashMap::new()),
            cameras: Mutex::new(HashMap::new()),
            recordings: Mutex::new(HashMap::new()),
            status: Mutex::new(MediaStatus { mic: player::describe_mic(), ..Default::default() }),
        });
        // Audio packets → players.
        let m = me.clone();
        let mut audio = node.subscribe_audio();
        tauri::async_runtime::spawn(async move {
            loop {
                match audio.recv().await {
                    Ok(p) => {
                        let key = (p.from, p.kind.clone());
                        if let Some(r) = m.recordings.lock().get_mut(&key) {
                            r.write(&p.pcm);
                        }
                        if let Some(pl) = m.players.lock().get_mut(&key) {
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
                    Ok(f) if f.source == "camera" => {
                        let first = {
                            let mut cams = m.cameras.lock();
                            match cams.get_mut(&f.from) {
                                Some(cam) => {
                                    if let Some(sink) = cam.sink.as_mut() {
                                        sink.frame(&f.jpeg);
                                    }
                                    !std::mem::replace(&mut cam.live, true)
                                }
                                None => false,
                            }
                        };
                        if first {
                            m.publish();
                        }
                    }
                    Ok(_) => {}
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
        s.cameras = self
            .cameras
            .lock()
            .iter()
            .map(|(d, c)| CameraState {
                device: d.to_string(),
                live: c.live,
                facing: c.facing.clone(),
                webcam: c.sink.is_some(),
            })
            .collect();
        // Checked every time: the module can be loaded while Devti runs.
        s.webcam = webcam::describe();
        s.webcam_ready = webcam::ready();
        s.webcam_setup = webcam::can_set_up() && !s.webcam_ready;
        s.recordings = self.recordings.lock().iter().map(|((d, k), r)| r.state(&d.to_string(), k)).collect();
        s.recordings_folder = crate::recorder::folder().to_string_lossy().into();
        s
    }

    /// Records a phone's microphone ("mic") or sound ("relay") into `Music/Devti`. The
    /// phone asks its user first.
    pub fn start_recording(self: &Arc<Self>, peer: &DeviceId, kind: &str) -> Result<(), String> {
        let name = self.node.peer(peer).map(|p| p.info.name).ok_or("that device isn't paired")?;
        let key = (*peer, kind.to_string());
        if self.recordings.lock().contains_key(&key) {
            return Ok(());
        }
        self.recordings.lock().insert(key.clone(), crate::recorder::Recording::new(&name, kind));
        if let Err(e) = self.node.request_audio(peer, kind, true) {
            self.recordings.lock().remove(&key);
            return Err(e.to_string());
        }
        self.publish();
        // The phone never answered: say so and give up.
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(45)).await;
            let stale =
                me.recordings.lock().get(&key).is_some_and(|r| !r.state("", "").live && r.waited().as_secs() >= 44);
            if stale {
                me.recordings.lock().remove(&key);
                me.notice(format!("{name} didn't start sending its audio. Check the phone: it asks first."));
                me.publish();
            }
        });
        Ok(())
    }

    /// Stops a recording; the saved file's path.
    pub fn stop_recording(&self, peer: &DeviceId, kind: &str) -> Option<String> {
        let rec = self.recordings.lock().remove(&(*peer, kind.to_string()));
        let _ = self.node.request_audio(peer, kind, false);
        self.publish();
        rec.and_then(|r| r.finish()).map(|p| p.to_string_lossy().into_owned())
    }

    fn publish(&self) {
        let _ = self.app.emit("devti://media", self.status());
    }

    fn notice(&self, text: impl Into<String>) {
        let _ = self.app.emit("devti://notice", text.into());
    }

    /// Uses a phone's camera: shown in Devti, and fed to the virtual webcam when there is
    /// one. Asking again (another facing) switches camera without stopping.
    pub fn use_camera(self: &Arc<Self>, peer: &DeviceId, facing: Option<String>) -> Result<(), String> {
        let running = self.cameras.lock().contains_key(peer);
        if running {
            // Switch camera: stop and ask again with the new facing.
            self.node.camera_stop(peer).map_err(|e| e.to_string())?;
        }
        let sink = webcam::Sink::open().ok();
        self.node.camera_start_facing(peer, 1280, 30, facing.as_deref()).map_err(|e| e.to_string())?;
        self.cameras
            .lock()
            .insert(*peer, Camera { sink, live: false, facing: facing.clone(), started: std::time::Instant::now() });
        self.publish();
        // Tell the user if the phone never answers (permission screen ignored, app closed…).
        let me = self.clone();
        let peer = *peer;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(40)).await;
            let waiting = me.cameras.lock().get(&peer).is_some_and(|c| !c.live && c.started.elapsed().as_secs() >= 39);
            if waiting {
                let name = me.node.peer(&peer).map(|p| p.info.name).unwrap_or_else(|| "The phone".into());
                me.notice(format!(
                    "{name} hasn't sent its camera yet. Check the phone: it asks before the camera turns on."
                ));
            }
        });
        Ok(())
    }

    pub fn stop_camera(&self, peer: &DeviceId) {
        self.cameras.lock().remove(peer);
        let _ = self.node.camera_stop(peer);
        self.publish();
    }

    pub fn on_node_event(self: &Arc<Self>, ev: &NodeEvent) {
        match ev {
            NodeEvent::AudioStarted { from, kind, sample_rate, channels, .. } => {
                if let Some(r) = self.recordings.lock().get_mut(&(*from, kind.clone()))
                    && let Err(e) = r.begin(*sample_rate, *channels)
                {
                    self.notice(format!("Couldn't save the recording: {e}"));
                }
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
                if let Some(path) = self.recordings.lock().remove(&(*from, kind.clone())).and_then(|r| r.finish()) {
                    self.notice(format!("Recording saved: {path}", path = path.display()));
                }
                self.publish();
            }
            NodeEvent::ScreenEnded { peer, source } if source == "camera" => {
                if let Some(cam) = self.cameras.lock().remove(peer) {
                    let name = self.node.peer(peer).map(|p| p.info.name).unwrap_or_else(|| "The phone".into());
                    self.notice(if cam.live {
                        format!("{name}'s camera stopped.")
                    } else {
                        format!("{name} didn't turn its camera on.")
                    });
                }
                self.publish();
            }
            NodeEvent::CameraOffered { from, from_name } => self.on_camera_offered(from, from_name),
            _ => {}
        }
    }
}

impl Media {
    /// The phone offered its camera: accept it (the phone already asked its user).
    pub fn on_camera_offered(self: &Arc<Self>, from: &DeviceId, from_name: &str) {
        let msg = match self.use_camera(from, None) {
            Ok(()) => format!("Using {from_name}'s camera. Open it in Devti from the device page."),
            Err(e) => format!("{from_name} offered its camera: {e}"),
        };
        self.notice(msg);
    }
}

/// Sets up the virtual webcam for other apps (Linux).
pub fn set_up_webcam() -> Result<String, String> {
    webcam::set_up()
}

pub fn app_state(app: &AppHandle) -> Option<Arc<Media>> {
    app.try_state::<Arc<Media>>().map(|s| s.inner().clone())
}

/// The virtual webcam's fixed picture size. A fixed size means the format is set once,
/// so a phone turning sideways never makes the device busy for apps already using it.
pub const WEBCAM_W: usize = 1280;
pub const WEBCAM_H: usize = 720;

/// Scales an RGB picture to fit `dw`×`dh` (nearest neighbour), centred on black.
pub fn fit_rgb(rgb: &[u8], w: usize, h: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh * 3];
    if w == 0 || h == 0 || rgb.len() < w * h * 3 {
        return out;
    }
    // Largest size with the source's shape that fits.
    let (fw, fh) = if w * dh > h * dw { (dw, (h * dw / w).max(1)) } else { ((w * dh / h).max(1), dh) };
    let (ox, oy) = ((dw - fw) / 2, (dh - fh) / 2);
    for y in 0..fh {
        let sy = y * h / fh;
        let src_row = &rgb[sy * w * 3..(sy + 1) * w * 3];
        let dst_row = &mut out[((oy + y) * dw + ox) * 3..((oy + y) * dw + ox + fw) * 3];
        for x in 0..fw {
            let sx = x * w / fw;
            dst_row[x * 3..x * 3 + 3].copy_from_slice(&src_row[sx * 3..sx * 3 + 3]);
        }
    }
    out
}

/// JPEG → YUYV 4:2:2 (what v4l2loopback consumers expect). `w` must be even.
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

    pub fn ready() -> bool {
        find().is_some()
    }

    pub fn can_set_up() -> bool {
        true
    }

    /// Whether the v4l2loopback module is installed (loaded or not).
    fn module_installed() -> bool {
        std::process::Command::new("modinfo")
            .arg("v4l2loopback")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn install_hint() -> String {
        let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default().to_lowercase();
        if os.contains("fedora") || os.contains("rhel") {
            "Install it first: sudo dnf install v4l2loopback (from RPM Fusion), then press Set up again.".into()
        } else if os.contains("arch") {
            "Install it first: sudo pacman -S v4l2loopback-dkms, then press Set up again.".into()
        } else if os.contains("suse") {
            "Install it first: sudo zypper install v4l2loopback-kmp-default, then press Set up again.".into()
        } else {
            "Install it first: sudo apt install v4l2loopback-dkms, then press Set up again.".into()
        }
    }

    pub fn describe() -> String {
        match find() {
            Some(d) => format!("Other apps see the phone as “Devti Camera” ({d})."),
            None if module_installed() => {
                "Set up the virtual webcam once so Zoom, Meet and other apps can use the phone camera.".into()
            }
            None => format!("For other apps to use the phone camera, Linux needs v4l2loopback. {}", install_hint()),
        }
    }

    /// Loads v4l2loopback now and on every start (one password prompt).
    pub fn set_up() -> Result<String, String> {
        if find().is_some() {
            return Ok(describe());
        }
        if !module_installed() {
            return Err(format!("v4l2loopback isn't installed. {}", install_hint()));
        }
        let script = r#"set -e
modprobe v4l2loopback devices=1 video_nr=42 card_label="Devti Camera" exclusive_caps=1
printf 'v4l2loopback\n' > /etc/modules-load.d/devti-camera.conf
printf 'options v4l2loopback devices=1 video_nr=42 card_label="Devti Camera" exclusive_caps=1\n' > /etc/modprobe.d/devti-camera.conf"#;
        let out = std::process::Command::new("pkexec")
            .args(["sh", "-c", script])
            .output()
            .map_err(|_| "pkexec isn't available. Run in a terminal: sudo modprobe v4l2loopback card_label=\"Devti Camera\" exclusive_caps=1".to_string())?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if why.is_empty() || out.status.code() == Some(126) {
                "Setup was cancelled.".into()
            } else {
                format!("Couldn't load v4l2loopback: {why}")
            });
        }
        // udev needs a moment to create the device.
        for _ in 0..20 {
            if find().is_some() {
                return Ok(describe());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err("v4l2loopback loaded, but no camera device appeared.".into())
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
            let mut sink = Self { file };
            if !sink.set_format(super::WEBCAM_W, super::WEBCAM_H) {
                return Err(format!("{path} is busy (another app is feeding it)"));
            }
            Ok(sink)
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

        pub fn frame(&mut self, jpeg: &[u8]) {
            let Some((rgb, w, h)) = super::decode_jpeg(jpeg) else { return };
            let fitted = super::fit_rgb(&rgb, w, h, super::WEBCAM_W, super::WEBCAM_H);
            let yuyv = super::rgb_to_yuyv(&fitted, super::WEBCAM_W, super::WEBCAM_H);
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
        "The camera shows in Devti. For Zoom, Meet or Teams, capture Devti's camera window with OBS (Window Capture, then Start Virtual Camera).".into()
    }

    pub fn ready() -> bool {
        false
    }

    pub fn can_set_up() -> bool {
        false
    }

    pub fn set_up() -> Result<String, String> {
        Err(describe())
    }

    impl Sink {
        pub fn open() -> Result<Self, String> {
            Err(describe())
        }

        pub fn frame(&mut self, _jpeg: &[u8]) {}
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

    #[test]
    fn portrait_frames_are_letterboxed_into_the_fixed_webcam_size() {
        // A 3×4 portrait picture of solid red, odd width included.
        let red: Vec<u8> = std::iter::repeat_n([255u8, 0, 0], 12).flatten().collect();
        let out = fit_rgb(&red, 3, 4, 16, 9);
        assert_eq!(out.len(), 16 * 9 * 3);
        let px = |x: usize, y: usize| &out[(y * 16 + x) * 3..(y * 16 + x) * 3 + 3];
        assert_eq!(px(8, 4), &[255, 0, 0], "centre is the picture");
        assert_eq!(px(0, 4), &[0, 0, 0], "sides are black bars");
        assert_eq!(px(15, 4), &[0, 0, 0]);
        // Wider than 16:9 fills the width, with bars above and below.
        let wide: Vec<u8> = std::iter::repeat_n([0u8, 255, 0], 32 * 9).flatten().collect();
        let out = fit_rgb(&wide, 32, 9, 16, 9);
        let px = |x: usize, y: usize| &out[(y * 16 + x) * 3..(y * 16 + x) * 3 + 3];
        assert_eq!(px(0, 4), &[0, 255, 0], "edge to edge in the middle");
        assert_eq!(px(0, 0), &[0, 0, 0], "bar on top");
    }
}
