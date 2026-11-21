//! Tauri commands: the only doors from the UI into the core.
//!
//! Each command is a thin, typed wrapper around [`devti_core::Node`]; errors become
//! plain sentences the UI can show as-is.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use devti_core::discovery::DiscoveredDevice;
use devti_core::model::{DeviceId, Permissions, TrustRole};
use devti_core::node::{LocalSnapshot, PairingWindowInfo, PeerSnapshot};
use devti_core::store::ActivityEntry;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::AppState;
use crate::system;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn parse_id(id: &str) -> CmdResult<DeviceId> {
    id.parse().map_err(err)
}

#[derive(Serialize)]
pub struct Snapshot {
    local: LocalSnapshot,
    peers: Vec<PeerSnapshot>,
    nearby: Vec<DiscoveredDevice>,
    window: Option<PairingWindowInfo>,
    settings: BTreeMap<String, String>,
}

#[tauri::command]
pub fn snapshot(state: State<'_, AppState>) -> CmdResult<Snapshot> {
    let node = &state.node;
    Ok(Snapshot {
        local: node.local(),
        peers: node.peers(),
        nearby: node.nearby(),
        window: node.pairing_window(),
        settings: node.settings().map_err(err)?.into_iter().collect(),
    })
}

#[derive(Serialize)]
pub struct SystemInfo {
    os: &'static str,
    os_version: String,
    session: String,
    autostart: bool,
    app_version: &'static str,
    data_dir: String,
    log_dir: String,
}

#[tauri::command]
pub fn system_info(app: AppHandle, state: State<'_, AppState>) -> SystemInfo {
    SystemInfo {
        os: devti_core::model::Platform::current().as_str(),
        os_version: system::os_version(),
        session: system::session_type(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        app_version: env!("CARGO_PKG_VERSION"),
        data_dir: system::tidy_path(&state.data_dir),
        log_dir: system::tidy_path(&state.log_dir),
    }
}

#[tauri::command]
pub fn open_pairing_window(state: State<'_, AppState>) -> CmdResult<PairingWindowInfo> {
    state.node.open_pairing_window().map_err(err)
}

#[tauri::command]
pub fn close_pairing_window(state: State<'_, AppState>) {
    state.node.close_pairing_window();
}

#[tauri::command]
pub fn confirm_pairing(state: State<'_, AppState>, pairing_id: u64, accept: bool) -> CmdResult<()> {
    state.node.confirm_pairing(pairing_id, accept).map_err(err)
}

#[tauri::command]
pub async fn pair_with_code(state: State<'_, AppState>, device_id: String, code: String) -> CmdResult<u64> {
    state.node.pair_with_code(&parse_id(&device_id)?, &code).map_err(err)
}

#[tauri::command]
pub async fn pair_with_code_at(state: State<'_, AppState>, address: String, code: String) -> CmdResult<u64> {
    let addr: SocketAddr = address
        .trim()
        .parse()
        .map_err(|_| "Enter the address as shown on the other computer, like 192.168.1.20:47810.".to_string())?;
    state.node.pair_with_code_at_address(addr, &code).map_err(err)
}

#[tauri::command]
pub async fn revoke(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.node.revoke(&parse_id(&id)?).map_err(err)
}

#[tauri::command]
pub fn set_access(
    state: State<'_, AppState>,
    id: String,
    role: TrustRole,
    permissions: Permissions,
) -> CmdResult<PeerSnapshot> {
    state.node.set_access(&parse_id(&id)?, role, permissions).map_err(err)
}

#[tauri::command]
pub async fn set_paused(state: State<'_, AppState>, paused: bool) -> CmdResult<()> {
    state.node.set_paused(paused).map_err(err)
}

#[tauri::command]
pub async fn rename(state: State<'_, AppState>, name: String) -> CmdResult<LocalSnapshot> {
    state.node.rename(&name).map_err(err)
}

#[tauri::command]
pub fn activity(
    state: State<'_, AppState>,
    limit: u32,
    before_id: Option<i64>,
    device_id: Option<String>,
) -> CmdResult<Vec<ActivityEntry>> {
    let device = device_id.as_deref().map(parse_id).transpose()?;
    state.node.activity(device.as_ref(), limit, before_id).map_err(err)
}

/// UI preferences the UI itself may write. Anything else is refused.
const UI_SETTINGS: &[&str] = &["theme", "motion", "close_to_tray", "onboarding_done", "autostart"];

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> CmdResult<()> {
    if !UI_SETTINGS.contains(&key.as_str()) || value.len() > 64 {
        return Err(format!("'{key}' is not a setting the app can change"));
    }
    state.node.set_setting(&key, &value).map_err(err)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> CmdResult<bool> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }
        .map_err(|e| format!("Couldn't change the startup setting: {e}"))?;
    let now = al.is_enabled().unwrap_or(enabled);
    let _ = state.node.set_setting("autostart", if now { "true" } else { "false" });
    Ok(now)
}

#[tauri::command]
pub fn bluetooth_scan_start(app: AppHandle, state: State<'_, AppState>) {
    state.bluetooth.start(app);
}

#[tauri::command]
pub fn bluetooth_scan_stop(state: State<'_, AppState>) {
    state.bluetooth.stop();
}

#[tauri::command]
pub fn export_diagnostics(app: AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    system::export_diagnostics(&app, &state.node, &state.log_dir).map_err(err)
}

/// Returns (and clears) an action the tray asked the UI to perform, e.g. "pair".
#[tauri::command]
pub fn take_ui_action(tray: State<'_, crate::tray::TrayState>) -> Option<String> {
    tray.pending_action.lock().ok().and_then(|mut p| p.take())
}
