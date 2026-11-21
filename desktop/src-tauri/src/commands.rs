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
    dnd: bool,
    low_battery_threshold: u8,
    downloads_dir: String,
    notifications: Vec<devti_core::node::MirroredNotification>,
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
        dnd: node.dnd(),
        low_battery_threshold: node.low_battery_threshold(),
        downloads_dir: node.downloads_dir().to_string_lossy().into(),
        notifications: node.notifications(),
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
const UI_SETTINGS: &[&str] =
    &["theme", "motion", "close_to_tray", "onboarding_done", "autostart", crate::continuity::SETTING_CLIPBOARD_SYNC];

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

// ------------------------------------------------------------------ Phase 2

#[tauri::command]
pub fn take_pending_send(tray: State<'_, crate::tray::TrayState>) -> Vec<String> {
    tray.pending_send.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default()
}

#[tauri::command]
pub fn clipboard_history(state: State<'_, AppState>) -> CmdResult<Vec<devti_core::store::ClipboardEntry>> {
    state.node.clipboard_history().map_err(err)
}

#[tauri::command]
pub fn clear_clipboard_history(state: State<'_, AppState>) -> CmdResult<()> {
    state.node.clear_clipboard_history().map_err(err)
}

/// Puts text on this computer's clipboard (e.g. from the history list).
#[tauri::command]
pub fn copy_to_clipboard(text: String) {
    crate::continuity::apply_clipboard(&text);
}

/// Shares text with every device right away (the history "send again" button).
#[tauri::command]
pub async fn share_clipboard(state: State<'_, AppState>, text: String) -> CmdResult<bool> {
    state.node.send_clipboard(&text).map_err(err)
}

#[tauri::command]
pub async fn ring(state: State<'_, AppState>, id: String, on: bool) -> CmdResult<()> {
    state.node.ring(&parse_id(&id)?, on).map_err(err)
}

#[tauri::command]
pub fn notifications(state: State<'_, AppState>) -> Vec<devti_core::node::MirroredNotification> {
    state.node.notifications()
}

#[tauri::command]
pub async fn notification_action(
    state: State<'_, AppState>,
    id: String,
    key: String,
    action: String,
    reply: Option<String>,
) -> CmdResult<()> {
    state.node.notification_action(&parse_id(&id)?, &key, &action, reply).map_err(err)
}

#[tauri::command]
pub async fn dismiss_notification(state: State<'_, AppState>, id: String, key: String) -> CmdResult<()> {
    state.node.dismiss_notification(&parse_id(&id)?, &key).map_err(err)
}

#[tauri::command]
pub async fn set_dnd(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    state.node.set_dnd(on);
    Ok(())
}

/// Native file picker. Returns the chosen paths (empty when cancelled).
#[tauri::command]
pub async fn pick_files(app: AppHandle) -> Vec<String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title("Choose files to send").pick_files(move |files| {
        let _ = tx.send(files.unwrap_or_default());
    });
    rx.await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

#[tauri::command]
pub async fn send_files(state: State<'_, AppState>, id: String, paths: Vec<String>) -> CmdResult<Vec<u64>> {
    let peer = parse_id(&id)?;
    paths.iter().map(|p| state.node.send_file(&peer, std::path::Path::new(p)).map_err(err)).collect()
}

#[tauri::command]
pub fn transfers(state: State<'_, AppState>) -> CmdResult<Vec<devti_core::store::TransferRecord>> {
    state.node.transfers().map_err(err)
}

#[tauri::command]
pub fn cancel_transfer(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    state.node.cancel_transfer(id).map_err(err)
}

#[tauri::command]
pub fn retry_transfer(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    state.node.retry_transfer(id).map_err(err)
}

#[tauri::command]
pub fn clear_finished_transfers(state: State<'_, AppState>) -> CmdResult<()> {
    state.node.clear_finished_transfers().map_err(err)
}

/// Opens a received file with its default app. Only files inside the downloads folder.
#[tauri::command]
pub fn open_path(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    ensure_in_downloads(&state, &path)?;
    app.opener().open_path(&path, None::<&str>).map_err(err)
}

#[tauri::command]
pub fn reveal_path(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    ensure_in_downloads(&state, &path)?;
    app.opener().reveal_item_in_dir(&path).map_err(err)
}

fn ensure_in_downloads(state: &State<'_, AppState>, path: &str) -> CmdResult<()> {
    let dir = state.node.downloads_dir().canonicalize().map_err(err)?;
    let p = std::path::Path::new(path).canonicalize().map_err(err)?;
    if p.starts_with(&dir) { Ok(()) } else { Err("Only received files can be opened from Devti.".into()) }
}

/// Lets the user choose where received files are saved.
#[tauri::command]
pub async fn choose_downloads_dir(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title("Save received files in").pick_folder(move |f| {
        let _ = tx.send(f);
    });
    let Some(dir) = rx.await.ok().flatten().and_then(|f| f.into_path().ok()) else { return Ok(None) };
    state.node.set_downloads_dir(&dir).map_err(err)?;
    Ok(Some(dir.to_string_lossy().into()))
}

#[tauri::command]
pub fn battery_history(
    state: State<'_, AppState>,
    id: Option<String>,
    since: i64,
) -> CmdResult<Vec<devti_core::store::BatterySample>> {
    let id = id.as_deref().map(parse_id).transpose()?;
    state.node.battery_history(id.as_ref(), since).map_err(err)
}

#[tauri::command]
pub fn set_low_battery_threshold(state: State<'_, AppState>, percent: u8) -> CmdResult<()> {
    state.node.set_low_battery_threshold(percent).map_err(err)
}
