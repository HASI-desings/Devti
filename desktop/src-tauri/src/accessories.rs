//! Phase 5 on the Hub: Bluetooth accessories as members of the ecosystem.
//!
//! For each paired accessory the Hub:
//! 1. reads what standard Bluetooth tells it (name, kind, services, battery, signal);
//! 2. matches a device profile from the library (`devti_core::accessory`);
//! 3. when the profile has a vendor channel (L2CAP, RFCOMM or GATT) and the accessory is
//!    connected, opens it, sends the handshake, re-applies remembered settings and keeps
//!    reading state and per-bud batteries;
//! 4. publishes everything to the UI, which builds the control panel from the profile.
//!
//! Linux uses BlueZ over D-Bus (`bluer`): RFCOMM through BlueZ's Profile API, L2CAP
//! sockets, GATT. Elsewhere Devti uses BLE through `btleplug`, so profiles with a GATT
//! channel and the standard battery work; Classic-only vendor channels say plainly that
//! they need a Linux Hub for now.

use devti_core::Node;
use devti_core::accessory::{BatteryLevel, Control, GattChar, Profile, Seen, builtin_profiles};
use devti_core::net::now_ms;
use devti_core::store::BatterySample;
use parking_lot::RwLock;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Serialize)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub verified: bool,
    pub notes: String,
    pub controls: Vec<Control>,
    pub has_channel: bool,
}

impl From<&Profile> for ProfileSummary {
    fn from(p: &Profile) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            vendor: p.vendor.clone(),
            verified: p.verified,
            notes: p.notes.clone(),
            controls: p.controls.clone(),
            has_channel: p.transport.is_some(),
        }
    }
}

/// One accessory as the UI shows it.
#[derive(Clone, Debug, Serialize)]
pub struct Accessory {
    pub address: String,
    pub name: String,
    /// headphones, speaker, keyboard, mouse, gamepad, watch, other.
    pub kind: String,
    pub paired: bool,
    pub connected: bool,
    pub rssi: Option<i16>,
    pub battery: BTreeMap<String, BatteryLevel>,
    pub profile: ProfileSummary,
    pub values: BTreeMap<String, String>,
    /// idle, connecting, connected, or a sentence saying why the vendor channel isn't open.
    pub session: String,
    pub services: Vec<String>,
    pub manufacturer_ids: Vec<u16>,
    pub drops: u32,
    pub last_connected: Option<i64>,
    pub remember: bool,
}

enum Cmd {
    Send(Vec<u8>),
}

pub struct Accessories {
    app: AppHandle,
    node: Node,
    list: RwLock<BTreeMap<String, Accessory>>,
    sessions: RwLock<HashMap<String, mpsc::UnboundedSender<Cmd>>>,
    pub status: RwLock<String>,
}

const KEY_REMEMBER: &str = "remember";
const KEY_VALUES: &str = "values";
const KEY_DROPS: &str = "drops";

fn kind_of(icon: &str, name: &str) -> String {
    let n = name.to_lowercase();
    let k = if icon.starts_with("audio-headset")
        || icon.starts_with("audio-headphones")
        || n.contains("buds")
        || n.contains("airpods")
        || n.contains("wh-")
        || n.contains("wf-")
    {
        "headphones"
    } else if icon.starts_with("audio") || n.contains("speaker") || n.starts_with("jbl") {
        "speaker"
    } else if icon == "input-keyboard" {
        "keyboard"
    } else if icon == "input-mouse" || icon == "input-tablet" {
        "mouse"
    } else if icon == "input-gaming" {
        "gamepad"
    } else if n.contains("watch") || n.contains("band") {
        "watch"
    } else {
        "other"
    };
    k.to_string()
}

