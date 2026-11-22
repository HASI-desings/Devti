//! Phase 3 on the Hub: performs what paired remotes ask, publishes what this computer
//! offers to them, and runs the local API for scripts and the `devti` CLI.
//!
//! Sections:
//! * **Input** — a dedicated thread owns the `enigo` injector (SendInput on Windows,
//!   XTest on X11, libei / virtual-keyboard protocols on Wayland) and performs
//!   `RemoteInput` events in order.
//! * **System** — lock, sleep, shut down, restart and sign out, volume and brightness, through
//!   each OS's standard tools (`loginctl`/`systemctl`/`wpctl`/`brightnessctl` on Linux,
//!   `LockWorkStation`/`shutdown`/media keys/WMI on Windows).
//! * **Apps** — installed apps from `.desktop` files (Linux) or Start-menu shortcuts
//!   (Windows); remotes can only launch entries from this list.
//! * **Saved commands and deck** — defined only here, in the GUI. Remotes see names and
//!   ids, never the command lines.
//! * **OBS** — scene switching over the OBS WebSocket v5 protocol (password auth).
//! * **Local API** — `devti_core::api`, 127.0.0.1 only, token in the data folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use devti_core::protocol::{
    Catalog, CatalogApp, CatalogCommand, DeckButton, DeckPage, InputEvent, MouseButton, SystemAction,
};
use devti_core::{Node, NodeEvent};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub const SETTING_COMMANDS: &str = "commands";
pub const SETTING_DECK: &str = "deck";
pub const SETTING_OBS_URL: &str = "obs_url";
pub const SETTING_OBS_PASSWORD: &str = "obs_password";
pub const SETTING_LOCAL_API: &str = "local_api";

/// A command line the user saved in the GUI.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedCommand {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub confirm: bool,
}

/// What a deck button does when pressed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeckAction {
    /// `hotkey`, `launch`, `command`, `media`, `text`, `url` or `obs_scene`.
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeckButtonDef {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub voice: String,
    pub action: DeckAction,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeckPageDef {
    pub name: String,
    pub buttons: Vec<DeckButtonDef>,
}

/// An installed app.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    pub exec: Option<String>,
}

/// The Hub's remote-control state.
pub struct Remote {
    node: Node,
    input: mpsc::Sender<Vec<InputEvent>>,
    apps: Mutex<Vec<AppEntry>>,
    api: Mutex<Option<devti_core::api::ApiServer>>,
    data_dir: PathBuf,
}

