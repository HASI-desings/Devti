//! Phase 6 automation on the Hub: runs the user's rules (`devti_core::rules`) against
//! what happens in the ecosystem and performs their actions here.
//!
//! Signal sources, each started only when a rule needs it:
//! * node events — devices connecting, disconnecting, battery levels;
//! * the accessory manager — accessories connecting;
//! * a minute clock (local time);
//! * a process watcher (an app started);
//! * a network watcher (Wi-Fi name via NetworkManager or netsh);
//! * a BLE scanner for the phone's rotating proximity beacon (near / away).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Local, Timelike};
use devti_core::model::DeviceId;
use devti_core::net::now_ms;
use devti_core::protocol::SystemAction;
use devti_core::rules::{Action, Engine, Proximity, Rule, Signal, Trigger, default_rules};
use devti_core::{Node, NodeEvent};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_notification::NotificationExt;

pub const SETTING_RULES: &str = "rules";

#[derive(Clone, Debug, Serialize)]
pub struct RunLog {
    pub rule: String,
    pub name: String,
    pub ts: i64,
    pub ok: bool,
    pub detail: String,
}

pub struct Automation {
    node: Node,
    app: AppHandle,
    engine: Mutex<Engine>,
    log: Mutex<VecDeque<RunLog>>,
    online: Mutex<HashSet<DeviceId>>,
    accessories_connected: Mutex<HashSet<String>>,
    proximity: Mutex<HashMap<DeviceId, Proximity>>,
    network: Mutex<Option<String>>,
}

impl Automation {
    pub fn start(app: &AppHandle, node: Node) -> Arc<Self> {
        let rules: Vec<Rule> = node
            .setting(SETTING_RULES)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(default_rules);
        let online = node.peers().into_iter().filter(|p| p.online).map(|p| p.info.id).collect();
        let me = Arc::new(Self {
            node,
            app: app.clone(),
            engine: Mutex::new(Engine::new(rules)),
            log: Mutex::new(VecDeque::new()),
            online: Mutex::new(online),
            accessories_connected: Mutex::new(HashSet::new()),
            proximity: Mutex::new(HashMap::new()),
            network: Mutex::new(None),
        });
        // Accessories: connected transitions from the accessory manager's updates.
        let m = me.clone();
        app.listen("devti://accessories", move |e| {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(e.payload()) else { return };
            let now: HashSet<String> = v["list"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| a["connected"].as_bool() == Some(true))
                .filter_map(|a| a["name"].as_str().map(str::to_string))
                .collect();
            let new: Vec<String> = {
                let mut prev = m.accessories_connected.lock();
                let new = now.difference(&prev).cloned().collect();
                *prev = now;
                new
            };
            for name in new {
                m.signal(Signal::AccessoryConnected { name });
            }
        });
        // The minute clock, processes and network.
        let m = me.clone();
        std::thread::Builder::new().name("devti-automation".into()).spawn(move || m.watch()).ok();
        // Proximity, when a rule needs it.
        let m = me.clone();
        tauri::async_runtime::spawn(async move { proximity::run(m).await });
        me
    }

    pub fn rules(&self) -> Vec<Rule> {
        self.engine.lock().rules.clone()
    }

    pub fn log(&self) -> Vec<RunLog> {
        self.log.lock().iter().cloned().collect()
    }

    pub fn proximity_state(&self) -> HashMap<String, bool> {
        self.proximity.lock().iter().filter_map(|(d, p)| p.is_near().map(|n| (d.to_string(), n))).collect()
    }

    pub fn save(&self, rules: Vec<Rule>) -> anyhow::Result<()> {
        self.node.set_setting(SETTING_RULES, &serde_json::to_string(&rules)?)?;
        self.engine.lock().rules = rules;
        Ok(())
    }

    fn uses(&self, f: impl Fn(&Trigger) -> bool) -> bool {
        self.engine.lock().rules.iter().any(|r| r.enabled && f(&r.trigger))
    }

    /// Feeds one signal; performs what fires.
    pub fn signal(&self, s: Signal) {
        let node = self.node.clone();
        let names = move |id: &str| {
            id.parse::<DeviceId>().ok().and_then(|d| node.peer(&d)).map(|p| p.info.name).unwrap_or_default()
        };
        let fired = self.engine.lock().signal(&s, now_ms(), &names);
        for (id, actions) in fired {
            let name = self.engine.lock().rules.iter().find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or_default();
            let mut ok = true;
            let mut detail = Vec::new();
            for a in &actions {
                if let Err(e) = self.perform(a) {
                    ok = false;
                    detail.push(e);
                }
            }
            let entry = RunLog { rule: id, name, ts: now_ms(), ok, detail: detail.join("; ") };
            let mut log = self.log.lock();
            log.push_front(entry);
            log.truncate(50);
            drop(log);
            let _ = self.app.emit("devti://automation", ());
        }
    }