impl Accessories {
    pub fn start(app: &AppHandle, node: Node) -> Arc<Self> {
        let me = Arc::new(Self {
            app: app.clone(),
            node,
            list: RwLock::new(BTreeMap::new()),
            sessions: RwLock::new(HashMap::new()),
            status: RwLock::new("Looking for Bluetooth…".into()),
        });
        let m = me.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = platform::run(m.clone()).await {
                *m.status.write() = e;
                m.publish();
            }
        });
        me
    }

    pub fn list(&self) -> Vec<Accessory> {
        self.list.read().values().cloned().collect()
    }

    fn setting(&self, addr: &str, key: &str) -> Option<String> {
        self.node.setting(&format!("acc:{addr}:{key}")).ok().flatten()
    }

    fn set_setting(&self, addr: &str, key: &str, value: &str) {
        let _ = self.node.set_setting(&format!("acc:{addr}:{key}"), value);
    }

    fn remembered(&self, addr: &str) -> BTreeMap<String, String> {
        self.setting(addr, KEY_VALUES).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
    }

    pub fn publish(&self) {
        let _ = self
            .app
            .emit("devti://accessories", serde_json::json!({ "status": *self.status.read(), "list": self.list() }));
    }

    /// Changes a control: sends it to the accessory and remembers it.
    pub fn set(&self, addr: &str, control: &str, value: &str) -> Result<(), String> {
        let profile = self.profile_of(addr).ok_or("unknown accessory")?;
        let payload = profile.payload_for(control, value)?;
        let tx = self.sessions.read().get(addr).cloned().ok_or("the accessory isn't connected to Devti")?;
        tx.send(Cmd::Send(payload)).map_err(|_| "the connection just closed".to_string())?;
        let mut values = self.remembered(addr);
        values.insert(control.to_string(), value.to_string());
        self.set_setting(addr, KEY_VALUES, &serde_json::to_string(&values).unwrap_or_default());
        if let Some(a) = self.list.write().get_mut(addr) {
            a.values.insert(control.to_string(), value.to_string());
        }
        self.publish();
        Ok(())
    }

    pub fn set_remember(&self, addr: &str, on: bool) {
        self.set_setting(addr, KEY_REMEMBER, if on { "true" } else { "false" });
        if let Some(a) = self.list.write().get_mut(addr) {
            a.remember = on;
        }
        self.publish();
    }

    fn profile_of(&self, addr: &str) -> Option<Profile> {
        let id = self.list.read().get(addr).map(|a| a.profile.id.clone())?;
        builtin_profiles().iter().find(|p| p.id == id).cloned()
    }

    /// Updates (or adds) an accessory from what the radio layer just read.
    fn observe(&self, mut a: Accessory) {
        let previous = self.list.read().get(&a.address).cloned();
        a.drops = self.setting(&a.address, KEY_DROPS).and_then(|v| v.parse().ok()).unwrap_or(0);
        if let Some(prev) = &previous {
            if prev.connected && !a.connected {
                a.drops += 1;
                self.set_setting(&a.address, KEY_DROPS, &a.drops.to_string());
            }
            // Keep what the vendor channel told us.
            if a.connected {
                for (k, v) in &prev.values {
                    a.values.entry(k.clone()).or_insert_with(|| v.clone());
                }
                for (k, v) in &prev.battery {
                    a.battery.entry(k.clone()).or_insert(*v);
                }
                a.session = prev.session.clone();
            }
            a.last_connected = if a.connected { Some(now_ms()) } else { prev.last_connected };
        } else if a.connected {
            a.last_connected = Some(now_ms());
        }
        a.remember = self.setting(&a.address, KEY_REMEMBER).as_deref() != Some("false");
        if !a.connected {
            a.session = "idle".into();
        }
        self.record_battery(&a);
        let changed =
            previous.as_ref().map(|p| serde_json::to_string(p).ok() != serde_json::to_string(&a).ok()).unwrap_or(true);
        self.list.write().insert(a.address.clone(), a);
        if changed {
            self.publish();
        }
    }

    /// History uses the lowest component (the bud that runs out first).
    fn record_battery(&self, a: &Accessory) {
        let level = a.battery.iter().filter(|(k, _)| k.as_str() != "case").map(|(_, v)| *v).min_by_key(|b| b.percent);
        if let Some(b) = level {
            let _ = self.node.record_accessory_battery(
                &a.address,
                BatterySample { ts: now_ms(), percent: b.percent, charging: b.charging },
            );
        }
    }

    fn session_state(&self, addr: &str, state: &str) {
        if let Some(a) = self.list.write().get_mut(addr) {
            a.session = state.to_string();
        }
        self.publish();
    }

    /// Applies what one decoded payload says.
    fn apply_payload(&self, addr: &str, profile: &Profile, payload: &[u8]) {
        let obs = profile.read(payload);
        if obs.values.is_empty() && obs.battery.is_empty() {
            return;
        }
        let snapshot = {
            let mut list = self.list.write();
            let Some(a) = list.get_mut(addr) else { return };
            a.values.extend(obs.values);
            a.battery.extend(obs.battery);
            a.clone()
        };
        self.record_battery(&snapshot);
        self.publish();
    }
}

