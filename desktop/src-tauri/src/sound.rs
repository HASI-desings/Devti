//! Shared headphones and pausing music for voice messages (1.1, D-57).
//!
//! **One pair of headphones for every device.** A Bluetooth headset plays one device at
//! a time, so Devti brings the other device's sound to it over the network:
//!
//! * *Here*: the phone sends what it's playing ("relay") and this computer plays it
//!   alongside its own sound. The system mixer plays both at once, so a game here and a
//!   song from the phone play together in the headphones connected to this computer.
//! * *On the phone*: this computer's sound goes to the phone instead (Linux: a virtual
//!   output whose monitor Devti records, so the speakers here go quiet; Windows: the
//!   output is recorded as it plays).
//!
//! **Voice messages pause the music.** Every device reports whether it's playing music
//! and whether a voice message (WhatsApp, Instagram, Telegram…) is playing. When a voice
//! message starts anywhere, the [`Coordinator`] pauses the music on the *other* devices
//! (a phone already pauses its own music for its own voice messages), and resumes exactly
//! what it paused once the voice message has been quiet for a moment.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use devti_core::model::DeviceId;
use devti_core::{Node, NodeEvent};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Where a sound comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Here,
    Peer(DeviceId),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub music: bool,
    pub voice: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Pause(Source),
    Resume(Source),
}

/// Quiet this long after a voice message before the music comes back.
pub const RESUME_AFTER: Duration = Duration::from_millis(900);
/// Right after Devti pauses something, "still playing" reports are stale.
pub const GRACE: Duration = Duration::from_millis(1500);

/// The pure decision part: what to pause and resume, given what's playing where.
#[derive(Debug, Default)]
pub struct Coordinator {
    activity: HashMap<Source, Activity>,
    /// Sources Devti paused, and when.
    paused: HashMap<Source, Instant>,
    quiet_since: Option<Instant>,
    pub enabled: bool,
}

impl Coordinator {
    pub fn new(enabled: bool) -> Self {
        Self { enabled, ..Default::default() }
    }

    /// A source reported what it's playing.
    pub fn update(&mut self, src: Source, a: Activity, now: Instant) -> Vec<Command> {
        if a.music
            && let Some(&at) = self.paused.get(&src)
            && now.duration_since(at) > GRACE
        {
            // Playing again after Devti paused it: the user resumed it by hand. Leave it be.
            self.paused.remove(&src);
        }
        self.activity.insert(src, a);
        self.step(now)
    }

    /// A source went away (disconnected).
    pub fn forget(&mut self, src: Source) {
        self.activity.remove(&src);
        self.paused.remove(&src);
    }

    /// Time passing (resumes after the quiet period).
    pub fn tick(&mut self, now: Instant) -> Vec<Command> {
        self.step(now)
    }

    pub fn waiting(&self) -> bool {
        !self.paused.is_empty()
    }

    fn step(&mut self, now: Instant) -> Vec<Command> {
        if !self.enabled {
            return self.paused.drain().map(|(s, _)| Command::Resume(s)).collect();
        }
        let voices: HashSet<Source> = self.activity.iter().filter(|(_, a)| a.voice).map(|(s, _)| *s).collect();
        if !voices.is_empty() {
            self.quiet_since = None;
            let mut out = Vec::new();
            for (src, a) in &self.activity {
                if a.music && !voices.contains(src) && !self.paused.contains_key(src) {
                    self.paused.insert(*src, now);
                    out.push(Command::Pause(*src));
                }
            }
            out.sort_by_key(|c| format!("{c:?}"));
            return out;
        }
        if self.paused.is_empty() {
            self.quiet_since = None;
            return Vec::new();
        }
        let since = *self.quiet_since.get_or_insert(now);
        if now.duration_since(since) < RESUME_AFTER {
            return Vec::new();
        }
        self.quiet_since = None;
        let mut out: Vec<Command> = self
            .paused
            .drain()
            .filter(|(s, _)| !self.activity.get(s).is_some_and(|a| a.music))
            .map(|(s, _)| Command::Resume(s))
            .collect();
        out.sort_by_key(|c| format!("{c:?}"));
        out
    }
}