    /// Runs a rule's actions now (the "Try it" button).
    pub fn run_now(&self, id: &str) -> Result<(), String> {
        let actions =
            self.engine.lock().rules.iter().find(|r| r.id == id).map(|r| r.actions.clone()).ok_or("no such rule")?;
        for a in &actions {
            self.perform(a)?;
        }
        Ok(())
    }

    fn perform(&self, a: &Action) -> Result<(), String> {
        match a {
            Action::Notify { title, body } => {
                self.app.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
            }
            Action::LockComputer => crate::remote::system_action_now(&SystemAction::Lock),
            Action::SetDnd { on } => {
                self.node.set_dnd(*on);
                Ok(())
            }
            Action::RunCommand { command } => {
                let r = crate::remote::app_state(&self.app).ok_or("remote control isn't ready")?;
                // Rules fire from async tasks too; blocking there would panic, so the command
                // runs to completion on its own short-lived thread.
                let (ok, out) = std::thread::scope(|s| {
                    s.spawn(|| tauri::async_runtime::block_on(r.run_saved(command)))
                        .join()
                        .unwrap_or((false, "the command crashed".into()))
                });
                if ok { Ok(()) } else { Err(out) }
            }
            Action::LaunchApp { app } => crate::remote::app_state(&self.app)
                .ok_or("remote control isn't ready")?
                .launch(app)
                .map_err(|e| e.to_string()),
            Action::SwitchAudioOutput { output } => audio_output(output),
            Action::AccessoryControl { accessory, control, value } => {
                let acc = crate::accessories::app_state(&self.app).ok_or("Bluetooth isn't ready")?;
                let target = acc
                    .list()
                    .into_iter()
                    .find(|a| a.address == *accessory || a.name.to_lowercase().contains(&accessory.to_lowercase()))
                    .ok_or("that accessory isn't paired")?;
                acc.set(&target.address, control, value)
            }
            Action::RingDevice { device } => {
                let id = self
                    .node
                    .peers()
                    .into_iter()
                    .find(|p| p.info.id.to_string() == *device || p.info.name.eq_ignore_ascii_case(device))
                    .map(|p| p.info.id)
                    .ok_or("no such device")?;
                self.node.ring(&id, true).map_err(|e| e.to_string())
            }
        }
    }

    pub fn on_node_event(&self, ev: &NodeEvent) {
        if let NodeEvent::PeerChanged { peer } = ev {
            let id = peer.info.id;
            let was = self.online.lock().contains(&id);
            if peer.online && !was {
                self.online.lock().insert(id);
                self.signal(Signal::DeviceConnected { device: id.to_string(), name: peer.info.name.clone() });
            } else if !peer.online && was {
                self.online.lock().remove(&id);
                self.signal(Signal::DeviceDisconnected { device: id.to_string(), name: peer.info.name.clone() });
            }
            if let Some(b) = peer.status.battery {
                self.signal(Signal::Battery { device: id.to_string(), percent: b.percent, charging: b.charging });
            }
        }
    }

    fn watch(self: Arc<Self>) {
        let mut last_minute = None;
        let mut known: Option<HashSet<String>> = None;
        let mut sys = sysinfo::System::new();
        let mut ticks = 0u64;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            ticks += 1;
            let now = Local::now();
            let minute = (now.hour(), now.minute());
            if last_minute != Some(minute) {
                last_minute = Some(minute);
                self.signal(Signal::Minute {
                    hour: now.hour() as u8,
                    minute: now.minute() as u8,
                    weekday: now.weekday().number_from_monday() as u8,
                });
            }
            if self.uses(|t| matches!(t, Trigger::AppOpened { .. })) {
                sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
                let names: HashSet<String> = sys
                    .processes()
                    .values()
                    .map(|p| p.name().to_string_lossy().trim_end_matches(".exe").to_lowercase())
                    .collect();
                if let Some(prev) = &known {
                    for n in names.difference(prev) {
                        self.signal(Signal::AppStarted { app: n.clone() });
                    }
                }
                known = Some(names);
            } else {
                known = None;
            }
            if ticks % 2 == 0 && self.uses(|t| matches!(t, Trigger::NetworkChanged { .. })) {
                let net = wifi_name();
                let mut prev = self.network.lock();
                if net.is_some() && *prev != net {
                    let first = prev.is_none();
                    *prev = net.clone();
                    drop(prev);
                    if !first {
                        self.signal(Signal::NetworkChanged { network: net.unwrap_or_default() });
                    }
                }
            }
        }
    }
}

