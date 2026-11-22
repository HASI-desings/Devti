//! Phase 7 plugins: small WebAssembly programs that extend the Hub.
//!
//! * **Sandbox.** Each plugin runs in Extism (wasmtime) on its own thread: no WASI, so no
//!   files, clock tricks or processes; 64 MiB of memory; 2 seconds per call; web requests
//!   only to hosts it was granted. Everything else goes through Devti's host functions,
//!   and each of those checks the plugin's granted permissions first.
//! * **Consent.** A plugin lists its permissions in `devti-plugin.json`. The Hub shows them
//!   in plain words before installing; if an update asks for more, it stops until the user
//!   approves again. Unknown permissions are refused outright.
//! * **What a plugin can do.** Export `command` (run from the Plugins page, the phone's
//!   remote and deck, or a rule) and/or `on_event` (called for the events it subscribed
//!   to), and call `devti_log`, `devti_notify`, `devti_clipboard_get/set`, `devti_devices`
//!   and `devti_send_text`.
//!
//! Layout on disk: `<data>/plugins/<id>/devti-plugin.json` + `plugin.wasm`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use devti_core::Node;
use devti_core::model::DeviceId;
use devti_core::node::NodeEvent;
use devti_core::protocol::{CatalogCommand, HandoffItem};
use extism::{Manifest, PTR, Plugin, PluginBuilder, UserData, Wasm, host_fn};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

pub const MANIFEST_FILE: &str = "devti-plugin.json";
pub const WASM_FILE: &str = "plugin.wasm";
const SETTING: &str = "plugins";
const CALL_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_WAIT: Duration = Duration::from_secs(5);
/// 64 KiB pages: 64 MiB.
const MAX_PAGES: u32 = 1024;
const MAX_WASM_BYTES: u64 = 32 * 1024 * 1024;
const LOG_LINES: usize = 50;
/// Prefix of plugin commands in the Hub's catalog: `plugin:<plugin id>:<command id>`.
pub const COMMAND_PREFIX: &str = "plugin:";

// ------------------------------------------------------------------ manifest

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCommand {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub commands: Vec<PluginCommand>,
}

/// What a permission lets a plugin do, in the words the consent sheet shows.
pub fn describe_permission(p: &str) -> Option<String> {
    let fixed = match p {
        "notify" => "Show notifications on this computer",
        "clipboard:read" => "Read this computer's clipboard",
        "clipboard:write" => "Change this computer's clipboard (which may then sync to your devices)",
        "devices" => "See your devices' names, kinds, battery and whether they're online",
        "send" => "Put text on your devices' clipboards",
        "events:device" => "Be told when a device connects or disconnects",
        "events:clipboard" => "Be told when text is copied on another device — it receives the text",
        "events:notification" => "Be told about phone notifications — it receives their text",
        "events:battery" => "Be told when a device's battery runs low",
        _ => {
            let host = p.strip_prefix("http:")?;
            return valid_host(host).then(|| format!("Make web requests to {host}"));
        }
    };
    Some(fixed.to_string())
}

fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && !h.starts_with('.')
        && !h.starts_with('-')
        && h.split('.').all(|l| !l.is_empty() && l.len() <= 63)
        && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

impl PluginManifest {
    /// Checks everything the Hub relies on; the error says what to fix.
    pub fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.id) {
            return Err("The plugin id must be 1–40 lowercase letters, digits, - or _.".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 60 {
            return Err("The plugin needs a name of at most 60 characters.".into());
        }
        if self.version.is_empty() || self.version.len() > 20 {
            return Err("The plugin needs a version of at most 20 characters.".into());
        }
        if self.description.len() > 500 || self.author.len() > 100 {
            return Err("The description or author is too long.".into());
        }
        if self.permissions.len() > 20 {
            return Err("A plugin can ask for at most 20 permissions.".into());
        }
        if let Some(p) = self.permissions.iter().find(|p| describe_permission(p).is_none()) {
            return Err(format!("This plugin asks for “{p}”, which Devti doesn't know, so it can't be installed."));
        }
        if self.commands.len() > 20 {
            return Err("A plugin can have at most 20 commands.".into());
        }
        let mut seen = HashSet::new();
        for c in &self.commands {
            if !valid_id(&c.id) || c.name.trim().is_empty() || c.name.len() > 60 || !seen.insert(&c.id) {
                return Err(format!("The command “{}” needs a unique id and a name.", c.id));
            }
        }
        Ok(())
    }
}