impl Remote {
    pub fn start(app: &AppHandle, node: Node, data_dir: PathBuf) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<Vec<InputEvent>>();
        let warn_app = app.clone();
        std::thread::Builder::new().name("devti-input".into()).spawn(move || input_thread(rx, warn_app)).ok();
        let remote = Arc::new(Self { node, input: tx, apps: Mutex::new(Vec::new()), api: Mutex::new(None), data_dir });
        let r = remote.clone();
        std::thread::Builder::new()
            .name("devti-apps".into())
            .spawn(move || {
                *r.apps.lock() = installed_apps();
                r.publish_catalog();
            })
            .ok();
        remote.publish_catalog();
        remote.apply_api_setting();
        remote
    }

    pub fn commands(&self) -> Vec<SavedCommand> {
        read_json(&self.node, SETTING_COMMANDS).unwrap_or_default()
    }

    pub fn deck(&self) -> Vec<DeckPageDef> {
        read_json(&self.node, SETTING_DECK).unwrap_or_else(default_deck)
    }

    pub fn apps(&self) -> Vec<AppEntry> {
        self.apps.lock().clone()
    }

    pub fn save_commands(&self, list: Vec<SavedCommand>) -> anyhow::Result<()> {
        let list: Vec<SavedCommand> =
            list.into_iter().filter(|c| !c.name.trim().is_empty() && !c.command.trim().is_empty()).take(100).collect();
        self.node.set_setting(SETTING_COMMANDS, &serde_json::to_string(&list)?)?;
        self.publish_catalog();
        Ok(())
    }

    pub fn save_deck(&self, pages: Vec<DeckPageDef>) -> anyhow::Result<()> {
        self.node.set_setting(SETTING_DECK, &serde_json::to_string(&pages)?)?;
        self.publish_catalog();
        Ok(())
    }

    /// Tells the core what remotes may see.
    pub fn publish_catalog(&self) {
        let apps = self.apps.lock().iter().map(|a| CatalogApp { id: a.id.clone(), name: a.name.clone() }).collect();
        let mut commands: Vec<CatalogCommand> = self
            .commands()
            .into_iter()
            .map(|c| CatalogCommand { id: c.id, name: c.name, confirm: c.confirm })
            .collect();
        // Running plugins' commands, as `plugin:<plugin>:<command>` (Phase 7).
        if let Some(p) = crate::plugins::host() {
            commands.extend(p.catalog_commands());
        }
        let deck = self
            .deck()
            .into_iter()
            .map(|p| DeckPage {
                name: p.name,
                buttons: p
                    .buttons
                    .into_iter()
                    .map(|b| DeckButton { id: b.id, label: b.label, icon: b.icon, voice: b.voice })
                    .collect(),
            })
            .collect();
        let system = ["lock", "sleep", "shutdown", "restart", "sign_out", "set_volume", "set_brightness"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        self.node.set_catalog(Catalog { apps, commands, deck, system });
    }

    /// Starts or stops the local API to match the setting (on by default).
    pub fn apply_api_setting(self: &Arc<Self>) {
        let enabled = self.node.setting(SETTING_LOCAL_API).ok().flatten().as_deref() != Some("false");
        let mut slot = self.api.lock();
        if !enabled {
            if let Some(s) = slot.take() {
                s.stop();
            }
            return;
        }
        if slot.is_some() {
            return;
        }
        let me = self.clone();
        let runner: devti_core::api::LocalRunner = Arc::new(move |id: String| {
            let me = me.clone();
            Box::pin(async move { me.run_saved(&id).await })
        });
        let node = self.node.clone();
        let dir = self.data_dir.clone();
        let started = tauri::async_runtime::block_on(async move {
            devti_core::api::start(node, &dir, devti_core::api::DEFAULT_PORT, Some(runner)).await
        });
        match started {
            Ok(s) => {
                tracing::info!(addr = %s.addr, "local API listening");
                *slot = Some(s);
            }
            Err(e) => tracing::warn!(error = %e, "local API could not start"),
        }
    }

    pub fn api_info(&self) -> serde_json::Value {
        let slot = self.api.lock();
        let token = devti_core::api::load_or_create_token(&self.data_dir).unwrap_or_default();
        serde_json::json!({
            "enabled": slot.is_some(),
            "port": slot.as_ref().map(|s| s.addr.port()),
            "token": token,
        })
    }

    pub fn reset_api_token(self: &Arc<Self>) -> anyhow::Result<String> {
        let token = devti_core::api::reset_token(&self.data_dir)?;
        // Restart so the server uses the new token.
        if let Some(s) = self.api.lock().take() {
            s.stop();
        }
        self.apply_api_setting();
        Ok(token)
    }

    /// Runs a saved command by id: `(ok, output)`.
    pub async fn run_saved(&self, id: &str) -> (bool, String) {
        if id.starts_with(crate::plugins::COMMAND_PREFIX) {
            let Some(host) = crate::plugins::host() else { return (false, "Plugins aren't ready yet.".into()) };
            let id = id.to_string();
            return match tauri::async_runtime::spawn_blocking(move || host.run_catalog_id(&id)).await {
                Ok(Some(Ok(out))) => (true, out),
                Ok(Some(Err(e))) => (false, e),
                _ => (false, "That plugin command doesn't exist.".into()),
            };
        }
        match self.commands().into_iter().find(|c| c.id == id) {
            Some(c) => run_shell(&c.command).await,
            None => (false, "That command doesn't exist any more.".into()),
        }
    }

    pub fn launch(&self, app_id: &str) -> anyhow::Result<()> {
        let app = self.apps.lock().iter().find(|a| a.id == app_id).cloned();
        let app = app.ok_or_else(|| anyhow::anyhow!("no such app"))?;
        launch_app(&app)
    }

    fn press_deck(self: &Arc<Self>, app: &AppHandle, button: &str) {
        let Some(b) = self.deck().into_iter().flat_map(|p| p.buttons).find(|b| b.id == button) else { return };
        let me = self.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = me.do_action(&b.action).await {
                let _ =
                    app.notification().builder().title(format!("“{}” didn't work", b.label)).body(e.to_string()).show();
            }
        });
    }

    /// Performs a deck action on this computer.
    pub async fn do_action(&self, a: &DeckAction) -> anyhow::Result<()> {
        match a.kind.as_str() {
            "hotkey" => {
                let (key, mods) = parse_hotkey(&a.value).ok_or_else(|| anyhow::anyhow!("unknown key combination"))?;
                self.input.send(vec![InputEvent::Key { key, mods }])?;
            }
            "text" => self.input.send(vec![InputEvent::Text { text: a.value.clone() }])?,
            "media" => self.input.send(vec![InputEvent::Media { action: a.value.clone() }])?,
            "launch" => self.launch(&a.value)?,
            "command" => {
                let (ok, out) = self.run_saved(&a.value).await;
                if !ok {
                    anyhow::bail!(if out.is_empty() { "the command failed".to_string() } else { out });
                }
            }
            "url" => {
                if !(a.value.starts_with("https://") || a.value.starts_with("http://")) {
                    anyhow::bail!("only web links can be opened");
                }
                tauri_plugin_opener::open_url(&a.value, None::<&str>)?;
            }
            "obs_scene" => {
                let url = self.node.setting(SETTING_OBS_URL)?.unwrap_or_else(|| "ws://127.0.0.1:4455".into());
                let password = self.node.setting(SETTING_OBS_PASSWORD)?.unwrap_or_default();
                obs::set_scene(&url, &password, &a.value).await?;
            }
            other => anyhow::bail!("unknown action {other}"),
        }
        Ok(())
    }
}