/// Apps whose playback counts as a voice message.
pub fn is_messaging(name: &str) -> bool {
    let n = name.to_lowercase();
    ["whatsapp", "telegram", "signal", "instagram", "messenger", "snapchat", "viber", "wechat", "imo"]
        .iter()
        .any(|m| n.contains(m))
        || n.contains("web.whatsapp.com")
        || n.contains("instagram.com/direct")
}

// ------------------------------------------------------------------ runtime

/// Where everything plays.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Mode {
    /// Every device plays its own sound.
    Separate,
    /// The phone's sound plays here, alongside this computer's.
    Here { device: String },
    /// This computer's sound plays on the phone.
    OnPhone { device: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct SoundStatus {
    #[serde(flatten)]
    pub mode: Mode,
    pub voice_pause: bool,
    /// This computer can send its own sound to a phone.
    pub can_send: bool,
    /// What's playing where, by device id ("this" = this computer).
    pub playing: HashMap<String, (bool, bool)>,
    /// A voice message is playing and music is paused for it.
    pub paused_for_voice: bool,
    /// Why sending this computer's sound stopped, if it did.
    pub note: Option<String>,
}

pub struct Sound {
    node: Node,
    app: AppHandle,
    coord: Mutex<Coordinator>,
    mode: Mutex<Mode>,
    capture: Mutex<Option<capture::Capture>>,
    note: Mutex<Option<String>>,
    watching: AtomicBool,
}

const SETTING_VOICE_PAUSE: &str = "voice_pause";

impl Sound {
    pub fn start(app: &AppHandle, node: Node) -> Arc<Self> {
        let enabled = node.setting(SETTING_VOICE_PAUSE).ok().flatten().as_deref() != Some("false");
        let me = Arc::new(Self {
            node,
            app: app.clone(),
            coord: Mutex::new(Coordinator::new(enabled)),
            mode: Mutex::new(Mode::Separate),
            capture: Mutex::new(None),
            note: Mutex::new(None),
            watching: AtomicBool::new(false),
        });
        if enabled {
            me.clone().watch_here();
        }
        // Resume after the quiet period, even when no new report arrives.
        let m = me.clone();
        tauri::async_runtime::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_millis(250));
            loop {
                t.tick().await;
                if m.coord.lock().waiting() {
                    let cmds = m.coord.lock().tick(Instant::now());
                    m.run(cmds);
                }
            }
        });
        me
    }

    pub fn status(&self) -> SoundStatus {
        let c = self.coord.lock();
        let playing = c
            .activity
            .iter()
            .map(|(s, a)| {
                let key = match s {
                    Source::Here => "this".to_string(),
                    Source::Peer(d) => d.to_string(),
                };
                (key, (a.music, a.voice))
            })
            .collect();
        SoundStatus {
            mode: self.mode.lock().clone(),
            voice_pause: c.enabled,
            can_send: capture::supported(),
            playing,
            paused_for_voice: c.waiting(),
            note: self.note.lock().clone(),
        }
    }

    fn publish(&self) {
        let _ = self.app.emit("devti://sound", self.status());
    }

    pub fn set_voice_pause(self: &Arc<Self>, on: bool) {
        let _ = self.node.set_setting(SETTING_VOICE_PAUSE, if on { "true" } else { "false" });
        let cmds = {
            let mut c = self.coord.lock();
            c.enabled = on;
            c.tick(Instant::now())
        };
        self.run(cmds);
        if on {
            self.clone().watch_here();
        }
        self.publish();
    }

    /// Chooses where everything plays.
    pub fn set_mode(&self, mode: Mode) -> Result<(), String> {
        // Undo the current mode first.
        let prev = self.mode.lock().clone();
        match &prev {
            Mode::Here { device } => {
                if let Ok(id) = device.parse() {
                    let _ = self.node.request_audio(&id, "relay", false);
                }
            }
            Mode::OnPhone { .. } => {
                if let Some(c) = self.capture.lock().take() {
                    c.stop();
                }
            }
            Mode::Separate => {}
        }
        *self.note.lock() = None;
        match &mode {
            Mode::Here { device } => {
                let id: DeviceId = device.parse().map_err(|_| "unknown device".to_string())?;
                self.node.request_audio(&id, "relay", true).map_err(|e| e.to_string())?;
            }
            Mode::OnPhone { device } => {
                let id: DeviceId = device.parse().map_err(|_| "unknown device".to_string())?;
                let sender = self.node.start_audio(&id, "relay", 48_000, 2).map_err(|e| e.to_string())?;
                let app = self.app.clone();
                let c = capture::Capture::start(sender, move |why| {
                    if let Some(s) = app.try_state::<Arc<Sound>>() {
                        *s.note.lock() = Some(why);
                        *s.mode.lock() = Mode::Separate;
                        s.publish();
                    }
                })?;
                *self.capture.lock() = Some(c);
            }
            Mode::Separate => {}
        }
        *self.mode.lock() = mode;
        self.publish();
        Ok(())
    }

    pub fn on_node_event(self: &Arc<Self>, ev: &NodeEvent) {
        match ev {
            NodeEvent::MediaActivity { from, music, voice, .. } => {
                let cmds = self.coord.lock().update(
                    Source::Peer(*from),
                    Activity { music: *music, voice: *voice },
                    Instant::now(),
                );
                self.run(cmds);
                self.publish();
            }
            NodeEvent::PeerChanged { peer } if !peer.online => {
                self.coord.lock().forget(Source::Peer(peer.info.id));
                let gone = match &*self.mode.lock() {
                    Mode::Here { device } | Mode::OnPhone { device } => *device == peer.info.id.to_string(),
                    Mode::Separate => false,
                };
                if gone {
                    let _ = self.set_mode(Mode::Separate);
                }
            }
            NodeEvent::MediaCommandRequested { action, .. } => {
                // A phone asked this computer to pause or resume its music.
                if action == "pause" {
                    local::pause();
                } else {
                    local::resume();
                }
            }
            _ => {}
        }
    }

    fn run(&self, cmds: Vec<Command>) {
        for c in cmds {
            match c {
                Command::Pause(Source::Here) => local::pause(),
                Command::Resume(Source::Here) => local::resume(),
                Command::Pause(Source::Peer(id)) => {
                    let _ = self.node.media_command(&id, "pause");
                }
                Command::Resume(Source::Peer(id)) => {
                    let _ = self.node.media_command(&id, "play");
                }
            }
        }
    }

    /// Watches what this computer plays, about once a second, while the feature is on.
    fn watch_here(self: Arc<Self>) {
        if self.watching.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::Builder::new()
            .name("devti-sound-watch".into())
            .spawn(move || {
                let mut last = Activity::default();
                while self.coord.lock().enabled {
                    // Only worth it while another device is online.
                    if self.node.peers().iter().any(|p| p.online) {
                        let a = local::activity();
                        if a != last {
                            last = a;
                            let cmds = self.coord.lock().update(Source::Here, a, Instant::now());
                            self.run(cmds);
                            // Phones pause their music for this computer's voice messages too.
                            self.node.report_media_activity(a.music, a.voice, "");
                            self.publish();
                        }
                    }
                    std::thread::sleep(Duration::from_millis(1000));
                }
                self.watching.store(false, Ordering::SeqCst);
            })
            .ok();
    }
}

