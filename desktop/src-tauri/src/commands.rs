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

// ------------------------------------------------------------- Phase 3

use devti_core::protocol::{InputEvent, SystemAction};
use tauri::ipc::{Channel, InvokeResponseBody, Request};

use crate::remote::{self, AppEntry, DeckAction, DeckPageDef, SavedCommand};

fn remote_of(app: &AppHandle) -> CmdResult<std::sync::Arc<remote::Remote>> {
    remote::app_state(app).ok_or_else(|| "remote control isn't ready yet".to_string())
}

#[tauri::command]
pub fn send_input(state: State<'_, AppState>, id: String, events: Vec<InputEvent>) -> CmdResult<()> {
    state.node.send_input(&parse_id(&id)?, events).map_err(err)
}

#[tauri::command]
pub fn system_action(state: State<'_, AppState>, id: String, action: SystemAction) -> CmdResult<()> {
    state.node.system_action(&parse_id(&id)?, action).map_err(err)
}

#[tauri::command]
pub fn launch_app(state: State<'_, AppState>, id: String, app: String) -> CmdResult<()> {
    state.node.launch_app(&parse_id(&id)?, &app).map_err(err)
}

#[tauri::command]
pub fn run_remote_command(state: State<'_, AppState>, id: String, command: String) -> CmdResult<()> {
    state.node.run_command(&parse_id(&id)?, &command).map_err(err)
}

#[tauri::command]
pub fn deck_press(state: State<'_, AppState>, id: String, button: String) -> CmdResult<()> {
    state.node.deck_press(&parse_id(&id)?, &button).map_err(err)
}

#[tauri::command]
pub fn request_catalog(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.node.request_catalog(&parse_id(&id)?).map_err(err)
}

#[derive(Serialize)]
pub struct RemoteSettings {
    commands: Vec<SavedCommand>,
    deck: Vec<DeckPageDef>,
    apps: Vec<AppEntry>,
    obs_url: String,
    obs_password_set: bool,
    api: serde_json::Value,
}

#[tauri::command]
pub fn remote_settings(app: AppHandle, state: State<'_, AppState>) -> CmdResult<RemoteSettings> {
    let r = remote_of(&app)?;
    Ok(RemoteSettings {
        commands: r.commands(),
        deck: r.deck(),
        apps: r.apps(),
        obs_url: state
            .node
            .setting(remote::SETTING_OBS_URL)
            .ok()
            .flatten()
            .unwrap_or_else(|| "ws://127.0.0.1:4455".into()),
        obs_password_set: state
            .node
            .setting(remote::SETTING_OBS_PASSWORD)
            .ok()
            .flatten()
            .is_some_and(|p| !p.is_empty()),
        api: r.api_info(),
    })
}

#[tauri::command]
pub fn save_commands(app: AppHandle, commands: Vec<SavedCommand>) -> CmdResult<()> {
    remote_of(&app)?.save_commands(commands).map_err(err)
}

#[tauri::command]
pub fn save_deck(app: AppHandle, pages: Vec<DeckPageDef>) -> CmdResult<()> {
    remote_of(&app)?.save_deck(pages).map_err(err)
}

/// Runs a command line from the editor's "Test" button; returns `(ok, output)`.
#[tauri::command]
pub async fn test_command(command: String) -> (bool, String) {
    remote::run_shell(&command).await
}

/// Tries a deck action on this computer (the editor's "Try it").
#[tauri::command]
pub async fn try_deck_action(app: AppHandle, action: DeckAction) -> CmdResult<()> {
    remote_of(&app)?.do_action(&action).await.map_err(err)
}