fn read_json<T: serde::de::DeserializeOwned>(node: &Node, key: &str) -> Option<T> {
    node.setting(key).ok().flatten().and_then(|s| serde_json::from_str(&s).ok())
}

/// A useful first deck, so the phone shows something real on day one.
fn default_deck() -> Vec<DeckPageDef> {
    let b = |id: &str, label: &str, icon: &str, voice: &str, kind: &str, value: &str| DeckButtonDef {
        id: id.into(),
        label: label.into(),
        icon: icon.into(),
        voice: voice.into(),
        action: DeckAction { kind: kind.into(), value: value.into() },
    };
    vec![DeckPageDef {
        name: "Everyday".into(),
        buttons: vec![
            b("d-play", "Play / pause", "play", "iris", "media", "play_pause"),
            b("d-prev", "Previous", "previous", "iris", "media", "previous"),
            b("d-next", "Next", "next", "iris", "media", "next"),
            b("d-mute", "Mute", "mute", "aqua", "media", "mute"),
            b("d-tab", "Switch window", "windows", "mint", "hotkey", "alt+tab"),
            b("d-desk", "Show desktop", "desktop", "mint", "hotkey", "meta+d"),
            b("d-copy", "Copy", "copy", "marigold", "hotkey", "ctrl+c"),
            b("d-paste", "Paste", "paste", "marigold", "hotkey", "ctrl+v"),
        ],
    }]
}

