//! Devti Hub — the Tauri host around Devti Core.
//!
//! Sections:
//! * [`run`] — boots logging, the core node, plugins, tray and the event bridge.
//! * [`commands`] — everything the React UI may ask for (thin wrappers over `Node`).
//! * [`tray`] — tray icon and menu; the app keeps running when the window closes.
//! * [`bluetooth`] — live BLE scan used by onboarding.
//! * [`system`] — battery polling, OS details, diagnostics export.

mod accessories;
mod automation;
mod bluetooth;
mod commands;
mod comms_host;
mod continuity;
mod edges;
mod guard_setup;
mod location;
mod media;
mod plugins;
mod recorder;
mod remote;
mod screens;
mod sound;
mod system;
mod tray;

use std::sync::Arc;

use devti_core::model::{Capability, DeviceKind};
use devti_core::{Node, NodeConfig};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewWindowBuilder};
use tauri_plugin_autostart::MacosLauncher;

pub(crate) struct AppState {
    pub node: Node,
    pub bluetooth: bluetooth::Scanner,
    pub log_dir: std::path::PathBuf,
    pub data_dir: std::path::PathBuf,
}

/// Opens (or focuses) the main window. The window is destroyed when closed to the
/// tray, so this recreates it from the config when needed.
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let Some(cfg) = app.config().app.windows.iter().find(|w| w.label == "main").cloned() else { return };
    match WebviewWindowBuilder::from_config(app, &cfg).and_then(|b| b.build()) {
        Ok(w) => {
            tune_webview(&w);
            fit_to_screen(&w);
            let _ = w.set_focus();
        }
        Err(e) => tracing::error!(error = %e, "could not open the Devti window"),
    }
}

/// The configured 1280×820 is taller than many laptop screens (1366×768 minus a panel). Shrink
/// the window to the screen's usable area so its bottom is never off-screen.
fn fit_to_screen(w: &tauri::WebviewWindow) {
    let Ok(Some(m)) = w.current_monitor().or_else(|_| w.primary_monitor()) else { return };
    let area = m.work_area();
    let Ok(size) = w.outer_size() else { return };
    let (aw, ah) = (area.size.width, area.size.height);
    if size.width <= aw && size.height <= ah {
        return;
    }
    let scale = m.scale_factor();
    // Let the window go below its configured minimum on small screens instead of overflowing.
    let _ = w.set_min_size(Some(tauri::LogicalSize::new(
        640.0_f64.min(aw as f64 / scale),
        420.0_f64.min(ah as f64 / scale),
    )));
    let _ = w.set_size(tauri::PhysicalSize::new(size.width.min(aw), size.height.min(ah)));
    if size.height >= ah {
        let _ = w.maximize();
    } else {
        let _ = w.center();
    }
}

/// Devti's window is one app, not a browser: WebKit's "document viewer" cache model turns off
/// the page and memory caches meant for browsing many sites, so the window holds less memory.
pub(crate) fn tune_webview(w: &tauri::WebviewWindow) {
    #[cfg(target_os = "linux")]
    let _ = w.with_webview(|wv| {
        use webkit2gtk::{WebContextExt, WebViewExt};
        if let Some(ctx) = wv.inner().context() {
            ctx.set_cache_model(webkit2gtk::CacheModel::DocumentViewer);
        }
    });
    #[cfg(not(target_os = "linux"))]
    let _ = w;
}