#[tauri::command]
pub fn set_obs(state: State<'_, AppState>, url: String, password: Option<String>) -> CmdResult<()> {
    state.node.set_setting(remote::SETTING_OBS_URL, url.trim()).map_err(err)?;
    if let Some(p) = password {
        state.node.set_setting(remote::SETTING_OBS_PASSWORD, &p).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_local_api(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> CmdResult<serde_json::Value> {
    state.node.set_setting(remote::SETTING_LOCAL_API, if enabled { "true" } else { "false" }).map_err(err)?;
    let r = remote_of(&app)?;
    r.apply_api_setting();
    Ok(r.api_info())
}

#[tauri::command]
pub fn reset_api_token(app: AppHandle) -> CmdResult<serde_json::Value> {
    let r = remote_of(&app)?;
    r.reset_api_token().map_err(err)?;
    Ok(r.api_info())
}

/// One frame for the UI: a 12-byte header (seq u32, width u16, height u16, latency ms u16,
/// fps u8, 0) followed by the JPEG.
fn frame_bytes(f: &devti_core::node::screen::ViewerFrame) -> Vec<u8> {
    let mut buf = Vec::with_capacity(12 + f.jpeg.len());
    buf.extend_from_slice(&(f.seq as u32).to_le_bytes());
    buf.extend_from_slice(&(f.w.min(65535) as u16).to_le_bytes());
    buf.extend_from_slice(&(f.h.min(65535) as u16).to_le_bytes());
    buf.extend_from_slice(&(f.latency_ms.min(65535) as u16).to_le_bytes());
    buf.push(f.fps);
    buf.push(0);
    buf.extend_from_slice(&f.jpeg);
    buf
}

/// Forwards `peer`'s frames of `source` to the UI until it stops or the view closes.
fn forward_frames(
    node: devti_core::Node,
    peer: DeviceId,
    source: &'static str,
    mut rx: tokio::sync::broadcast::Receiver<devti_core::node::screen::ViewerFrame>,
    frames: Channel<InvokeResponseBody>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(f) if f.from == peer && f.source == source => {
                    if frames.send(InvokeResponseBody::Raw(frame_bytes(&f))).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            }
            let still = if source == "camera" { node.is_viewing_camera(&peer) } else { node.is_viewing(&peer) };
            if !still {
                break;
            }
        }
    });
}

/// Starts watching a device's screen. Frames arrive on `frames` (see [`frame_bytes`]).
#[tauri::command]
pub fn screen_watch(
    state: State<'_, AppState>,
    id: String,
    max_width: u32,
    fps: u8,
    frames: Channel<InvokeResponseBody>,
) -> CmdResult<()> {
    let peer = parse_id(&id)?;
    let rx = state.node.subscribe_frames();
    state.node.screen_start(&peer, max_width, fps).map_err(err)?;
    forward_frames(state.node.clone(), peer, "screen", rx, frames);
    Ok(())
}

/// Shows a phone camera that's in use (see `use_phone_camera`) in the UI.
#[tauri::command]
pub fn camera_view(state: State<'_, AppState>, id: String, frames: Channel<InvokeResponseBody>) -> CmdResult<()> {
    let peer = parse_id(&id)?;
    if !state.node.is_viewing_camera(&peer) {
        return Err("that camera isn't on".into());
    }
    forward_frames(state.node.clone(), peer, "camera", state.node.subscribe_frames(), frames);
    Ok(())
}

/// Sets up the virtual webcam (Linux: loads v4l2loopback with one password prompt).
#[tauri::command]
pub async fn setup_webcam(app: AppHandle) -> CmdResult<crate::media::MediaStatus> {
    tauri::async_runtime::spawn_blocking(crate::media::set_up_webcam).await.map_err(err)??;
    let media = crate::media::app_state(&app).ok_or("media isn't ready")?;
    Ok(media.status())
}

#[tauri::command]
pub fn screen_unwatch(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.node.screen_stop(&parse_id(&id)?).map_err(err)
}

/// Saves a screenshot or recording of a mirrored device into the received-files folder.
/// Body: raw bytes; header `x-devti-name`: file name.
#[tauri::command]
pub fn save_capture(state: State<'_, AppState>, request: Request<'_>) -> CmdResult<String> {
    let name = request.headers().get("x-devti-name").and_then(|v| v.to_str().ok()).unwrap_or("capture.png").to_string();
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected file bytes".into());
    };
    let path = remote::capture_path(&state.node, &name);
    std::fs::write(&path, bytes).map_err(err)?;
    Ok(path.to_string_lossy().into())
}