/// Reacts to Phase 3 node events.
pub fn on_node_event(app: &AppHandle, remote: &Arc<Remote>, ev: &NodeEvent) {
    match ev {
        NodeEvent::RemoteInput { events, .. } => {
            let _ = remote.input.send(events.clone());
        }
        NodeEvent::SystemRequested { from_name, action, .. } => {
            let action = action.clone();
            let who = from_name.clone();
            let app = app.clone();
            let input = remote.input.clone();
            std::thread::spawn(move || {
                if let Err(e) = system_action(&action, &input) {
                    let _ = app
                        .notification()
                        .builder()
                        .title(format!("{who} couldn't do that"))
                        .body(e.to_string())
                        .show();
                }
            });
        }
        NodeEvent::LaunchRequested { app: id, .. } => {
            if let Err(e) = remote.launch(id) {
                tracing::warn!(error = %e, "launch failed");
            }
        }
        NodeEvent::RunRequested { from, command, .. } => {
            let (node, remote, from, command) = (remote.node.clone(), remote.clone(), *from, command.clone());
            tauri::async_runtime::spawn(async move {
                let (ok, out) = remote.run_saved(&command).await;
                let _ = node.report_run(&from, &command, ok, &out);
            });
        }
        NodeEvent::DeckPressed { button, .. } => remote.press_deck(app, button),
        _ => {}
    }
}

// ------------------------------------------------------------------ input

fn input_thread(rx: mpsc::Receiver<Vec<InputEvent>>, app: AppHandle) {
    use enigo::{Enigo, Settings};
    let mut enigo: Option<Enigo> = None;
    let mut warned = false;
    while let Ok(batch) = rx.recv() {
        if enigo.is_none() {
            match Enigo::new(&Settings { open_prompt_to_get_permissions: false, ..Settings::default() }) {
                Ok(e) => enigo = Some(e),
                Err(e) => {
                    tracing::warn!(error = %e, "input injection unavailable");
                    if !warned {
                        warned = true;
                        let _ = app.emit("devti://notice", format!("Remote control can't move the pointer here: {e}"));
                    }
                    continue;
                }
            }
        }
        let Some(e) = enigo.as_mut() else { continue };
        for ev in batch {
            if let Err(err) = perform(e, &ev) {
                tracing::debug!(error = %err, ?ev, "input event failed");
            }
        }
    }
}

fn perform(e: &mut enigo::Enigo, ev: &InputEvent) -> Result<(), enigo::InputError> {
    use enigo::{Axis, Button, Coordinate, Direction, Keyboard, Mouse};
    let button = |b: &MouseButton| match b {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
    };
    match ev {
        InputEvent::Move { dx, dy } => e.move_mouse(dx.round() as i32, dy.round() as i32, Coordinate::Rel),
        InputEvent::MoveTo { x, y } => {
            let (w, h) = e.main_display()?;
            e.move_mouse((x.clamp(0.0, 1.0) * w as f32) as i32, (y.clamp(0.0, 1.0) * h as f32) as i32, Coordinate::Abs)
        }
        InputEvent::Button { button: b, down } => {
            e.button(button(b), if *down { Direction::Press } else { Direction::Release })
        }
        InputEvent::Click { button: b, count } => {
            for _ in 0..(*count).max(1) {
                e.button(button(b), Direction::Click)?;
            }
            Ok(())
        }
        InputEvent::Scroll { dx, dy } => {
            if dy.abs() >= 0.5 {
                e.scroll(dy.round() as i32, Axis::Vertical)?;
            }
            if dx.abs() >= 0.5 {
                e.scroll(dx.round() as i32, Axis::Horizontal)?;
            }
            Ok(())
        }
        InputEvent::Text { text } => e.text(text),
        InputEvent::Key { key, mods } => {
            let Some(k) = key_of(key) else { return Ok(()) };
            let mods: Vec<enigo::Key> = mods.iter().filter_map(|m| modifier_of(m)).collect();
            for m in &mods {
                e.key(*m, Direction::Press)?;
            }
            let r = e.key(k, Direction::Click);
            for m in mods.iter().rev() {
                e.key(*m, Direction::Release)?;
            }
            r
        }
        InputEvent::Media { action } => match media_key(action) {
            Some(k) => e.key(k, Direction::Click),
            None => Ok(()),
        },
        // Touch gestures and navigation are for phones; a computer ignores them.
        _ => Ok(()),
    }
}

