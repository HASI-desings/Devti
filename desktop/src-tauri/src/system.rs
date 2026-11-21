//! OS-facing helpers: battery, names, session type, diagnostics.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use devti_core::Node;
use devti_core::model::{BatteryState, DeviceStatus};
use tauri::{AppHandle, Manager};

/// Current battery of this computer, `None` on desktops.
pub fn read_battery() -> Option<BatteryState> {
    let manager = starship_battery::Manager::new().ok()?;
    let battery = manager.batteries().ok()?.flatten().next()?;
    let pct = (battery.state_of_charge().value * 100.0).round().clamp(0.0, 100.0) as u8;
    let charging = matches!(battery.state(), starship_battery::State::Charging | starship_battery::State::Full);
    Some(BatteryState::new(pct, charging))
}

/// Publishes this computer's battery every minute (cheap: one sysfs/WMI read).
pub fn spawn_battery_poller(node: Node) {
    tauri::async_runtime::spawn(async move {
        loop {
            let battery = tauri::async_runtime::spawn_blocking(read_battery).await.ok().flatten();
            node.set_status(DeviceStatus { battery });
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

/// The computer's own name, e.g. "hussnain-thinkpad".
pub fn default_device_name() -> String {
    let host = gethostname::gethostname().to_string_lossy().to_string();
    let host = host.trim_end_matches(".local").trim();
    if host.is_empty() { "My computer".into() } else { host.to_string() }
}

/// "Windows 11 (26100)", "Linux Mint 22", …
pub fn os_version() -> String {
    let info = os_info::get();
    let version = info.version().to_string();
    if version == "Unknown" || version.is_empty() {
        info.os_type().to_string()
    } else {
        format!("{} {}", info.os_type(), version)
    }
}

/// "wayland", "x11", "windows" or "unknown".
pub fn session_type() -> String {
    if cfg!(target_os = "windows") {
        return "windows".into();
    }
    match std::env::var("XDG_SESSION_TYPE").unwrap_or_default().to_lowercase().as_str() {
        "wayland" => "wayland".into(),
        "x11" => "x11".into(),
        _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => "wayland".into(),
        _ if std::env::var_os("DISPLAY").is_some() => "x11".into(),
        _ => "unknown".into(),
    }
}

/// Replaces the home directory with `~` for display and diagnostics.
pub fn tidy_path(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    match home_dir() {
        Some(home) if !home.is_empty() && s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

fn home_dir() -> Option<String> {
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok()
}

/// Removes anything secret or personal from a log line.
pub fn redact(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    // Pairing secrets travel as `s=<base32>` inside devti:// links.
    while let Some(i) = rest.find("s=") {
        let (head, tail) = rest.split_at(i + 2);
        out.push_str(head);
        let end = tail.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(tail.len());
        if end > 0 {
            out.push_str("[redacted]");
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    let mut out = match home_dir() {
        Some(home) if !home.is_empty() => out.replace(&home, "~"),
        _ => out,
    };
    // Six-digit codes next to the word "code".
    for marker in ["code\":\"", "code=", "sas\":\""] {
        if let Some(i) = out.find(marker) {
            let start = i + marker.len();
            let end = out[start..].find(|c: char| !(c.is_ascii_digit() || c == ' ')).map_or(out.len(), |e| start + e);
            out.replace_range(start..end, "[redacted]");
        }
    }
    out
}

/// Writes a redacted bundle of recent logs plus basic facts to the Downloads folder.
pub fn export_diagnostics(app: &AppHandle, node: &Node, log_dir: &Path) -> anyhow::Result<String> {
    let dest_dir = app.path().download_dir().or_else(|_| app.path().home_dir())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dest_dir.join(format!("devti-diagnostics-{stamp}.txt"));
    let mut f = std::fs::File::create(&path)?;
    let local = node.local();
    writeln!(f, "Devti diagnostics")?;
    writeln!(f, "app {} / core {}", env!("CARGO_PKG_VERSION"), devti_core::VERSION)?;
    writeln!(f, "os {} / session {}", os_version(), session_type())?;
    writeln!(f, "device id {} / kind {}", local.info.id, local.info.kind.as_str())?;
    writeln!(
        f,
        "paused {} / discovery {} / key store {}",
        local.paused, local.discovery_enabled, local.secret_backend
    )?;
    writeln!(f, "paired devices {}", node.peers().len())?;
    for p in node.peers() {
        writeln!(
            f,
            "  {} {} online={} rtt={:?}",
            p.info.id,
            p.info.platform.as_str(),
            p.online,
            p.connection.map(|c| c.rtt_ms)
        )?;
    }
    writeln!(f, "\n--- recent log ---")?;
    let mut logs: Vec<_> = std::fs::read_dir(log_dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "log")).collect())
        .unwrap_or_default();
    logs.sort();
    for log in logs.iter().rev().take(2).rev() {
        if let Ok(text) = std::fs::read_to_string(log) {
            let lines: Vec<&str> = text.lines().collect();
            for line in lines.iter().skip(lines.len().saturating_sub(2000)) {
                writeln!(f, "{}", redact(line))?;
            }
        }
    }
    Ok(format!("Diagnostics saved to {}", tidy_path(&path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_removes_secrets() {
        let line = r#"{"msg":"open devti://pair?v=1&s=ABCDEFGH234567&p=1","code":"482 913"}"#;
        let r = redact(line);
        assert!(!r.contains("ABCDEFGH234567"));
        assert!(r.contains("s=[redacted]"));
        assert!(!r.contains("482 913"));
        assert_eq!(redact("nothing secret"), "nothing secret");
    }

    #[test]
    fn names_and_paths() {
        assert!(!default_device_name().is_empty());
        assert!(!os_version().is_empty());
        assert!(["wayland", "x11", "windows", "unknown"].contains(&session_type().as_str()));
        if let Some(home) = home_dir() {
            assert_eq!(tidy_path(&Path::new(&home).join("x")), format!("~{}x", std::path::MAIN_SEPARATOR));
        }
    }
}
