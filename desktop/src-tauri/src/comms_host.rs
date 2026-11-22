//! Phase 6 on the Hub: calls and texts from the phone become native notifications (the
//! UI shows the full call card and conversations), and handoffs open here.
//!
//! "Continue on phone" sends the link that's on the clipboard, or — on Linux — whatever
//! is playing (MPRIS gives the URL and position, e.g. a browser video), so it resumes
//! at the same second.

use devti_core::protocol::HandoffItem;
use devti_core::{Node, NodeEvent};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub fn on_node_event(app: &AppHandle, node: &Node, ev: &NodeEvent) {
    match ev {
        NodeEvent::Call { from_name, call, .. } if call.incoming && call.state == "ringing" => {
            let who = if call.name.is_empty() { call.number.clone() } else { call.name.clone() };
            let _ =
                app.notification().builder().title(format!("{who} is calling")).body(format!("On {from_name}")).show();
            crate::show_main_window(app);
        }
        NodeEvent::Sms { from_name, message, .. } if !message.outgoing && !node.dnd() => {
            let who = if message.name.is_empty() { message.address.clone() } else { message.name.clone() };
            let _ = app
                .notification()
                .builder()
                .title(format!("{who} · {from_name}"))
                .body(message.body.chars().take(240).collect::<String>())
                .show();
        }
        NodeEvent::Handoff { from_name, item, .. } => open_handoff(app, from_name, item),
        _ => {}
    }
}

/// Adds the position to links that understand it (YouTube's `t=`); others open as-is.
pub fn media_url(url: &str, position_ms: Option<u64>) -> String {
    let Some(ms) = position_ms else { return url.to_string() };
    let secs = ms / 1000;
    let youtube = url.contains("youtube.com/") || url.contains("youtu.be/");
    if !youtube || secs == 0 {
        return url.to_string();
    }
    let base: String = url.split(['#']).next().unwrap_or(url).to_string();
    let cleaned: Vec<&str> = base.split('&').filter(|p| !p.starts_with("t=")).collect();
    let base = cleaned.join("&");
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}t={secs}s")
}

fn open_handoff(app: &AppHandle, from: &str, item: &HandoffItem) {
    if !item.url.is_empty() {
        let url = media_url(&item.url, item.position_ms);
        if tauri_plugin_opener::open_url(&url, None::<&str>).is_err() {
            tracing::warn!("couldn't open a handoff link");
        }
        let _ = app.notification().builder().title(format!("Continued from {from}")).body(item.title.clone()).show();
    } else if !item.text.is_empty() {
        crate::continuity::apply_clipboard(&item.text);
        let _ = app.notification().builder().title(format!("From {from}")).body("Text is on the clipboard.").show();
    }
}

/// What to hand off from this computer: what's playing (Linux, MPRIS), else a link on
/// the clipboard.
pub fn current_item() -> Option<HandoffItem> {
    if let Some(item) = mpris::playing() {
        return Some(item);
    }
    let text = arboard::Clipboard::new().ok()?.get_text().ok()?;
    let t = text.trim();
    if t.starts_with("https://") || t.starts_with("http://") {
        return Some(HandoffItem {
            kind: "url".into(),
            url: t.to_string(),
            title: String::new(),
            text: String::new(),
            position_ms: None,
        });
    }
    if t.is_empty() {
        return None;
    }
    Some(HandoffItem {
        kind: "text".into(),
        url: String::new(),
        title: String::new(),
        text: t.chars().take(10_000).collect(),
        position_ms: None,
    })
}

#[cfg(target_os = "linux")]
mod mpris {
    use std::collections::HashMap;

    use devti_core::protocol::HandoffItem;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::OwnedValue;

    /// The first MPRIS player that is playing and has a web URL.
    pub fn playing() -> Option<HandoffItem> {
        let conn = Connection::session().ok()?;
        let dbus = Proxy::new(&conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").ok()?;
        let names: Vec<String> = dbus.call("ListNames", &()).ok()?;
        for name in names.into_iter().filter(|n| n.starts_with("org.mpris.MediaPlayer2.")) {
            let Ok(p) = Proxy::new(&conn, name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player")
            else {
                continue;
            };
            let status: String = p.get_property("PlaybackStatus").unwrap_or_default();
            if status != "Playing" {
                continue;
            }
            let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap_or_default();
            let url = meta.get("xesam:url").and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                continue;
            }
            let title = meta.get("xesam:title").and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
            let position: i64 = p.get_property("Position").unwrap_or(0);
            return Some(HandoffItem {
                kind: "media".into(),
                url,
                title,
                text: String::new(),
                position_ms: Some((position.max(0) / 1000) as u64),
            });
        }
        None
    }
}

#[cfg(not(target_os = "linux"))]
mod mpris {
    pub fn playing() -> Option<devti_core::protocol::HandoffItem> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::media_url;

    #[test]
    fn youtube_links_resume_at_the_same_second() {
        assert_eq!(
            media_url("https://www.youtube.com/watch?v=abc", Some(83_400)),
            "https://www.youtube.com/watch?v=abc&t=83s"
        );
        assert_eq!(media_url("https://youtu.be/abc", Some(5_000)), "https://youtu.be/abc?t=5s");
        assert_eq!(
            media_url("https://www.youtube.com/watch?v=abc&t=10s", Some(20_000)),
            "https://www.youtube.com/watch?v=abc&t=20s"
        );
        assert_eq!(media_url("https://example.com/v", Some(5_000)), "https://example.com/v");
        assert_eq!(media_url("https://youtu.be/abc", None), "https://youtu.be/abc");
    }
}