fn media_key(action: &str) -> Option<enigo::Key> {
    use enigo::Key;
    Some(match action {
        "play_pause" => Key::MediaPlayPause,
        "next" => Key::MediaNextTrack,
        "previous" => Key::MediaPrevTrack,
        "stop" => Key::MediaStop,
        "volume_up" => Key::VolumeUp,
        "volume_down" => Key::VolumeDown,
        "mute" => Key::VolumeMute,
        _ => return None,
    })
}

fn modifier_of(m: &str) -> Option<enigo::Key> {
    use enigo::Key;
    Some(match m.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Key::Control,
        "alt" | "option" => Key::Alt,
        "shift" => Key::Shift,
        "meta" | "super" | "win" | "cmd" => Key::Meta,
        _ => return None,
    })
}

/// Named keys used by remotes and deck hotkeys.
pub fn key_of(name: &str) -> Option<enigo::Key> {
    use enigo::Key;
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "enter" | "return" => Key::Return,
        "backspace" => Key::Backspace,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "space" => Key::Space,
        "delete" | "del" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "page_up" | "pageup" => Key::PageUp,
        "page_down" | "pagedown" => Key::PageDown,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "print_screen" | "printscreen" => Key::PrintScr,
        "caps_lock" => Key::CapsLock,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => {
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Unicode(c.to_ascii_lowercase()),
                _ => return None,
            }
        }
    })
}

/// "ctrl+shift+t" → ("t", ["ctrl", "shift"]).
pub fn parse_hotkey(s: &str) -> Option<(String, Vec<String>)> {
    let parts: Vec<String> = s.split('+').map(|p| p.trim().to_ascii_lowercase()).filter(|p| !p.is_empty()).collect();
    let (key, mods) = parts.split_last()?;
    if key_of(key).is_none() || mods.iter().any(|m| modifier_of(m).is_none()) {
        return None;
    }
    Some((key.clone(), mods.to_vec()))
}

// ----------------------------------------------------------------- system

fn try_cmds(cands: &[&[&str]]) -> anyhow::Result<()> {
    let mut last = String::from("no tool available");
    for c in cands {
        match std::process::Command::new(c[0]).args(&c[1..]).output() {
            Ok(o) if o.status.success() => return Ok(()),
            Ok(o) => last = String::from_utf8_lossy(&o.stderr).trim().to_string(),
            Err(e) => last = format!("{}: {e}", c[0]),
        }
    }
    anyhow::bail!(last)
}

/// Performs a power action on this computer right away (used by automation rules).
pub fn system_action_now(action: &SystemAction) -> Result<(), String> {
    let (tx, _rx) = mpsc::channel();
    system_action(action, &tx).map_err(|e| e.to_string())
}