pub fn app_state(app: &AppHandle) -> Option<Arc<Sound>> {
    app.try_state::<Arc<Sound>>().map(|s| s.inner().clone())
}

// ------------------------------------------------- what plays on this computer

#[cfg(target_os = "linux")]
mod local {
    //! MPRIS for players (Spotify, browsers, VLC…), PulseAudio/PipeWire for apps that
    //! aren't MPRIS players.
    use std::collections::HashMap;
    use std::process::Command;

    use parking_lot::Mutex;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::OwnedValue;

    use super::{Activity, is_messaging};

    static PAUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn players(conn: &Connection) -> Vec<(String, String, bool)> {
        let Ok(dbus) = Proxy::new(conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus") else {
            return Vec::new();
        };
        let names: Vec<String> = dbus.call("ListNames", &()).unwrap_or_default();
        names
            .into_iter()
            .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
            .filter_map(|name| {
                let (url, playing) = {
                    let p = Proxy::new(conn, name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player")
                        .ok()?;
                    let playing = p.get_property::<String>("PlaybackStatus").ok()? == "Playing";
                    let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap_or_default();
                    let url = meta.get("xesam:url").and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
                    (url, playing)
                };
                Some((name, url, playing))
            })
            .collect()
    }

    /// Messaging apps playing through PulseAudio/PipeWire without an MPRIS player.
    fn messaging_stream_playing() -> bool {
        let Ok(out) = Command::new("pactl").args(["list", "sink-inputs"]).output() else { return false };
        let text = String::from_utf8_lossy(&out.stdout);
        text.split("Sink Input #").any(|block| {
            let corked = block.lines().any(|l| l.trim() == "Corked: yes");
            let role_phone =
                block.contains("media.role = \"phone\"") || block.contains("media.role = \"communication\"");
            let app = block
                .lines()
                .filter(|l| l.contains("application.name") || l.contains("application.process.binary"))
                .any(is_messaging);
            !corked && (app || role_phone)
        })
    }

    pub fn activity() -> Activity {
        let Ok(conn) = Connection::session() else { return Activity::default() };
        let mut a = Activity::default();
        for (name, url, playing) in players(&conn) {
            if !playing {
                continue;
            }
            if is_messaging(&name) || is_messaging(&url) {
                a.voice = true;
            } else {
                a.music = true;
            }
        }
        if !a.voice {
            a.voice = messaging_stream_playing();
        }
        a
    }

    fn call(name: &str, method: &str) {
        if let Ok(conn) = Connection::session()
            && let Ok(p) = Proxy::new(&conn, name, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player")
        {
            let _ = p.call_method(method, &());
        }
    }

    /// Pauses every music player that's playing, remembering which.
    pub fn pause() {
        let Ok(conn) = Connection::session() else { return };
        let mut paused = PAUSED.lock();
        for (name, url, playing) in players(&conn) {
            if playing && !is_messaging(&name) && !is_messaging(&url) {
                call(&name, "Pause");
                paused.push(name);
            }
        }
    }

    /// Resumes what [`pause`] paused.
    pub fn resume() {
        for name in PAUSED.lock().drain(..) {
            call(&name, "Play");
        }
    }
}

#[cfg(windows)]
mod local {
    //! Windows' media controls (SMTC): every app that shows in the volume flyout's media
    //! card — Spotify, browsers, the Media Player, WhatsApp…
    use parking_lot::Mutex;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };

    use super::{Activity, is_messaging};

    static PAUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn sessions() -> Vec<(String, bool, windows::Media::Control::GlobalSystemMediaTransportControlsSession)> {
        let Ok(manager) = Manager::RequestAsync().and_then(|op| op.join()) else { return Vec::new() };
        let Ok(list) = manager.GetSessions() else { return Vec::new() };
        list.into_iter()
            .filter_map(|s| {
                let app = s.SourceAppUserModelId().ok()?.to_string();
                let playing = s.GetPlaybackInfo().ok()?.PlaybackStatus().ok()? == Status::Playing;
                Some((app, playing, s))
            })
            .collect()
    }

    pub fn activity() -> Activity {
        let mut a = Activity::default();
        for (app, playing, _) in sessions() {
            if !playing {
                continue;
            }
            if is_messaging(&app) {
                a.voice = true;
            } else {
                a.music = true;
            }
        }
        a
    }

    pub fn pause() {
        let mut paused = PAUSED.lock();
        for (app, playing, s) in sessions() {
            if playing && !is_messaging(&app) && s.TryPauseAsync().and_then(|op| op.join()).unwrap_or(false) {
                paused.push(app);
            }
        }
    }

    pub fn resume() {
        let apps: Vec<String> = PAUSED.lock().drain(..).collect();
        for (app, _, s) in sessions() {
            if apps.contains(&app) {
                let _ = s.TryPlayAsync().and_then(|op| op.join());
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod local {
    use super::Activity;
    pub fn activity() -> Activity {
        Activity::default()
    }
    pub fn pause() {}
    pub fn resume() {}
}

// --------------------------------------------- this computer's sound to a phone

#[cfg(target_os = "linux")]
mod capture {
    //! A virtual output ("Devti to phone") becomes the default, every playing app moves
    //! to it, and its monitor is recorded with `parec` and sent to the phone. Stopping
    //! moves everything back.
    use std::io::Read;
    use std::process::{Child, Command, Stdio};

    use devti_core::node::audio::AudioSender;

    const SINK: &str = "devti_to_phone";

    pub fn supported() -> bool {
        Command::new("pactl").arg("info").output().map(|o| o.status.success()).unwrap_or(false)
    }

    pub struct Capture {
        child: Child,
        module: Option<String>,
        previous: Option<String>,
    }

    fn pactl(args: &[&str]) -> Option<String> {
        let out = Command::new("pactl").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn move_all(to: &str) {
        if let Some(list) = pactl(&["list", "short", "sink-inputs"]) {
            for id in list.lines().filter_map(|l| l.split_whitespace().next()) {
                let _ = pactl(&["move-sink-input", id, to]);
            }
        }
    }

    impl Capture {
        pub fn start(sender: AudioSender, on_stop: impl Fn(String) + Send + 'static) -> Result<Self, String> {
            if !supported() {
                return Err("needs PipeWire or PulseAudio (pactl)".into());
            }
            let previous = pactl(&["get-default-sink"]);
            let module = pactl(&[
                "load-module",
                "module-null-sink",
                &format!("sink_name={SINK}"),
                "sink_properties=device.description=Devti-to-phone",
            ]);
            let _ = pactl(&["set-default-sink", SINK]);
            move_all(SINK);
            let mut child = Command::new("parec")
                .args([
                    &format!("--device={SINK}.monitor"),
                    "--format=s16le",
                    "--rate=48000",
                    "--channels=2",
                    "--latency-msec=20",
                    "--raw",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "parec isn't installed (pulseaudio-utils)".to_string())?;
            let mut out = child.stdout.take().ok_or("no audio pipe")?;
            std::thread::Builder::new()
                .name("devti-capture".into())
                .spawn(move || {
                    // 20 ms of 48 kHz stereo s16.
                    let mut buf = vec![0u8; 3840];
                    loop {
                        if out.read_exact(&mut buf).is_err() {
                            break;
                        }
                        if !sender.push(buf.clone()) && sender.closed() {
                            on_stop("The phone stopped playing this computer's sound.".into());
                            break;
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(Self { child, module, previous })
        }

        pub fn stop(mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            if let Some(prev) = &self.previous {
                let _ = pactl(&["set-default-sink", prev]);
                move_all(prev);
            }
            if let Some(m) = &self.module {
                let _ = pactl(&["unload-module", m]);
            }
        }
    }
}

#[cfg(windows)]
mod capture {
    //! WASAPI loopback (cpal: an input stream on the output device) records what plays.
    //! Windows keeps playing it on this computer too; mute the speakers if you like.
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use devti_core::node::audio::AudioSender;

    pub fn supported() -> bool {
        cpal::default_host().default_output_device().is_some()
    }

    pub struct Capture {
        _stream: cpal::Stream,
    }

    // cpal's stream isn't Send on every platform; Devti only touches it from one place.
    unsafe impl Send for Capture {}

    impl Capture {
        pub fn start(sender: AudioSender, on_stop: impl Fn(String) + Send + 'static) -> Result<Self, String> {
            let device = cpal::default_host().default_output_device().ok_or("no audio output")?;
            let config = device.default_output_config().map_err(|e| e.to_string())?;
            let channels = config.channels() as usize;
            let mut pending: Vec<u8> = Vec::with_capacity(8192);
            let err = |e| tracing::warn!(error = %e, "loopback capture error");
            let mut stopped = false;
            let mut push = move |frames: &mut dyn Iterator<Item = [i16; 2]>| {
                for [l, r] in frames {
                    pending.extend_from_slice(&l.to_le_bytes());
                    pending.extend_from_slice(&r.to_le_bytes());
                }
                // Send in 20 ms chunks.
                while pending.len() >= 3840 {
                    let chunk: Vec<u8> = pending.drain(..3840).collect();
                    if !sender.push(chunk) && sender.closed() && !stopped {
                        stopped = true;
                        on_stop("The phone stopped playing this computer's sound.".into());
                    }
                }
            };
            let stream = match config.sample_format() {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    config.into(),
                    move |data: &[f32], _: &_| {
                        let to = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                        push(&mut data.chunks(channels).map(|f| [to(f[0]), to(*f.get(1).unwrap_or(&f[0]))]));
                    },
                    err,
                    None,
                ),
                _ => device.build_input_stream(
                    config.into(),
                    move |data: &[i16], _: &_| {
                        push(&mut data.chunks(channels).map(|f| [f[0], *f.get(1).unwrap_or(&f[0])]));
                    },
                    err,
                    None,
                ),
            }
            .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            Ok(Self { _stream: stream })
        }

        pub fn stop(self) {}
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod capture {
    use devti_core::node::audio::AudioSender;
    pub fn supported() -> bool {
        false
    }
    pub struct Capture;
    impl Capture {
        pub fn start(_s: AudioSender, _f: impl Fn(String) + Send + 'static) -> Result<Self, String> {
            Err("not supported here".into())
        }
        pub fn stop(self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_voice_message_pauses_music_elsewhere_and_resumes_it_after() {
        let phone = Source::Peer(DeviceId::from_bytes([1; 10]));
        let mut c = Coordinator::new(true);
        let t0 = Instant::now();
        assert!(c.update(Source::Here, Activity { music: true, voice: false }, t0).is_empty());
        // A WhatsApp voice note starts on the phone: the computer's music pauses.
        assert_eq!(
            c.update(phone, Activity { music: false, voice: true }, t0 + ms(100)),
            vec![Command::Pause(Source::Here)]
        );
        assert_eq!(c.update(Source::Here, Activity { music: false, voice: false }, t0 + ms(300)), vec![]);
        // The note ends; nothing until it's been quiet a moment, then exactly one resume.
        assert_eq!(c.update(phone, Activity { music: false, voice: false }, t0 + ms(5_000)), vec![]);
        assert_eq!(c.tick(t0 + ms(5_400)), vec![]);
        assert_eq!(c.tick(t0 + ms(6_000)), vec![Command::Resume(Source::Here)]);
        assert_eq!(c.tick(t0 + ms(7_000)), vec![]);
    }

    #[test]
    fn the_voice_messages_own_device_is_left_alone_and_hand_resumes_win() {
        let phone = Source::Peer(DeviceId::from_bytes([1; 10]));
        let mut c = Coordinator::new(true);
        let t0 = Instant::now();
        c.update(phone, Activity { music: true, voice: false }, t0);
        c.update(Source::Here, Activity { music: true, voice: false }, t0);
        // A voice message on the phone itself: Android pauses its music; Devti pauses ours.
        let cmds = c.update(phone, Activity { music: true, voice: true }, t0 + ms(10));
        assert_eq!(cmds, vec![Command::Pause(Source::Here)]);
        // The user starts the computer's music again by hand during the message.
        c.update(Source::Here, Activity { music: true, voice: false }, t0 + ms(3_000));
        c.update(phone, Activity { music: false, voice: false }, t0 + ms(4_000));
        assert_eq!(c.tick(t0 + ms(6_000)), vec![], "nothing to resume: it's already playing");
    }

    #[test]
    fn turning_it_off_gives_back_what_it_paused() {
        let phone = Source::Peer(DeviceId::from_bytes([1; 10]));
        let mut c = Coordinator::new(true);
        let t0 = Instant::now();
        c.update(phone, Activity { music: true, voice: false }, t0);
        assert_eq!(c.update(Source::Here, Activity { music: false, voice: true }, t0), vec![Command::Pause(phone)]);
        c.enabled = false;
        assert_eq!(c.tick(t0 + ms(10)), vec![Command::Resume(phone)]);
    }

    #[test]
    fn messaging_apps_are_recognised() {
        for a in [
            "WhatsApp",
            "org.mpris.MediaPlayer2.telegram-desktop",
            "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App",
            "https://web.whatsapp.com/",
        ] {
            assert!(is_messaging(a), "{a}");
        }
        for a in ["Spotify", "org.mpris.MediaPlayer2.vlc", "chromium.instance42"] {
            assert!(!is_messaging(a), "{a}");
        }
    }
}