/// The current Wi-Fi network name, if the OS tells us.
fn wifi_name() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("netsh").args(["wlan", "show", "interfaces"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find(|l| l.trim_start().starts_with("SSID") && !l.contains("BSSID"))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let out = std::process::Command::new("nmcli").args(["-t", "-f", "ACTIVE,SSID", "dev", "wifi"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).lines().find_map(|l| l.strip_prefix("yes:").map(str::to_string))
    }
}

/// Makes the output whose name contains `want` the default (empty = the newest
/// Bluetooth output, i.e. the earbuds that just connected). Linux: PipeWire/PulseAudio.
fn audio_output(want: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let _ = want;
        Err("Windows has no tool to switch the default output yet; switch it from the sound menu.".into())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let out = std::process::Command::new("pactl")
            .args(["list", "short", "sinks"])
            .output()
            .map_err(|_| "pactl isn't installed")?;
        let sinks: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split('\t').nth(1).map(str::to_string))
            .collect();
        let pick = if want.is_empty() {
            sinks.iter().rev().find(|s| s.starts_with("bluez_output") || s.starts_with("bluez_sink"))
        } else {
            sinks.iter().find(|s| s.to_lowercase().contains(&want.to_lowercase()))
        }
        .ok_or("no matching audio output")?;
        let ok = std::process::Command::new("pactl")
            .args(["set-default-sink", pick])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok { Ok(()) } else { Err("couldn't switch the audio output".into()) }
    }
}

mod proximity {
    use std::sync::Arc;
    use std::time::Duration;

    use btleplug::api::{Central, CentralEvent, Manager as _, ScanFilter};
    use btleplug::platform::Manager;
    use devti_core::net::now_ms;
    use devti_core::rules::{PROXIMITY_SERVICE_UUID, Signal, Trigger};
    use futures::StreamExt;

    use super::Automation;

    /// Scans for the phone's beacon while a proximity rule is on.
    pub async fn run(a: Arc<Automation>) {
        let uuid: uuid::Uuid = PROXIMITY_SERVICE_UUID.parse().expect("valid UUID");
        loop {
            while !a.uses(|t| matches!(t, Trigger::ProximityAway { .. } | Trigger::ProximityNear { .. })) {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            let Ok(manager) = Manager::new().await else { return };
            let Some(adapter) = manager.adapters().await.ok().and_then(|v| v.into_iter().next()) else { return };
            let Ok(mut events) = adapter.events().await else { return };
            // No service filter: the phone's beacon carries the UUID as service data only (a
            // service-UUID list would not fit beside it in one advert), and filters match
            // service UUIDs. Only addresses seen with the beacon are re-read on updates.
            let _ = adapter.start_scan(ScanFilter::default()).await;
            let mut beacons = std::collections::HashSet::new();
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    ev = events.next() => {
                        let id = match ev {
                            Some(CentralEvent::ServiceDataAdvertisement { id, .. } | CentralEvent::DeviceDiscovered(id)) => id,
                            Some(CentralEvent::DeviceUpdated(id)) if beacons.contains(&id) => id,
                            Some(_) => continue,
                            None => break,
                        };
                        let props = {
                            use btleplug::api::Peripheral as _;
                            match adapter.peripheral(&id).await {
                                Ok(p) => p.properties().await.ok().flatten(),
                                Err(_) => None,
                            }
                        };
                        let Some(props) = props else { continue };
                        let Some(token) = props.service_data.get(&uuid) else { continue };
                        beacons.insert(id);
                        let Some(rssi) = props.rssi else { continue };
                        for peer in a.node.peers() {
                            if a.node.proximity_tokens_of(&peer.info.id).iter().any(|t| t.as_slice() == token.as_slice()) {
                                let changed = a.proximity.lock().entry(peer.info.id).or_default().reading(rssi, now_ms());
                                if let Some(near) = changed {
                                    a.signal(Signal::Proximity { device: peer.info.id.to_string(), near });
                                }
                            }
                        }
                    }
                    _ = tick.tick() => {
                        let changes: Vec<_> = a.proximity.lock().iter_mut().filter_map(|(d, p)| p.tick(now_ms()).map(|n| (*d, n))).collect();
                        for (d, near) in changes {
                            a.signal(Signal::Proximity { device: d.to_string(), near });
                        }
                        if !a.uses(|t| matches!(t, Trigger::ProximityAway { .. } | Trigger::ProximityNear { .. })) {
                            let _ = adapter.stop_scan().await;
                            break;
                        }
                    }
                }
            }
        }
    }
}

pub fn app_state(app: &AppHandle) -> Option<Arc<Automation>> {
    app.try_state::<Arc<Automation>>().map(|s| s.inner().clone())
}