fn system_action(action: &SystemAction, input: &mpsc::Sender<Vec<InputEvent>>) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let _ = input;
        return match action {
            SystemAction::Lock => try_cmds(&[&["rundll32.exe", "user32.dll,LockWorkStation"]]),
            SystemAction::Sleep => try_cmds(&[&["rundll32.exe", "powrprof.dll,SetSuspendState", "0,1,0"]]),
            SystemAction::Shutdown => try_cmds(&[&["shutdown", "/s", "/t", "0"]]),
            SystemAction::Restart => try_cmds(&[&["shutdown", "/r", "/t", "0"]]),
            SystemAction::SignOut => try_cmds(&[&["shutdown", "/l"]]),
            SystemAction::SetVolume { percent } => {
                // Windows has no volume CLI: go to 0 with 50 key presses (2 % each), then up.
                let mut ev = vec![InputEvent::Media { action: "volume_down".into() }; 50];
                ev.extend(vec![InputEvent::Media { action: "volume_up".into() }; (*percent as usize).min(100) / 2]);
                for chunk in ev.chunks(60) {
                    input.send(chunk.to_vec())?;
                }
                Ok(())
            }
            SystemAction::SetBrightness { percent } => {
                let script = format!(
                    "(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1,{})",
                    percent.min(&100)
                );
                try_cmds(&[&["powershell", "-NoProfile", "-NonInteractive", "-Command", &script]])
                    .map_err(|_| anyhow::anyhow!("this screen's brightness can't be changed from software"))
            }
            SystemAction::Unknown => Ok(()),
        };
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = input;
        match action {
            SystemAction::Lock => try_cmds(&[
                &["loginctl", "lock-session"],
                &["xdg-screensaver", "lock"],
                &[
                    "dbus-send",
                    "--type=method_call",
                    "--dest=org.freedesktop.ScreenSaver",
                    "/ScreenSaver",
                    "org.freedesktop.ScreenSaver.Lock",
                ],
            ]),
            SystemAction::Sleep => try_cmds(&[&["systemctl", "suspend"], &["loginctl", "suspend"]]),
            SystemAction::Shutdown => try_cmds(&[&["systemctl", "poweroff"], &["loginctl", "poweroff"]]),
            SystemAction::Restart => try_cmds(&[&["systemctl", "reboot"], &["loginctl", "reboot"]]),
            SystemAction::SignOut => {
                let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
                try_cmds(&[
                    &["loginctl", "terminate-session", &session],
                    &["gnome-session-quit", "--logout", "--no-prompt"],
                ])
            }
            SystemAction::SetVolume { percent } => {
                let p = format!("{}%", percent.min(&100));
                try_cmds(&[
                    &["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", &p],
                    &["pactl", "set-sink-volume", "@DEFAULT_SINK@", &p],
                    &["amixer", "-q", "sset", "Master", &p],
                ])
            }
            SystemAction::SetBrightness { percent } => {
                let p = format!("{}%", (*percent).clamp(1, 100));
                try_cmds(&[&["brightnessctl", "-q", "set", &p], &["light", "-S", &percent.to_string()]])
                    .or_else(|_| logind_brightness(*percent))
                    .map_err(|_| anyhow::anyhow!("this screen's brightness can't be changed from software"))
            }
            SystemAction::Unknown => Ok(()),
        }
    }
}

/// systemd-logind lets the active session set its own backlight without root.
#[cfg(not(target_os = "windows"))]
fn logind_brightness(percent: u8) -> anyhow::Result<()> {
    let dir =
        std::fs::read_dir("/sys/class/backlight")?.flatten().next().ok_or_else(|| anyhow::anyhow!("no backlight"))?;
    let name = dir.file_name().to_string_lossy().to_string();
    let max: u32 = std::fs::read_to_string(dir.path().join("max_brightness"))?.trim().parse()?;
    let value = (max as u64 * percent.clamp(1, 100) as u64 / 100) as u32;
    try_cmds(&[&[
        "busctl",
        "call",
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
        "SetBrightness",
        "ssu",
        "backlight",
        &name,
        &value.to_string(),
    ]])
}

// ------------------------------------------------------------------- apps

fn installed_apps() -> Vec<AppEntry> {
    let mut apps = platform_apps();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    apps.truncate(500);
    apps
}

#[cfg(target_os = "windows")]
fn platform_apps() -> Vec<AppEntry> {
    let mut roots = Vec::new();
    if let Some(p) = std::env::var_os("ProgramData") {
        roots.push(PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(p) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    let mut out = Vec::new();
    for root in roots {
        walk(&root, 3, &mut |p| {
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
                let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let lower = name.to_lowercase();
                if lower.contains("uninstall") || lower.contains("readme") || lower.contains("help") {
                    return;
                }
                let id = p.strip_prefix(&root).map(|r| r.to_string_lossy().to_string()).unwrap_or(name.clone());
                out.push(AppEntry { id, name, path: p.to_path_buf(), exec: None });
            }
        });
    }
    out
}

#[cfg(not(target_os = "windows"))]
fn platform_apps() -> Vec<AppEntry> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    {
        dirs.push(home.join("applications"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("applications")));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    let mut out = Vec::new();
    for d in dirs {
        walk(&d, 2, &mut |p| {
            if p.extension().is_some_and(|e| e == "desktop")
                && let Some(a) = parse_desktop_file(p)
                && !out.iter().any(|x: &AppEntry| x.id == a.id)
            {
                out.push(a);
            }
        });
    }
    out
}

fn walk(dir: &Path, depth: u8, f: &mut dyn FnMut(&Path)) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                walk(&p, depth - 1, f);
            }
        } else {
            f(&p);
        }
    }
}