// ------------------------------------------------------------- Phase 4

#[derive(Serialize)]
pub struct ScreenSources {
    sources: Vec<crate::screens::SourceInfo>,
    selected: Option<crate::screens::Source>,
    wayland: bool,
    viewers: Vec<String>,
}

#[tauri::command]
pub async fn screen_sources(app: AppHandle, state: State<'_, AppState>) -> CmdResult<ScreenSources> {
    let screens = crate::screens::app_state(&app).ok_or("screens aren't ready yet")?;
    let viewers = state.node.screen_viewers().into_iter().map(|d| d.to_string()).collect();
    let sources = tauri::async_runtime::spawn_blocking(crate::screens::Screens::sources).await.map_err(err)?;
    Ok(ScreenSources { sources, selected: screens.source(), wayland: crate::screens::wayland(), viewers })
}

#[tauri::command]
pub fn set_screen_source(app: AppHandle, source: Option<crate::screens::Source>) -> CmdResult<()> {
    crate::screens::app_state(&app).ok_or("screens aren't ready yet")?.set_source(source).map_err(err)
}

/// The Hub's own "stop" for one device watching its screen.
#[tauri::command]
pub fn stop_screen_viewer(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.node.screen_stop_sharing(&parse_id(&id)?);
    if let Some(s) = crate::screens::app_state(&app) {
        s.ensure_running(Some(&app));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct ScreenLayoutInfo {
    layout: crate::edges::Layout,
    supported: bool,
    active: bool,
}

#[tauri::command]
pub fn screen_layout(edges: State<'_, std::sync::Arc<crate::edges::Edges>>) -> ScreenLayoutInfo {
    ScreenLayoutInfo {
        layout: edges.layout(),
        supported: crate::edges::Edges::supported(),
        active: crate::edges::is_active(&edges),
    }
}

#[tauri::command]
pub fn set_screen_layout(
    edges: State<'_, std::sync::Arc<crate::edges::Edges>>,
    layout: crate::edges::Layout,
) -> CmdResult<()> {
    edges.set_layout(layout).map_err(err)
}

// ------------------------------------------------------------- Phase 5

fn accessories_of(app: &AppHandle) -> CmdResult<std::sync::Arc<crate::accessories::Accessories>> {
    crate::accessories::app_state(app).ok_or_else(|| "Bluetooth isn't ready yet".to_string())
}

#[derive(Serialize)]
pub struct AccessoryList {
    status: String,
    list: Vec<crate::accessories::Accessory>,
}

#[tauri::command]
pub fn accessories(app: AppHandle) -> CmdResult<AccessoryList> {
    let a = accessories_of(&app)?;
    let status = a.status.read().clone();
    Ok(AccessoryList { status, list: a.list() })
}

#[tauri::command]
pub fn set_accessory_control(app: AppHandle, address: String, control: String, value: String) -> CmdResult<()> {
    accessories_of(&app)?.set(&address, &control, &value)
}

#[tauri::command]
pub fn set_accessory_remember(app: AppHandle, address: String, on: bool) -> CmdResult<()> {
    accessories_of(&app)?.set_remember(&address, on);
    Ok(())
}

#[tauri::command]
pub async fn connect_accessory(address: String, on: bool) -> CmdResult<()> {
    crate::accessories::connect(&address, on).await
}

#[tauri::command]
pub fn accessory_battery_history(
    state: State<'_, AppState>,
    address: String,
    since: i64,
) -> CmdResult<Vec<devti_core::store::BatterySample>> {
    state.node.accessory_battery_history(&address, since).map_err(err)
}

/// "Help support this device": a draft profile (structure only) saved for the user to submit.
#[tauri::command]
pub async fn help_support_accessory(app: AppHandle, address: String) -> CmdResult<String> {
    let a = accessories_of(&app)?;
    crate::accessories::help_support(&a, &address).await
}

// ------------------------------------------------------------- Phase 6

#[tauri::command]
pub fn request_sms_threads(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.node.request_sms_threads(&parse_id(&id)?).map_err(err)
}

#[tauri::command]
pub fn request_sms_messages(state: State<'_, AppState>, id: String, thread: String) -> CmdResult<()> {
    state.node.request_sms_messages(&parse_id(&id)?, &thread).map_err(err)
}

#[tauri::command]
pub fn send_sms(state: State<'_, AppState>, id: String, to: Vec<String>, text: String) -> CmdResult<()> {
    state.node.send_sms(&parse_id(&id)?, to, &text).map_err(err)
}

#[tauri::command]
pub fn call_action(state: State<'_, AppState>, id: String, call: String, action: String) -> CmdResult<()> {
    state.node.call_action(&parse_id(&id)?, &call, &action).map_err(err)
}

#[tauri::command]
pub fn handoff(state: State<'_, AppState>, id: String, item: devti_core::protocol::HandoffItem) -> CmdResult<()> {
    state.node.handoff(&parse_id(&id)?, item).map_err(err)
}

/// "Continue on <device>": what's playing (Linux), or the link/text on the clipboard.
#[tauri::command]
pub fn handoff_current(state: State<'_, AppState>, id: String) -> CmdResult<String> {
    let item = crate::comms_host::current_item().ok_or("Nothing to continue: play something, or copy a link first.")?;
    let what = if item.kind == "media" {
        format!("“{}”", item.title)
    } else if item.kind == "url" {
        "the link".into()
    } else {
        "the text".into()
    };
    state.node.handoff(&parse_id(&id)?, item).map_err(err)?;
    Ok(what)
}

#[tauri::command]
pub fn media_status(app: AppHandle) -> CmdResult<crate::media::MediaStatus> {
    Ok(crate::media::app_state(&app).ok_or("media isn't ready")?.status())
}

#[tauri::command]
pub fn use_phone_camera(app: AppHandle, id: String, facing: Option<String>) -> CmdResult<()> {
    crate::media::app_state(&app).ok_or("media isn't ready")?.use_camera(&parse_id(&id)?, facing)
}

#[tauri::command]
pub fn stop_phone_camera(app: AppHandle, id: String) -> CmdResult<()> {
    crate::media::app_state(&app).ok_or("media isn't ready")?.stop_camera(&parse_id(&id)?);
    Ok(())
}

#[derive(Serialize)]
pub struct AutomationInfo {
    rules: Vec<devti_core::rules::Rule>,
    log: Vec<crate::automation::RunLog>,
    proximity: std::collections::HashMap<String, bool>,
}

#[tauri::command]
pub fn automation(app: AppHandle) -> CmdResult<AutomationInfo> {
    let a = crate::automation::app_state(&app).ok_or("automation isn't ready")?;
    Ok(AutomationInfo { rules: a.rules(), log: a.log(), proximity: a.proximity_state() })
}

#[tauri::command]
pub fn save_rules(app: AppHandle, rules: Vec<devti_core::rules::Rule>) -> CmdResult<()> {
    crate::automation::app_state(&app).ok_or("automation isn't ready")?.save(rules).map_err(err)
}

#[tauri::command]
pub fn run_rule_now(app: AppHandle, id: String) -> CmdResult<()> {
    crate::automation::app_state(&app).ok_or("automation isn't ready")?.run_now(&id)
}

// ------------------------------------------------------------------ Phase 7

/// A folder chooser (shared folders, plugin folders); `None` when cancelled.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, title: String) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title(title).pick_folder(move |f| {
        let _ = tx.send(f);
    });
    rx.await.ok().flatten().and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub fn reach_info(state: State<'_, AppState>) -> devti_core::node::ReachInfo {
    state.node.reach_info()
}