fn init_logging(log_dir: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::prelude::*;
    let _ = std::fs::create_dir_all(log_dir);
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("devti")
        .filename_suffix("log")
        .max_log_files(7)
        .build(log_dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,devti_core=debug,devti_desktop_lib=debug"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(tracing_subscriber::fmt::layer().json().with_writer(writer))
        .try_init();
    Some(guard)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let start_hidden = std::env::args().any(|a| a == "--hidden");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            show_main_window(app);
            continuity::request_send(app, continuity::send_args(&args));
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            if let Some(w) = app.get_webview_window("main") {
                tune_webview(&w);
            }
            let handle = app.handle().clone();
            let data_dir = handle.path().app_data_dir()?;
            let log_dir = handle.path().app_log_dir()?;
            let guard = init_logging(&log_dir);
            // Keep the log writer alive for the whole process.
            std::mem::forget(guard);

            let has_battery = system::read_battery().is_some();
            let kind = if has_battery { DeviceKind::Laptop } else { DeviceKind::Desktop };
            let mut cfg = NodeConfig::new(&data_dir, &system::default_device_name(), kind);
            cfg.os_version = system::os_version();
            cfg.app_version = env!("CARGO_PKG_VERSION").to_string();
            cfg.capabilities = vec![Capability::Battery, Capability::BluetoothHub];
            let secrets = devti_core::secret::best_available("app.devti.hub", data_dir.join("secrets"))?;
            let node = tauri::async_runtime::block_on(Node::start(cfg, Arc::from(secrets)))?;

            let remote = remote::Remote::start(&handle, node.clone(), data_dir.clone());
            app.manage(remote.clone());
            let screens = screens::Screens::new(node.clone());
            app.manage(screens.clone());
            let edges = edges::Edges::new(node.clone());
            edges.start();
            app.manage(edges);
            app.manage(accessories::Accessories::start(&handle, node.clone()));
            let media = media::Media::start(&handle, node.clone());
            app.manage(media.clone());
            app.manage(sound::Sound::start(&handle, node.clone()));
            let automation = automation::Automation::start(&handle, node.clone());
            app.manage(automation.clone());
            let plugins = plugins::Plugins::start(&handle, node.clone(), &data_dir);
            app.manage(plugins.clone());

            // Forward every core event to the UI.
            let mut events = node.subscribe();
            let emitter = handle.clone();
            let event_node = node.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(ev) => {
                            tray::on_node_event(&emitter, &ev);
                            continuity::on_node_event(&emitter, &event_node, &ev);
                            remote::on_node_event(&emitter, &remote, &ev);
                            comms_host::on_node_event(&emitter, &event_node, &ev);
                            media.on_node_event(&ev);
                            location::on_node_event(&event_node, &ev);
                            if let Some(s) = sound::app_state(&emitter) {
                                s.on_node_event(&ev);
                            }
                            automation.on_node_event(&ev);
                            plugins.on_node_event(&ev);
                            if matches!(ev, devti_core::NodeEvent::ScreenRequested { .. }) {
                                screens.ensure_running(Some(&emitter));
                            }
                            // Input is high-frequency and only for the host; the UI doesn't need it.
                            if matches!(ev, devti_core::NodeEvent::RemoteInput { .. }) {
                                continue;
                            }
                            let _ = emitter.emit("devti://event", ev);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let _ = emitter.emit("devti://event", serde_json::json!({ "type": "resync" }));
                        }
                        Err(_) => break,
                    }
                }
            });

            system::spawn_battery_poller(node.clone());
            continuity::spawn_clipboard_watcher(node.clone());
            app.manage(AppState { node: node.clone(), bluetooth: bluetooth::Scanner::default(), log_dir, data_dir });
            tray::build(&handle, node.is_paused())?;
            if !start_hidden {
                show_main_window(&handle);
            }
            let send = continuity::send_args(&std::env::args().collect::<Vec<_>>());
            continuity::request_send(&handle, send);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::system_info,
            commands::open_pairing_window,
            commands::close_pairing_window,
            commands::confirm_pairing,
            commands::pair_with_code,
            commands::pair_with_code_at,
            commands::revoke,
            commands::set_access,
            commands::set_paused,
            commands::rename,
            commands::activity,
            commands::set_setting,
            commands::set_autostart,
            commands::bluetooth_scan_start,
            commands::bluetooth_scan_stop,
            commands::export_diagnostics,
            commands::take_ui_action,
            commands::take_pending_send,
            commands::clipboard_history,
            commands::clear_clipboard_history,
            commands::copy_to_clipboard,
            commands::share_clipboard,
            commands::ring,
            commands::notifications,
            commands::notification_action,
            commands::dismiss_notification,
            commands::set_dnd,
            commands::pick_files,
            commands::send_files,
            commands::transfers,
            commands::cancel_transfer,
            commands::retry_transfer,
            commands::clear_finished_transfers,
            commands::open_path,
            commands::reveal_path,
            commands::choose_downloads_dir,
            commands::battery_history,
            commands::set_low_battery_threshold,
            commands::send_input,
            commands::system_action,
            commands::launch_app,
            commands::run_remote_command,
            commands::deck_press,
            commands::request_catalog,
            commands::remote_settings,
            commands::save_commands,
            commands::save_deck,
            commands::test_command,
            commands::try_deck_action,
            commands::set_obs,
            commands::set_local_api,
            commands::reset_api_token,
            commands::screen_watch,
            commands::screen_unwatch,
            commands::save_capture,
            commands::screen_sources,
            commands::camera_view,
            commands::setup_webcam,
            commands::set_screen_source,
            commands::stop_screen_viewer,
            commands::screen_layout,
            commands::set_screen_layout,
            commands::accessories,
            commands::set_accessory_control,
            commands::set_accessory_remember,
            commands::connect_accessory,
            commands::accessory_battery_history,
            commands::help_support_accessory,
            commands::request_sms_threads,
            commands::request_sms_messages,
            commands::send_sms,
            commands::call_action,
            commands::handoff,
            commands::handoff_current,
            commands::media_status,
            commands::use_phone_camera,
            commands::stop_phone_camera,
            commands::automation,
            commands::save_rules,
            commands::run_rule_now,
            commands::pick_folder,
            commands::reach_info,
            commands::anywhere_info,
            commands::locate_device,
            commands::sound_status,
            commands::set_sound_mode,
            commands::set_voice_pause,
            commands::set_radio,
            commands::record_start,
            commands::record_stop,
            commands::guard_claim,
            commands::guard_set,
            commands::guard_release,
            commands::guard_unlock_code,
            commands::guard_statuses,
            commands::guard_setup_check,
            commands::guard_setup_run,
            commands::device_locations,
            commands::own_location,
            commands::set_anywhere,
            commands::set_reach,
            commands::peer_reach,
            commands::shared_folders,
            commands::set_shared_folders,
            commands::fs_list,
            commands::fs_get,
            commands::plugins,
            commands::plugin_inspect,
            commands::plugin_install,
            commands::plugin_set_enabled,
            commands::plugin_remove,
            commands::plugin_run,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Devti");

    app.run(|app, event| match event {
        // Closing the last window keeps Devti alive in the tray (unless the user turned
        // that off); its webview memory is released because the window is destroyed.
        RunEvent::ExitRequested { api, code: None, .. } => {
            let keep = app
                .try_state::<AppState>()
                .and_then(|s| s.node.setting("close_to_tray").ok().flatten())
                .is_none_or(|v| v != "false");
            if keep {
                api.prevent_exit();
                tray::notify_still_running(app);
            }
        }
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<AppState>() {
                tauri::async_runtime::block_on(state.node.shutdown());
            }
        }
        _ => {}
    });
}
