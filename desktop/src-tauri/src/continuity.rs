//! Desktop side of Phase 2: the OS clipboard, native notifications, find-my-device
//! window raising and "Send to Devti" paths.
//!
//! * Clipboard: polls the OS clipboard once a second (no OS offers a portable change
//!   event) and shares new text; text received from other devices is written to the OS
//!   clipboard. Devti Core suppresses echoes, so the two directions can't ping-pong.
//! * Native notifications for mirrored phone notifications (silent during Do Not
//!   Disturb), low battery and incoming find-my-device requests.

use std::time::Duration;

use devti_core::{Node, NodeEvent};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

pub const SETTING_CLIPBOARD_SYNC: &str = "clipboard_sync";

/// Starts the clipboard watcher on its own thread (arboard is synchronous).
pub fn spawn_clipboard_watcher(node: Node) {
    std::thread::Builder::new()
        .name("devti-clipboard".into())
        .spawn(move || {
            let mut clipboard = match arboard::Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "clipboard unavailable; clipboard sync is off on this computer");
                    return;
                }
            };
            let mut last = clipboard.get_text().ok();
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                if node.setting(SETTING_CLIPBOARD_SYNC).ok().flatten().as_deref() == Some("false") {
                    continue;
                }
                let now = clipboard.get_text().ok();
                if now.is_some() && now != last {
                    if let Some(text) = &now {
                        let _ = node.send_clipboard(text);
                    }
                    last = now;
                }
            }
        })
        .ok();
}

/// Writes text received from another device to the OS clipboard.
pub fn apply_clipboard(text: &str) {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
        Ok(()) => {}
        Err(e) => tracing::warn!(error = %e, "couldn't write the clipboard"),
    }
}

/// Reacts to node events that need the operating system.
pub fn on_node_event(app: &AppHandle, node: &Node, ev: &NodeEvent) {
    match ev {
        NodeEvent::ClipboardReceived { text, .. } => {
            if node.setting(SETTING_CLIPBOARD_SYNC).ok().flatten().as_deref() != Some("false") {
                apply_clipboard(text);
            }
        }
        NodeEvent::NotificationPosted { notification, dnd } => {
            if !dnd {
                let n = &notification.notif;
                let title = if n.title.is_empty() { n.app_name.clone() } else { n.title.clone() };
                let body = format!("{} · {}", notification.from_name, n.text);
                let _ = app.notification().builder().title(title).body(body).show();
            }
        }
        NodeEvent::LowBattery { name, percent, .. } => {
            let _ =
                app.notification().builder().title(format!("{name} is at {percent}%")).body("Plug it in soon.").show();
        }
        NodeEvent::RingRequested { from_name, on: true, .. } => {
            // The UI plays the sound; make sure there is a window to play it in.
            crate::show_main_window(app);
            let _ = app
                .notification()
                .builder()
                .title("Find my device")
                .body(format!("{from_name} is looking for this computer."))
                .show();
        }
        NodeEvent::Transfer { transfer, .. } if transfer.incoming && transfer.state == "done" => {
            let _ = app
                .notification()
                .builder()
                .title(format!("Received {}", transfer.name))
                .body(format!("From {}", transfer.device_name))
                .show();
        }
        _ => {}
    }
}

/// Paths passed on the command line as `--send <path>…` (Send to / Open with Devti).
pub fn send_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut take = false;
    for a in args {
        if a == "--send" {
            take = true;
        } else if a.starts_with("--") {
            take = false;
        } else if take {
            out.push(a.clone());
        }
    }
    out
}

/// Asks the UI to let the user pick a device for these files.
pub fn request_send(app: &AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    crate::show_main_window(app);
    if let Some(t) = tauri::Manager::try_state::<crate::tray::TrayState>(app)
        && let Ok(mut p) = t.pending_send.lock()
    {
        p.extend(paths);
    }
    let _ = app.emit("devti://ui", "send");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_args_are_parsed() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(send_args(&a(&["devti", "--send", "/a", "/b"])), a(&["/a", "/b"]));
        assert_eq!(send_args(&a(&["devti", "--hidden"])), Vec::<String>::new());
        assert_eq!(send_args(&a(&["devti", "--send", "/a", "--hidden"])), a(&["/a"]));
    }
}