pub fn read_manifest(dir: &Path) -> Result<PluginManifest, String> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE))
        .map_err(|_| format!("There's no {MANIFEST_FILE} in that folder."))?;
    let m: PluginManifest =
        serde_json::from_str(&text).map_err(|e| format!("{MANIFEST_FILE} couldn't be read: {e}"))?;
    m.validate()?;
    let wasm = dir.join(WASM_FILE);
    let size = std::fs::metadata(&wasm).map_err(|_| format!("There's no {WASM_FILE} next to the manifest."))?.len();
    if size > MAX_WASM_BYTES {
        return Err("The plugin is larger than 32 MB.".into());
    }
    Ok(m)
}

// ------------------------------------------------------------------ host side

/// What plugins can reach on this computer. The real one talks to the OS and the node;
/// tests use an in-memory one.
pub trait HostApi: Send + Sync {
    fn notify(&self, title: &str, body: &str) -> Result<(), String>;
    fn clipboard_get(&self) -> Result<String, String>;
    fn clipboard_set(&self, text: &str) -> Result<(), String>;
    fn devices(&self) -> serde_json::Value;
    fn send_text(&self, device: &str, text: &str) -> Result<(), String>;
}

struct AppHost {
    app: AppHandle,
    node: Node,
}

impl HostApi for AppHost {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        use tauri_plugin_notification::NotificationExt;
        self.app.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
    }

    fn clipboard_get(&self) -> Result<String, String> {
        arboard::Clipboard::new().and_then(|mut c| c.get_text()).map_err(|e| e.to_string())
    }

    fn clipboard_set(&self, text: &str) -> Result<(), String> {
        arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())).map_err(|e| e.to_string())
    }

    fn devices(&self) -> serde_json::Value {
        serde_json::Value::Array(self.node.peers().iter().map(device_json).collect())
    }

    fn send_text(&self, device: &str, text: &str) -> Result<(), String> {
        let id: DeviceId = device.parse().map_err(|_| "unknown device".to_string())?;
        let item = HandoffItem {
            kind: "text".into(),
            url: String::new(),
            title: String::new(),
            text: text.into(),
            position_ms: None,
        };
        self.node.handoff(&id, item).map_err(|e| e.to_string())
    }
}

fn device_json(p: &devti_core::node::PeerSnapshot) -> serde_json::Value {
    serde_json::json!({
        "id": p.info.id.to_string(),
        "name": p.info.name,
        "kind": p.info.kind,
        "online": p.online,
        "battery": p.status.battery.as_ref().map(|b| b.percent),
    })
}

type Log = Arc<Mutex<VecDeque<String>>>;

fn push_log(log: &Log, line: impl Into<String>) {
    let mut l = log.lock().unwrap_or_else(|e| e.into_inner());
    if l.len() == LOG_LINES {
        l.pop_front();
    }
    let line: String = line.into();
    l.push_back(line.chars().filter(|c| !c.is_control()).take(500).collect());
}

/// What every host function knows about the calling plugin.
pub struct Ctx {
    granted: HashSet<String>,
    host: Arc<dyn HostApi>,
    log: Log,
}

impl Ctx {
    fn need(&self, permission: &str) -> Result<(), extism::Error> {
        if self.granted.contains(permission) {
            Ok(())
        } else {
            Err(extism::Error::msg(format!("this plugin wasn't allowed “{permission}”")))
        }
    }
}

fn with_ctx<R>(ud: &UserData<Ctx>, f: impl FnOnce(&Ctx) -> Result<R, extism::Error>) -> Result<R, extism::Error> {
    let arc = ud.get()?;
    let ctx = arc.lock().map_err(|_| extism::Error::msg("plugin context poisoned"))?;
    f(&ctx)
}

host_fn!(devti_log(ud: Ctx; line: String) -> String {
    with_ctx(&ud, |c| {
        push_log(&c.log, line);
        Ok(String::new())
    })
});