pub fn app_state(app: &AppHandle) -> Option<Arc<Accessories>> {
    app.try_state::<Arc<Accessories>>().map(|s| s.inner().clone())
}

/// Writes a draft profile for an accessory into the received-files folder.
pub fn export_draft(node: &Node, seen: &Seen, gatt: &[GattChar]) -> Result<String, String> {
    let yaml = devti_core::accessory::draft_profile(seen, gatt);
    let name = format!("devti-profile-{}.yaml", devti_core::node::safe_file_name(&seen.name).replace(' ', "-"));
    let path = crate::remote::capture_path(node, &name);
    std::fs::write(&path, yaml).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into())
}

fn seen_of(a: &Accessory) -> Seen {
    Seen { name: a.name.clone(), manufacturer_ids: a.manufacturer_ids.clone(), services: a.services.clone() }
}

// ------------------------------------------------------------- Linux / BlueZ

#[cfg(target_os = "linux")]
mod platform {
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;

    use bluer::{Address, AddressType, Device, Session, Uuid};
    use devti_core::accessory::codec::Framer;
    use devti_core::accessory::{GattChar, Profile, Transport, match_profile};
    use futures::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc;

    use super::{Accessories, Accessory, Cmd, ProfileSummary, kind_of};

    pub async fn run(acc: Arc<Accessories>) -> Result<(), String> {
        let session =
            Session::new().await.map_err(|_| "Bluetooth isn't available (BlueZ isn't running).".to_string())?;
        let adapter = session.default_adapter().await.map_err(|_| "No Bluetooth adapter found.".to_string())?;
        *acc.status.write() = format!("Using {}", adapter.name());
        loop {
            if !adapter.is_powered().await.unwrap_or(false) {
                *acc.status.write() = "Bluetooth is turned off.".into();
                acc.publish();
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            *acc.status.write() = String::new();
            for addr in adapter.device_addresses().await.unwrap_or_default() {
                let Ok(dev) = adapter.device(addr) else { continue };
                let Some(a) = read_device(&dev).await else { continue };
                let connected = a.connected;
                let has_channel = a.profile.has_channel;
                acc.observe(a);
                let key = addr.to_string();
                if connected && has_channel && !acc.sessions.read().contains_key(&key) {
                    let (tx, rx) = mpsc::unbounded_channel();
                    acc.sessions.write().insert(key.clone(), tx);
                    let (acc2, session2) = (acc.clone(), session.clone());
                    tokio::spawn(async move {
                        let result = run_session(&acc2, &session2, dev, rx).await;
                        acc2.sessions.write().remove(&key);
                        acc2.session_state(
                            &key,
                            &match result {
                                Ok(()) => "idle".to_string(),
                                Err(e) => e,
                            },
                        );
                    });
                }
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }

    async fn read_device(dev: &Device) -> Option<Accessory> {
        let paired = dev.is_paired().await.unwrap_or(false);
        let connected = dev.is_connected().await.unwrap_or(false);
        if !paired && !connected {
            return None;
        }
        let icon = dev.icon().await.ok().flatten().unwrap_or_default();
        // Phones and computers pair over Bluetooth too; those are Devti devices, not accessories.
        if icon.starts_with("phone") || icon.starts_with("computer") {
            return None;
        }
        let name = match dev.alias().await {
            Ok(a) if !a.is_empty() => a,
            _ => dev.name().await.ok().flatten().unwrap_or_else(|| dev.address().to_string()),
        };
        let services: Vec<String> =
            dev.uuids().await.ok().flatten().unwrap_or_default().into_iter().map(|u| u.to_string()).collect();
        let manufacturer_ids: Vec<u16> =
            dev.manufacturer_data().await.ok().flatten().map(|m| m.keys().copied().collect()).unwrap_or_default();
        let mut battery = std::collections::BTreeMap::new();
        if let Ok(Some(p)) = dev.battery_percentage().await {
            battery.insert(
                "battery".to_string(),
                devti_core::accessory::BatteryLevel { percent: p.min(100), charging: false },
            );
        }
        let seen = devti_core::accessory::Seen {
            name: name.clone(),
            manufacturer_ids: manufacturer_ids.clone(),
            services: services.clone(),
        };
        let profile = match_profile(&seen, &[]);
        Some(Accessory {
            address: dev.address().to_string(),
            kind: kind_of(&icon, &name),
            name,
            paired,
            connected,
            rssi: dev.rssi().await.ok().flatten(),
            battery,
            profile: ProfileSummary::from(profile),
            values: Default::default(),
            session: if profile.transport.is_some() { "connecting".into() } else { "not needed".into() },
            services,
            manufacturer_ids,
            drops: 0,
            last_connected: None,
            remember: true,
        })
    }

    enum Channel {
        L2cap(bluer::l2cap::SeqPacket),
        Stream(bluer::rfcomm::Stream),
        Gatt {
            write: bluer::gatt::remote::Characteristic,
            notify: std::pin::Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>>,
        },
    }

    async fn open(session: &bluer::Session, dev: &Device, profile: &Profile) -> Result<Channel, String> {
        match profile.transport.as_ref().ok_or("no vendor channel")? {
            Transport::L2cap { psm } => {
                let addr = dev.address();
                let sa = bluer::l2cap::SocketAddr::new(addr, AddressType::BrEdr, *psm);
                let sock = tokio::time::timeout(Duration::from_secs(8), bluer::l2cap::SeqPacket::connect(sa))
                    .await
                    .map_err(|_| "the accessory didn't answer".to_string())?
                    .map_err(|e| format!("couldn't open its control channel ({e})"))?;
                Ok(Channel::L2cap(sock))
            }
            Transport::Rfcomm { uuid } => {
                let uuid = Uuid::from_str(uuid).map_err(|_| "bad profile UUID")?;
                let mut handle = session
                    .register_profile(bluer::rfcomm::Profile {
                        uuid,
                        role: Some(bluer::rfcomm::Role::Client),
                        require_authentication: Some(false),
                        require_authorization: Some(false),
                        auto_connect: Some(false),
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| format!("BlueZ refused the profile ({e})"))?;
                let connect = dev.connect_profile(&uuid);
                let accept = async {
                    let req = handle.next().await.ok_or("no connection")?;
                    req.accept().map_err(|e| e.to_string())
                };
                let (c, stream) =
                    tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(connect, accept) })
                        .await
                        .map_err(|_| "the accessory didn't answer".to_string())?;
                c.map_err(|e| format!("couldn't open its control channel ({e})"))?;
                Ok(Channel::Stream(stream?))
            }
            Transport::Gatt { service, write, notify } => {
                let (svc, w, n) = (norm(service), norm(write), norm(notify));
                let mut wc = None;
                let mut nc = None;
                for s in dev.services().await.map_err(|e| e.to_string())? {
                    if s.uuid().await.map(|u| u.to_string()) != Ok(svc.clone()) {
                        continue;
                    }
                    for c in s.characteristics().await.map_err(|e| e.to_string())? {
                        let u = c.uuid().await.map(|u| u.to_string()).unwrap_or_default();
                        if u == w {
                            wc = Some(c.clone());
                        }
                        if u == n {
                            nc = Some(c);
                        }
                    }
                }
                let (Some(write), Some(notify)) = (wc, nc) else {
                    return Err("the accessory's GATT layout doesn't match this profile".into());
                };
                let stream = notify.notify().await.map_err(|e| e.to_string())?;
                Ok(Channel::Gatt { write, notify: Box::pin(stream) })
            }
        }
    }

    fn norm(u: &str) -> String {
        let u = u.trim().to_lowercase();
        if u.len() == 4 { format!("0000{u}-0000-1000-8000-00805f9b34fb") } else { u }
    }

    async fn run_session(
        acc: &Accessories,
        session: &bluer::Session,
        dev: Device,
        mut rx: mpsc::UnboundedReceiver<Cmd>,
    ) -> Result<(), String> {
        let addr = dev.address().to_string();
        let profile = acc.profile_of(&addr).ok_or("unknown accessory")?;
        acc.session_state(&addr, "connecting");
        let mut ch = open(session, &dev, &profile).await?;
        let mut framer = Framer::new(profile.framing);
        acc.session_state(&addr, "connected");

        let mut outbox: Vec<Vec<u8>> = Vec::new();
        for t in &profile.init {
            if let Ok(p) = devti_core::accessory::codec::fill(t, &[]) {
                outbox.push(p);
            }
        }
        if acc.setting(&addr, super::KEY_REMEMBER).as_deref() != Some("false") {
            for (control, value) in acc.remembered(&addr) {
                if let Ok(p) = profile.payload_for(&control, &value) {
                    outbox.push(p);
                }
            }
        }
        for p in outbox {
            send(&mut ch, &framer.encode(&p)).await?;
            tokio::time::sleep(Duration::from_millis(60)).await;
        }

        let mut buf = vec![0u8; 4096];
        loop {
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    Some(Cmd::Send(p)) => send(&mut ch, &framer.encode(&p)).await?,
                    None => return Ok(()),
                },
                got = recv(&mut ch, &mut buf) => {
                    let bytes = got?;
                    if bytes.is_empty() {
                        return Err("the accessory closed its control channel".into());
                    }
                    for payload in framer.decode(&bytes) {
                        acc.apply_payload(&addr, &profile, &payload);
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(2)) => framer.reset(),
            }
            if !dev.is_connected().await.unwrap_or(false) {
                return Ok(());
            }
        }
    }

    async fn send(ch: &mut Channel, bytes: &[u8]) -> Result<(), String> {
        let r = match ch {
            Channel::L2cap(s) => s.send(bytes).await.map(|_| ()).map_err(|e| e.to_string()),
            Channel::Stream(s) => s.write_all(bytes).await.map_err(|e| e.to_string()),
            Channel::Gatt { write, .. } => write.write(bytes).await.map_err(|e| e.to_string()),
        };
        r.map_err(|e| format!("couldn't send to the accessory ({e})"))
    }

    async fn recv(ch: &mut Channel, buf: &mut [u8]) -> Result<Vec<u8>, String> {
        match ch {
            Channel::L2cap(s) => s.recv(buf).await.map(|n| buf[..n].to_vec()).map_err(|e| e.to_string()),
            Channel::Stream(s) => s.read(buf).await.map(|n| buf[..n].to_vec()).map_err(|e| e.to_string()),
            Channel::Gatt { notify, .. } => Ok(notify.next().await.unwrap_or_default()),
        }
    }

    /// GATT layout of a connected accessory, for "Help support this device".
    pub async fn gatt_layout(address: &str) -> Result<Vec<GattChar>, String> {
        let session = Session::new().await.map_err(|e| e.to_string())?;
        let adapter = session.default_adapter().await.map_err(|e| e.to_string())?;
        let dev = adapter.device(Address::from_str(address).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for s in dev.services().await.unwrap_or_default() {
            let su = s.uuid().await.map(|u| u.to_string()).unwrap_or_default();
            for c in s.characteristics().await.unwrap_or_default() {
                let uuid = c.uuid().await.map(|u| u.to_string()).unwrap_or_default();
                let flags = c.flags().await.ok();
                let mut properties = Vec::new();
                if let Some(f) = flags {
                    if f.read {
                        properties.push("read".to_string());
                    }
                    if f.write {
                        properties.push("write".to_string());
                    }
                    if f.write_without_response {
                        properties.push("write_without_response".to_string());
                    }
                    if f.notify {
                        properties.push("notify".to_string());
                    }
                    if f.indicate {
                        properties.push("indicate".to_string());
                    }
                }
                out.push(GattChar { service: su.clone(), uuid, properties });
            }
        }
        Ok(out)
    }

    pub async fn connect(address: &str, on: bool) -> Result<(), String> {
        let session = Session::new().await.map_err(|e| e.to_string())?;
        let adapter = session.default_adapter().await.map_err(|e| e.to_string())?;
        let dev = adapter.device(Address::from_str(address).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let r = if on { dev.connect().await } else { dev.disconnect().await };
        r.map_err(|e| format!("Bluetooth said: {e}"))
    }
}

// ------------------------------------------------------- elsewhere (BLE only)

#[cfg(not(target_os = "linux"))]
mod platform {
    use std::sync::Arc;
    use std::time::Duration;

    use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
    use btleplug::platform::Manager;
    use devti_core::accessory::{GattChar, Seen, match_profile};

    use super::{Accessories, Accessory, ProfileSummary, kind_of};

    const BATTERY_LEVEL: &str = "00002a19-0000-1000-8000-00805f9b34fb";

    pub async fn run(acc: Arc<Accessories>) -> Result<(), String> {
        let manager = Manager::new().await.map_err(|_| "Bluetooth isn't available.".to_string())?;
        let adapter =
            manager.adapters().await.ok().and_then(|a| a.into_iter().next()).ok_or("No Bluetooth adapter found.")?;
        *acc.status.write() = String::new();
        loop {
            let _ = adapter.start_scan(ScanFilter::default()).await;
            tokio::time::sleep(Duration::from_secs(8)).await;
            let _ = adapter.stop_scan().await;
            for p in adapter.peripherals().await.unwrap_or_default() {
                let Ok(Some(props)) = p.properties().await else { continue };
                let connected = p.is_connected().await.unwrap_or(false);
                let Some(name) = props.local_name.clone() else { continue };
                if !connected && props.rssi.is_none() {
                    continue;
                }
                let services: Vec<String> = props.services.iter().map(|u| u.to_string()).collect();
                let manufacturer_ids: Vec<u16> = props.manufacturer_data.keys().copied().collect();
                let seen =
                    Seen { name: name.clone(), manufacturer_ids: manufacturer_ids.clone(), services: services.clone() };
                let profile = match_profile(&seen, &[]);
                let mut battery = std::collections::BTreeMap::new();
                if connected {
                    let _ = p.discover_services().await;
                    if let Some(c) = p.characteristics().into_iter().find(|c| c.uuid.to_string() == BATTERY_LEVEL)
                        && let Ok(v) = p.read(&c).await
                        && let Some(&level) = v.first()
                    {
                        battery.insert(
                            "battery".to_string(),
                            devti_core::accessory::BatteryLevel { percent: level.min(100), charging: false },
                        );
                    }
                }
                let session = match &profile.transport {
                    None => "not needed".to_string(),
                    Some(devti_core::accessory::Transport::Gatt { .. }) => "idle".to_string(),
                    Some(_) => "Its controls need a Linux Hub for now (Windows support for this channel is coming)."
                        .to_string(),
                };
                acc.observe(Accessory {
                    address: p.address().to_string(),
                    kind: kind_of("", &name),
                    name,
                    paired: connected,
                    connected,
                    rssi: props.rssi,
                    battery,
                    profile: ProfileSummary::from(profile),
                    values: Default::default(),
                    session,
                    services,
                    manufacturer_ids,
                    drops: 0,
                    last_connected: None,
                    remember: true,
                });
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }

    pub async fn gatt_layout(_address: &str) -> Result<Vec<GattChar>, String> {
        Ok(Vec::new())
    }

    pub async fn connect(_address: &str, _on: bool) -> Result<(), String> {
        Err("Connect it from Windows' Bluetooth settings; Devti picks it up.".into())
    }
}

pub use platform::{connect, gatt_layout};

/// Draft profile for an accessory, written to the received-files folder.
pub async fn help_support(acc: &Accessories, address: &str) -> Result<String, String> {
    let a = acc.list.read().get(address).cloned().ok_or("unknown accessory")?;
    let gatt = gatt_layout(address).await.unwrap_or_default();
    export_draft(&acc.node, &seen_of(&a), &gatt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_from_icons_and_names() {
        assert_eq!(kind_of("audio-headset", "x"), "headphones");
        assert_eq!(kind_of("", "Galaxy Buds2"), "headphones");
        assert_eq!(kind_of("audio-card", "Soundbar"), "speaker");
        assert_eq!(kind_of("", "JBL Flip 6"), "speaker");
        assert_eq!(kind_of("input-keyboard", "K380"), "keyboard");
        assert_eq!(kind_of("input-mouse", "MX"), "mouse");
        assert_eq!(kind_of("input-gaming", "Pad"), "gamepad");
        assert_eq!(kind_of("", "Galaxy Watch"), "watch");
    }
}