#[tauri::command]
pub fn set_reach(
    state: State<'_, AppState>,
    enabled: bool,
    extra: Vec<String>,
) -> CmdResult<devti_core::node::ReachInfo> {
    state.node.set_reach(enabled, extra).map_err(err)?;
    Ok(state.node.reach_info())
}

/// Connecting from anywhere: status, relay and this network's public address.
#[tauri::command]
pub fn anywhere_info(state: State<'_, AppState>) -> devti_core::node::AnywhereInfo {
    state.node.anywhere_info()
}

#[tauri::command]
pub async fn set_anywhere(
    state: State<'_, AppState>,
    enabled: bool,
    relay: String,
) -> CmdResult<devti_core::node::AnywhereInfo> {
    state.node.set_anywhere(enabled, &relay).await.map_err(err)?;
    Ok(state.node.anywhere_info())
}

// ------------------------------------------------------------- Find my device

/// Asks a device where it is; the answer arrives as a `location_received` event.
#[tauri::command]
pub fn locate_device(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.node.locate(&parse_id(&id)?).map_err(err)
}

/// The last location heard from every paired device.
#[tauri::command]
pub fn device_locations(state: State<'_, AppState>) -> Vec<devti_core::node::locate::DeviceLocation> {
    state.node.locations()
}

