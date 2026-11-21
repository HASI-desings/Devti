//! Tray icon: Devti lives here when its window is closed.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use devti_core::NodeEvent;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_notification::NotificationExt;

use crate::{AppState, show_main_window};

pub struct TrayState {
    pause: MenuItem<Wry>,
    pair: MenuItem<Wry>,
    /// A UI action requested from the tray before the window was ready.
    pub pending_action: Mutex<Option<String>>,
    /// Files handed to Devti by "Send to Devti" that still need a target device.
    pub pending_send: Mutex<Vec<String>>,
}

pub fn build(app: &AppHandle, paused: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Devti", true, None::<&str>)?;
    let pair = MenuItem::with_id(app, "pair", "Pair a device…", !paused, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", pause_label(paused), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Devti", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &pair, &sep1, &pause, &sep2, &quit])?;

    let mut builder = TrayIconBuilder::with_id("devti")
        .tooltip("Devti")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "pair" => request_ui_action(app, "pair"),
            "pause" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let paused = state.node.is_paused();
                    if let Err(e) = state.node.set_paused(!paused) {
                        tracing::warn!(error = %e, "tray pause toggle failed");
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    app.manage(TrayState { pause, pair, pending_action: Mutex::new(None), pending_send: Mutex::new(Vec::new()) });
    Ok(())
}

fn pause_label(paused: bool) -> &'static str {
    if paused { "Resume ecosystem" } else { "Pause ecosystem" }
}

/// Opens the window and asks the UI to perform `action` (picked up even if the
/// window is still loading, via `take_ui_action`).
pub fn request_ui_action(app: &AppHandle, action: &str) {
    if let Some(t) = app.try_state::<TrayState>()
        && let Ok(mut p) = t.pending_action.lock()
    {
        *p = Some(action.to_string());
    }
    show_main_window(app);
    let _ = app.emit("devti://ui", action);
}

/// Keeps the tray menu in sync with the node.
pub fn on_node_event(app: &AppHandle, ev: &NodeEvent) {
    if let NodeEvent::Paused { paused } = ev
        && let Some(t) = app.try_state::<TrayState>()
    {
        let _ = t.pause.set_text(pause_label(*paused));
        let _ = t.pair.set_enabled(!*paused);
    }
}

/// Tells the user once per session that closing the window didn't quit Devti.
pub fn notify_still_running(app: &AppHandle) {
    static SHOWN: AtomicBool = AtomicBool::new(false);
    if SHOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = app
        .notification()
        .builder()
        .title("Devti is still running")
        .body("Your devices stay connected. Open Devti or quit it from the tray icon.")
        .show();
}