host_fn!(devti_notify(ud: Ctx; json: String) -> String {
    with_ctx(&ud, |c| {
        c.need("notify")?;
        #[derive(Deserialize)]
        struct N { title: String, #[serde(default)] body: String }
        let n: N = serde_json::from_str(&json)?;
        let title: String = n.title.chars().take(100).collect();
        let body: String = n.body.chars().take(500).collect();
        c.host.notify(&title, &body).map_err(extism::Error::msg)?;
        Ok(String::new())
    })
});

host_fn!(devti_clipboard_get(ud: Ctx; _unused: String) -> String {
    with_ctx(&ud, |c| {
        c.need("clipboard:read")?;
        c.host.clipboard_get().map_err(extism::Error::msg)
    })
});

host_fn!(devti_clipboard_set(ud: Ctx; text: String) -> String {
    with_ctx(&ud, |c| {
        c.need("clipboard:write")?;
        if text.len() > 256 * 1024 {
            return Err(extism::Error::msg("clipboard text is limited to 256 KB"));
        }
        c.host.clipboard_set(&text).map_err(extism::Error::msg)?;
        Ok(String::new())
    })
});

host_fn!(devti_devices(ud: Ctx; _unused: String) -> String {
    with_ctx(&ud, |c| {
        c.need("devices")?;
        Ok(c.host.devices().to_string())
    })
});

host_fn!(devti_send_text(ud: Ctx; json: String) -> String {
    with_ctx(&ud, |c| {
        c.need("send")?;
        #[derive(Deserialize)]
        struct S { device: String, text: String }
        let s: S = serde_json::from_str(&json)?;
        c.host.send_text(&s.device, &s.text).map_err(extism::Error::msg)?;
        Ok(String::new())
    })
});

// ------------------------------------------------------------------ runtime

enum Job {
    Event(String),
    Command(String, mpsc::Sender<Result<String, String>>),
}

struct Running {
    tx: SyncSender<Job>,
}

/// Builds the sandbox for one plugin: its own manifest limits plus Devti's host functions.
pub fn build(wasm: Vec<u8>, manifest: &PluginManifest, host: Arc<dyn HostApi>, log: Log) -> Result<Plugin, String> {
    let hosts: Vec<String> =
        manifest.permissions.iter().filter_map(|p| p.strip_prefix("http:").map(String::from)).collect();
    let mut m = Manifest::new([Wasm::data(wasm)]).with_timeout(CALL_TIMEOUT).with_memory_max(MAX_PAGES);
    m = if hosts.is_empty() { m.disallow_all_hosts() } else { m.with_allowed_hosts(hosts.into_iter()) };
    let ud = UserData::new(Ctx { granted: manifest.permissions.iter().cloned().collect(), host, log });
    PluginBuilder::new(m)
        .with_wasi(false)
        .with_function("devti_log", [PTR], [PTR], ud.clone(), devti_log)
        .with_function("devti_notify", [PTR], [PTR], ud.clone(), devti_notify)
        .with_function("devti_clipboard_get", [PTR], [PTR], ud.clone(), devti_clipboard_get)
        .with_function("devti_clipboard_set", [PTR], [PTR], ud.clone(), devti_clipboard_set)
        .with_function("devti_devices", [PTR], [PTR], ud.clone(), devti_devices)
        .with_function("devti_send_text", [PTR], [PTR], ud, devti_send_text)
        .build()
        .map_err(|e| format!("The plugin couldn't start: {e}"))
}

fn spawn_runtime(
    id: &str,
    wasm: Vec<u8>,
    manifest: PluginManifest,
    host: Arc<dyn HostApi>,
    log: Log,
) -> Result<Running, String> {
    let (tx, rx) = mpsc::sync_channel::<Job>(64);
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let thread_log = log.clone();
    std::thread::Builder::new()
        .name(format!("devti-plugin-{id}"))
        .spawn(move || match build(wasm, &manifest, host, thread_log.clone()) {
            Ok(plugin) => {
                let _ = ready_tx.send(Ok(()));
                serve(plugin, rx, thread_log);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(())) => Ok(Running { tx }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("The plugin took too long to start.".into()),
    }
}

/// The reason behind a failed call, without wasmtime's backtrace.
fn plain(e: &extism::Error) -> String {
    e.root_cause().to_string()
}

fn serve(mut plugin: Plugin, rx: Receiver<Job>, log: Log) {
    while let Ok(job) = rx.recv() {
        match job {
            Job::Event(json) => {
                if plugin.function_exists("on_event")
                    && let Err(e) = plugin.call::<&str, String>("on_event", &json)
                {
                    push_log(&log, format!("on_event failed: {}", plain(&e)));
                }
            }
            Job::Command(cmd, reply) => {
                let r = if plugin.function_exists("command") {
                    plugin.call::<&str, String>("command", &cmd).map_err(|e| plain(&e))
                } else {
                    Err("This plugin has no commands.".to_string())
                };
                if let Err(e) = &r {
                    push_log(&log, format!("command {cmd} failed: {e}"));
                }
                let _ = reply.send(r);
            }
        }
    }
}

// ------------------------------------------------------------------ manager

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Saved {
    enabled: bool,
    granted: Vec<String>,
}

/// One installed plugin as the Plugins page shows it.
#[derive(Clone, Debug, Serialize)]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub enabled: bool,
    pub running: bool,
    /// The plugin now asks for permissions that weren't approved.
    pub needs_approval: bool,
    pub permissions: Vec<(String, String)>,
    pub problem: Option<String>,
    pub log: Vec<String>,
}

pub struct Plugins {
    dir: PathBuf,
    node: Node,
    app: Option<AppHandle>,
    host: Arc<dyn HostApi>,
    running: Mutex<HashMap<String, Running>>,
    problems: Mutex<HashMap<String, String>>,
    logs: Mutex<HashMap<String, Log>>,
    online: Mutex<HashMap<DeviceId, bool>>,
}

static HOST: OnceLock<Arc<Plugins>> = OnceLock::new();

/// The running plugin manager, once the Hub started it.
pub fn host() -> Option<Arc<Plugins>> {
    HOST.get().cloned()
}

impl Plugins {
    pub fn start(app: &AppHandle, node: Node, data_dir: &Path) -> Arc<Self> {
        let host: Arc<dyn HostApi> = Arc::new(AppHost { app: app.clone(), node: node.clone() });
        let me = Arc::new(Self::new(data_dir.join("plugins"), node, Some(app.clone()), host));
        let _ = HOST.set(me.clone());
        let bg = me.clone();
        std::thread::Builder::new()
            .name("devti-plugins".into())
            .spawn(move || {
                bg.load_all();
                bg.refresh_catalog();
            })
            .ok();
        me
    }

    pub fn new(dir: PathBuf, node: Node, app: Option<AppHandle>, host: Arc<dyn HostApi>) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self {
            dir,
            node,
            app,
            host,
            running: Mutex::new(HashMap::new()),
            problems: Mutex::new(HashMap::new()),
            logs: Mutex::new(HashMap::new()),
            online: Mutex::new(HashMap::new()),
        }
    }

    fn saved(&self) -> HashMap<String, Saved> {
        self.node.setting(SETTING).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self, map: &HashMap<String, Saved>) -> Result<(), String> {
        let json = serde_json::to_string(map).map_err(|e| e.to_string())?;
        self.node.set_setting(SETTING, &json).map_err(|e| e.to_string())
    }

    fn log_of(&self, id: &str) -> Log {
        self.logs.lock().unwrap_or_else(|e| e.into_inner()).entry(id.to_string()).or_default().clone()
    }

    fn installed(&self) -> Vec<(PathBuf, Result<PluginManifest, String>)> {
        let mut out: Vec<_> = std::fs::read_dir(&self.dir)
            .map(|it| {
                it.flatten().filter(|e| e.path().is_dir()).map(|e| (e.path(), read_manifest(&e.path()))).collect()
            })
            .unwrap_or_default();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn load_all(&self) {
        let saved = self.saved();
        for (path, m) in self.installed() {
            let Ok(m) = m else { continue };
            let s = saved.get(&m.id).cloned().unwrap_or_default();
            if s.enabled && m.permissions.iter().all(|p| s.granted.contains(p)) {
                self.launch(&path, &m);
            }
        }
    }

    fn launch(&self, path: &Path, m: &PluginManifest) {
        let log = self.log_of(&m.id);
        let result = std::fs::read(path.join(WASM_FILE))
            .map_err(|e| e.to_string())
            .and_then(|wasm| spawn_runtime(&m.id, wasm, m.clone(), self.host.clone(), log.clone()));
        match result {
            Ok(r) => {
                self.running.lock().unwrap_or_else(|e| e.into_inner()).insert(m.id.clone(), r);
                self.problems.lock().unwrap_or_else(|e| e.into_inner()).remove(&m.id);
                push_log(&log, format!("started {} {}", m.name, m.version));
            }
            Err(e) => {
                push_log(&log, e.clone());
                self.problems.lock().unwrap_or_else(|e| e.into_inner()).insert(m.id.clone(), e);
            }
        }
    }

    fn stop(&self, id: &str) {
        // Dropping the sender ends the plugin's thread after its current call.
        self.running.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        let saved = self.saved();
        let running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        let problems = self.problems.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.installed()
            .into_iter()
            .filter_map(|(_, m)| m.ok())
            .map(|m| {
                let s = saved.get(&m.id).cloned().unwrap_or_default();
                let log = self.log_of(&m.id).lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
                PluginInfo {
                    enabled: s.enabled,
                    running: running.contains_key(&m.id),
                    needs_approval: !m.permissions.iter().all(|p| s.granted.contains(p)),
                    permissions: m
                        .permissions
                        .iter()
                        .map(|p| (p.clone(), describe_permission(p).unwrap_or_default()))
                        .collect(),
                    problem: problems.get(&m.id).cloned(),
                    log,
                    manifest: m,
                }
            })
            .collect()
    }

    /// Reads a plugin folder for the consent sheet, without installing anything.
    pub fn inspect(&self, src: &Path) -> Result<PluginInfo, String> {
        let m = read_manifest(src)?;
        Ok(PluginInfo {
            permissions: m
                .permissions
                .iter()
                .map(|p| (p.clone(), describe_permission(p).unwrap_or_default()))
                .collect(),
            enabled: false,
            running: false,
            needs_approval: true,
            problem: None,
            log: Vec::new(),
            manifest: m,
        })
    }

    /// Copies a plugin in (replacing an older version), approves exactly the permissions
    /// it lists, and starts it.
    pub fn install(&self, src: &Path) -> Result<PluginInfo, String> {
        let m = read_manifest(src)?;
        let dest = self.dir.join(&m.id);
        self.stop(&m.id);
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        for f in [MANIFEST_FILE, WASM_FILE] {
            std::fs::copy(src.join(f), dest.join(f)).map_err(|e| format!("Couldn't copy {f}: {e}"))?;
        }
        let mut saved = self.saved();
        saved.insert(m.id.clone(), Saved { enabled: true, granted: m.permissions.clone() });
        self.save(&saved)?;
        self.launch(&dest, &m);
        self.node_log("plugin_installed", &m.id);
        self.refresh_catalog();
        if let Some(p) = self.problems.lock().unwrap_or_else(|e| e.into_inner()).get(&m.id) {
            return Err(p.clone());
        }
        self.list().into_iter().find(|p| p.manifest.id == m.id).ok_or_else(|| "installed, but not found".into())
    }

    /// Turns a plugin on or off. Turning on approves the permissions it lists now.
    pub fn set_enabled(&self, id: &str, on: bool) -> Result<(), String> {
        let (path, m) = self
            .installed()
            .into_iter()
            .find_map(|(p, m)| m.ok().filter(|m| m.id == id).map(|m| (p, m)))
            .ok_or("That plugin isn't installed.")?;
        let mut saved = self.saved();
        let entry = saved.entry(id.to_string()).or_default();
        entry.enabled = on;
        if on {
            entry.granted = m.permissions.clone();
        }
        self.save(&saved)?;
        self.stop(id);
        if on {
            self.launch(&path, &m);
        }
        self.node_log(if on { "plugin_enabled" } else { "plugin_disabled" }, id);
        self.refresh_catalog();
        match self.problems.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            Some(p) if on => Err(p.clone()),
            _ => Ok(()),
        }
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        if !valid_id(id) {
            return Err("That plugin isn't installed.".into());
        }
        self.stop(id);
        let dir = self.dir.join(id);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        let mut saved = self.saved();
        saved.remove(id);
        self.save(&saved)?;
        self.node_log("plugin_removed", id);
        self.refresh_catalog();
        Ok(())
    }

    /// Runs one of a plugin's commands and returns what it said.
    pub fn run_command(&self, plugin: &str, command: &str) -> Result<String, String> {
        let tx = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(plugin)
            .map(|r| r.tx.clone())
            .ok_or("That plugin isn't running. Turn it on in Plugins.")?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.try_send(Job::Command(command.to_string(), reply_tx))
            .map_err(|_| "The plugin is busy. Try again.".to_string())?;
        match reply_rx.recv_timeout(COMMAND_WAIT) {
            Ok(r) => r.map(|s| s.chars().take(4000).collect()),
            Err(_) => Err("The plugin didn't answer in time.".into()),
        }
    }

    /// Runs `plugin:<plugin>:<command>` ids from the catalog, the remote and rules.
    pub fn run_catalog_id(&self, id: &str) -> Option<Result<String, String>> {
        let rest = id.strip_prefix(COMMAND_PREFIX)?;
        let (plugin, command) = rest.split_once(':')?;
        Some(self.run_command(plugin, command))
    }

    /// Commands of running plugins, for the catalog remotes see.
    pub fn catalog_commands(&self) -> Vec<CatalogCommand> {
        let running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        self.installed()
            .into_iter()
            .filter_map(|(_, m)| m.ok())
            .filter(|m| running.contains_key(&m.id))
            .flat_map(|m| {
                let pid = m.id.clone();
                let pname = m.name.clone();
                m.commands.into_iter().map(move |c| CatalogCommand {
                    id: format!("{COMMAND_PREFIX}{pid}:{}", c.id),
                    name: format!("{} · {}", c.name, pname),
                    confirm: false,
                })
            })
            .collect()
    }

    fn refresh_catalog(&self) {
        if let Some(r) = self.app.as_ref().and_then(crate::remote::app_state) {
            r.publish_catalog();
        }
    }

    fn node_log(&self, action: &str, detail: &str) {
        tracing::info!(action, plugin = detail, "plugins");
    }

    /// Hands node events to plugins that subscribed to them (and may see them).
    pub fn on_node_event(&self, ev: &NodeEvent) {
        let Some((category, json)) = self.event_json(ev) else { return };
        let permission = format!("events:{category}");
        let saved = self.saved();
        let running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        for (id, r) in running.iter() {
            if saved.get(id).is_some_and(|s| s.granted.contains(&permission)) {
                // A plugin that can't keep up misses events rather than delaying Devti.
                let _ = r.tx.try_send(Job::Event(json.clone()));
            }
        }
    }

    fn event_json(&self, ev: &NodeEvent) -> Option<(&'static str, String)> {
        let v = match ev {
            NodeEvent::PeerChanged { peer } => {
                let mut online = self.online.lock().unwrap_or_else(|e| e.into_inner());
                if online.insert(peer.info.id, peer.online) == Some(peer.online) {
                    return None; // only connects and disconnects
                }
                ("device", serde_json::json!({ "type": "device", "device": device_json(peer) }))
            }
            NodeEvent::ClipboardReceived { from, from_name, text, .. } => (
                "clipboard",
                serde_json::json!({ "type": "clipboard", "from": from.to_string(), "from_name": from_name, "text": text }),
            ),
            NodeEvent::NotificationPosted { notification, .. } => (
                "notification",
                serde_json::json!({
                    "type": "notification",
                    "from": notification.from.to_string(),
                    "from_name": notification.from_name,
                    "app": notification.notif.app_name,
                    "title": notification.notif.title,
                    "text": notification.notif.text,
                }),
            ),
            NodeEvent::LowBattery { id, name, percent } => (
                "battery",
                serde_json::json!({ "type": "battery", "id": id.to_string(), "name": name, "percent": percent }),
            ),
            _ => return None,
        };
        Some((v.0, v.1.to_string()))
    }
}

pub fn app_state(app: &AppHandle) -> Option<Arc<Plugins>> {
    use tauri::Manager;
    app.try_state::<Arc<Plugins>>().map(|s| s.inner().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(perms: &[&str]) -> PluginManifest {
        PluginManifest {
            id: "link-cleaner".into(),
            name: "Link cleaner".into(),
            version: "1.0.0".into(),
            description: String::new(),
            author: String::new(),
            permissions: perms.iter().map(|s| s.to_string()).collect(),
            commands: vec![PluginCommand { id: "clean".into(), name: "Clean the copied link".into() }],
        }
    }

    #[test]
    fn manifests_are_validated_and_permissions_described() {
        assert!(manifest(&["clipboard:read", "clipboard:write", "notify", "http:api.example.org"]).validate().is_ok());
        assert!(manifest(&["root"]).validate().unwrap_err().contains("doesn't know"));
        assert!(manifest(&["http:"]).validate().is_err());
        assert!(manifest(&["http:bad host"]).validate().is_err());
        let mut m = manifest(&[]);
        m.id = "Bad Id".into();
        assert!(m.validate().is_err());
        let mut m = manifest(&[]);
        m.commands.push(m.commands[0].clone());
        assert!(m.validate().is_err(), "duplicate command ids");
        assert_eq!(describe_permission("http:api.example.org").unwrap(), "Make web requests to api.example.org");
        assert!(describe_permission("events:clipboard").unwrap().contains("receives the text"));
    }

    #[derive(Default)]
    struct FakeHost {
        clipboard: Mutex<String>,
        notes: Mutex<Vec<String>>,
    }

    impl HostApi for FakeHost {
        fn notify(&self, title: &str, body: &str) -> Result<(), String> {
            self.notes.lock().unwrap().push(format!("{title}: {body}"));
            Ok(())
        }
        fn clipboard_get(&self) -> Result<String, String> {
            Ok(self.clipboard.lock().unwrap().clone())
        }
        fn clipboard_set(&self, text: &str) -> Result<(), String> {
            *self.clipboard.lock().unwrap() = text.to_string();
            Ok(())
        }
        fn devices(&self) -> serde_json::Value {
            serde_json::json!([])
        }
        fn send_text(&self, _device: &str, _text: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// Runs the example plugin (built by CI into DEVTI_TEST_PLUGIN) in the real sandbox.
    #[test]
    fn example_plugin_runs_in_the_sandbox_within_its_permissions() {
        let Ok(path) = std::env::var("DEVTI_TEST_PLUGIN") else {
            eprintln!("DEVTI_TEST_PLUGIN not set: build plugins/link-cleaner for wasm32 to run this test");
            return;
        };
        let wasm = std::fs::read(&path).expect("the example plugin was built");
        let host = Arc::new(FakeHost::default());
        *host.clipboard.lock().unwrap() = "https://shop.example.org/item?id=7&utm_source=mail&fbclid=abc".into();
        let full = manifest(&["clipboard:read", "clipboard:write", "notify", "events:clipboard"]);
        let log: Log = Arc::default();
        let mut plugin = build(wasm.clone(), &full, host.clone(), log.clone()).unwrap();
        let out = plugin.call::<&str, String>("command", "clean").unwrap();
        assert_eq!(*host.clipboard.lock().unwrap(), "https://shop.example.org/item?id=7");
        assert!(out.contains("Removed 2"), "{out}");

        // An event from another device is cleaned the same way.
        let ev = serde_json::json!({ "type": "clipboard", "from_name": "Pixel 8", "text": "https://x.org/?gclid=1" });
        plugin.call::<&str, String>("on_event", &ev.to_string()).unwrap();
        assert_eq!(*host.clipboard.lock().unwrap(), "https://x.org/");

        // Without clipboard:write, the host refuses and the call fails.
        let read_only = manifest(&["clipboard:read"]);
        *host.clipboard.lock().unwrap() = "https://x.org/?utm_medium=a".into();
        let mut limited = build(wasm, &read_only, host.clone(), log).unwrap();
        let err = plain(&limited.call::<&str, String>("command", "clean").unwrap_err());
        assert!(err.contains("wasn't allowed"), "{err}");
        assert_eq!(*host.clipboard.lock().unwrap(), "https://x.org/?utm_medium=a");
    }
}