/// Reads the `[Desktop Entry]` group of a `.desktop` file.
pub fn parse_desktop_file(path: &Path) -> Option<AppEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_desktop_entry(&text, path)
}

pub fn parse_desktop_entry(text: &str, path: &Path) -> Option<AppEntry> {
    let mut in_entry = false;
    let (mut name, mut exec, mut kind) = (None, None, None);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "Name" => name = Some(v.trim().to_string()),
            "Exec" => exec = Some(v.trim().to_string()),
            "Type" => kind = Some(v.trim().to_string()),
            "NoDisplay" | "Hidden" if v.trim() == "true" => return None,
            _ => {}
        }
    }
    if kind.as_deref() != Some("Application") {
        return None;
    }
    let id = path.file_name()?.to_string_lossy().to_string();
    Some(AppEntry { id, name: name?, path: path.to_path_buf(), exec })
}

/// `Exec=firefox %u` → `firefox` (field codes removed, per the Desktop Entry spec).
pub fn strip_field_codes(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|t| !(t.len() == 2 && t.starts_with('%')))
        .collect::<Vec<_>>()
        .join(" ")
        .replace("%%", "%")
}

fn launch_app(app: &AppEntry) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        tauri_plugin_opener::open_path(app.path.to_string_lossy().as_ref(), None::<&str>)?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let via_gio = std::process::Command::new("gio")
            .arg("launch")
            .arg(&app.path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if via_gio.is_ok() {
            return Ok(());
        }
        let exec = app.exec.as_deref().ok_or_else(|| anyhow::anyhow!("no command to start it"))?;
        std::process::Command::new("sh").arg("-c").arg(strip_field_codes(exec)).spawn()?;
        Ok(())
    }
}

/// Runs a saved command line with a 60-second limit.
pub async fn run_shell(command: &str) -> (bool, String) {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.kill_on_drop(true);
    match tokio::time::timeout(Duration::from_secs(60), cmd.output()).await {
        Ok(Ok(o)) => {
            let mut text = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(err.trim());
            }
            let text: String = text.trim().chars().take(4000).collect();
            (o.status.success(), text)
        }
        Ok(Err(e)) => (false, format!("couldn't start it: {e}")),
        Err(_) => (false, "stopped after 60 seconds".into()),
    }
}

// -------------------------------------------------------------------- OBS

pub mod obs {
    //! Minimal OBS WebSocket v5 client: identify (with password auth) and switch scene.
    use base64::Engine;
    use futures::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use tokio_tungstenite::tungstenite::Message;