/// This computer's own location and addresses (may ask the OS for permission).
#[tauri::command]
pub async fn own_location(state: State<'_, AppState>) -> CmdResult<devti_core::node::locate::Location> {
    let fix = tauri::async_runtime::spawn_blocking(crate::location::fix).await.map_err(err)?;
    Ok(state.node.own_location(fix))
}

/// Where `id` said it can be reached away from home.
#[tauri::command]
pub fn peer_reach(state: State<'_, AppState>, id: String) -> CmdResult<Vec<String>> {
    Ok(state.node.peer_reach(&parse_id(&id)?))
}

#[tauri::command]
pub fn shared_folders(state: State<'_, AppState>) -> Vec<devti_core::node::SharedFolder> {
    state.node.shared_folders()
}

#[tauri::command]
pub fn set_shared_folders(state: State<'_, AppState>, folders: Vec<devti_core::node::SharedFolder>) -> CmdResult<()> {
    state.node.set_shared_folders(folders).map_err(err)
}

#[tauri::command]
pub fn fs_list(state: State<'_, AppState>, id: String, path: String) -> CmdResult<()> {
    state.node.fs_list(&parse_id(&id)?, &path).map_err(err)
}

#[tauri::command]
pub fn fs_get(state: State<'_, AppState>, id: String, path: String) -> CmdResult<()> {
    state.node.fs_get(&parse_id(&id)?, &path).map_err(err)
}

#[tauri::command]
pub fn plugins(app: AppHandle) -> CmdResult<Vec<crate::plugins::PluginInfo>> {
    Ok(crate::plugins::app_state(&app).ok_or("plugins aren't ready")?.list())
}

/// Reads a plugin folder for the consent sheet (nothing is installed yet).
#[tauri::command]
pub fn plugin_inspect(app: AppHandle, path: String) -> CmdResult<crate::plugins::PluginInfo> {
    crate::plugins::app_state(&app).ok_or("plugins aren't ready")?.inspect(std::path::Path::new(&path))
}

#[tauri::command]
pub async fn plugin_install(app: AppHandle, path: String) -> CmdResult<crate::plugins::PluginInfo> {
    let p = crate::plugins::app_state(&app).ok_or("plugins aren't ready")?;
    tauri::async_runtime::spawn_blocking(move || p.install(std::path::Path::new(&path))).await.map_err(err)?
}

#[tauri::command]
pub async fn plugin_set_enabled(app: AppHandle, id: String, enabled: bool) -> CmdResult<()> {
    let p = crate::plugins::app_state(&app).ok_or("plugins aren't ready")?;
    tauri::async_runtime::spawn_blocking(move || p.set_enabled(&id, enabled)).await.map_err(err)?
}

#[tauri::command]
pub fn plugin_remove(app: AppHandle, id: String) -> CmdResult<()> {
    crate::plugins::app_state(&app).ok_or("plugins aren't ready")?.remove(&id)
}

#[tauri::command]
pub async fn plugin_run(app: AppHandle, id: String, command: String) -> CmdResult<String> {
    let p = crate::plugins::app_state(&app).ok_or("plugins aren't ready")?;
    tauri::async_runtime::spawn_blocking(move || p.run_command(&id, &command)).await.map_err(err)?
}