    /// `base64(sha256(base64(sha256(password + salt)) + challenge))`, per the OBS spec.
    pub fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
        let b64 = base64::engine::general_purpose::STANDARD;
        let secret = b64.encode(Sha256::digest(format!("{password}{salt}").as_bytes()));
        b64.encode(Sha256::digest(format!("{secret}{challenge}").as_bytes()))
    }

    pub async fn set_scene(url: &str, password: &str, scene: &str) -> anyhow::Result<()> {
        if !(url.starts_with("ws://127.0.0.1") || url.starts_with("ws://localhost") || url.starts_with("ws://[::1]")) {
            anyhow::bail!("OBS must be on this computer (ws://127.0.0.1:4455)");
        }
        let connect = tokio_tungstenite::connect_async(url);
        let (mut ws, _) = tokio::time::timeout(std::time::Duration::from_secs(5), connect)
            .await
            .map_err(|_| anyhow::anyhow!("OBS didn't answer — is its WebSocket server on?"))??;
        let hello = next_json(&mut ws).await?;
        let mut identify = json!({ "op": 1, "d": { "rpcVersion": 1 } });
        if let Some(auth) = hello["d"].get("authentication") {
            let (salt, challenge) = (auth["salt"].as_str().unwrap_or(""), auth["challenge"].as_str().unwrap_or(""));
            identify["d"]["authentication"] = json!(auth_string(password, salt, challenge));
        }
        ws.send(Message::Text(identify.to_string().into())).await?;
        let identified = next_json(&mut ws).await?;
        if identified["op"] != 2 {
            anyhow::bail!("OBS refused the password");
        }
        let req = json!({ "op": 6, "d": { "requestType": "SetCurrentProgramScene", "requestId": "devti",
            "requestData": { "sceneName": scene } } });
        ws.send(Message::Text(req.to_string().into())).await?;
        let res = next_json(&mut ws).await?;
        let _ = ws.close(None).await;
        if res["d"]["requestStatus"]["result"].as_bool() == Some(true) {
            Ok(())
        } else {
            anyhow::bail!(
                res["d"]["requestStatus"]["comment"].as_str().unwrap_or("OBS couldn't switch scene").to_string()
            )
        }
    }

    async fn next_json<S>(ws: &mut S) -> anyhow::Result<Value>
    where
        S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => return Ok(serde_json::from_str(t.as_str())?),
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(e.into()),
                Ok(None) => anyhow::bail!("OBS closed the connection"),
                Err(_) => anyhow::bail!("OBS didn't answer"),
            }
        }
    }
}

/// Where a screenshot or recording from a mirrored device is saved.
pub fn capture_path(node: &Node, name: &str) -> PathBuf {
    let safe = devti_core::node::safe_file_name(name);
    let dir = node.downloads_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut p = dir.join(&safe);
    let mut n = 1;
    while p.exists() {
        let stem = Path::new(&safe).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = Path::new(&safe).extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        p = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    p
}

pub fn app_state(app: &AppHandle) -> Option<Arc<Remote>> {
    app.try_state::<Arc<Remote>>().map(|s| s.inner().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_parse() {
        assert_eq!(parse_hotkey("ctrl+shift+t"), Some(("t".into(), vec!["ctrl".into(), "shift".into()])));
        assert_eq!(parse_hotkey("Alt + Tab"), Some(("tab".into(), vec!["alt".into()])));
        assert_eq!(parse_hotkey("f5"), Some(("f5".into(), vec![])));
        assert_eq!(parse_hotkey("ctrl+nope"), None);
        assert_eq!(parse_hotkey("hyper+a"), None);
        assert_eq!(parse_hotkey(""), None);
    }

    #[test]
    fn desktop_entries_parse() {
        let p = Path::new("/usr/share/applications/firefox.desktop");
        let a = parse_desktop_entry("[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n", p).unwrap();
        assert_eq!((a.id.as_str(), a.name.as_str()), ("firefox.desktop", "Firefox"));
        assert_eq!(strip_field_codes(a.exec.as_deref().unwrap()), "firefox");
        assert!(parse_desktop_entry("[Desktop Entry]\nType=Application\nName=X\nNoDisplay=true\n", p).is_none());
        assert!(parse_desktop_entry("[Desktop Entry]\nType=Link\nName=X\n", p).is_none());
        // Keys in other groups (actions) don't override the main entry.
        let b = parse_desktop_entry(
            "[Desktop Entry]\nType=Application\nName=Files\n[Desktop Action new]\nName=New window\n",
            p,
        )
        .unwrap();
        assert_eq!(b.name, "Files");
    }

    #[test]
    fn obs_auth_matches_the_spec_example() {
        // Worked example from the obs-websocket v5 protocol documentation.
        assert_eq!(
            obs::auth_string(
                "supersecretpassword",
                "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
                "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="
            ),
            "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
        );
    }

    #[tokio::test]
    async fn saved_commands_run_with_output() {
        let (ok, out) = run_shell("echo hello").await;
        assert!(ok);
        assert_eq!(out, "hello");
        let (ok, _) = run_shell("exit 3").await;
        assert!(!ok);
    }
}
