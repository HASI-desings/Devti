//! This computer's own location, for Find my device.
//!
//! * **Linux**: GeoClue (the system location service). Desktops ask the user the first
//!   time ("Devti wants to use your location"). GeoClue locates by Wi-Fi networks nearby,
//!   or by GPS where there is one.
//! * **Windows**: the Windows location service (Settings → Privacy → Location).
//!
//! When neither gives a fix, the device still reports its IP addresses and network, and
//! the map shows the last place it was seen.

use devti_core::node::locate::LocationFix;

/// A fix, or why there isn't one. Blocks for up to ~15 seconds; call off the UI thread.
pub fn fix() -> LocationFix {
    let network = network_name();
    match imp::locate() {
        Ok((lat, lon, accuracy)) => LocationFix {
            lat: Some(lat),
            lon: Some(lon),
            accuracy_m: Some(accuracy as f32),
            source: "os".into(),
            network,
            ..Default::default()
        },
        Err(e) => LocationFix { source: "none".into(), network, error: Some(e), ..Default::default() },
    }
}

/// Answers a device that asked where this computer is.
pub fn on_node_event(node: &devti_core::Node, ev: &devti_core::NodeEvent) {
    if let devti_core::NodeEvent::LocateRequested { from, .. } = ev {
        let (node, from) = (node.clone(), *from);
        tauri::async_runtime::spawn_blocking(move || {
            let _ = node.report_location(&from, fix());
        });
    }
}

/// The Wi-Fi network this computer is on, if any.
fn network_name() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("nmcli").args(["-t", "-f", "ACTIVE,SSID", "dev", "wifi"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| l.strip_prefix("yes:").map(str::to_string))
            .filter(|s| !s.is_empty())
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
            let l = l.trim();
            l.strip_prefix("SSID")
                .filter(|r| !r.starts_with("BSSID"))
                .and_then(|r| r.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::time::{Duration, Instant};

    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::OwnedObjectPath;

    const GEOCLUE: &str = "org.freedesktop.GeoClue2";

    pub fn locate() -> Result<(f64, f64, f64), String> {
        let conn = Connection::system().map_err(|_| "no system bus".to_string())?;
        let manager =
            Proxy::new(&conn, GEOCLUE, "/org/freedesktop/GeoClue2/Manager", "org.freedesktop.GeoClue2.Manager")
                .map_err(|e| e.to_string())?;
        let path: OwnedObjectPath = manager
            .call("GetClient", &())
            .map_err(|_| "Linux's location service (GeoClue) isn't running".to_string())?;
        let client =
            Proxy::new(&conn, GEOCLUE, path.as_str(), "org.freedesktop.GeoClue2.Client").map_err(|e| e.to_string())?;
        // GeoClue asks the desktop's agent to authorise this app id ("Devti" in the prompt).
        client.set_property("DesktopId", "devti").map_err(|e| e.to_string())?;
        client.set_property("RequestedAccuracyLevel", 8u32).map_err(|e| e.to_string())?; // exact
        client.call::<_, _, ()>("Start", &()).map_err(|e| format!("location is off or not allowed ({e})"))?;

        let deadline = Instant::now() + Duration::from_secs(15);
        let result = loop {
            // Check the current location first: GeoClue may already have one.
            let current: Option<OwnedObjectPath> = client.get_property("Location").ok();
            if let Some(p) = current.filter(|p| p.as_str() != "/")
                && let Some(r) = read(&conn, p.as_str())
            {
                break Ok(r);
            }
            if Instant::now() > deadline {
                break Err("no location fix yet (no Wi-Fi networks nearby?)".to_string());
            }
            std::thread::sleep(Duration::from_millis(500));
        };
        let _ = client.call::<_, _, ()>("Stop", &());
        result
    }

    fn read(conn: &Connection, path: &str) -> Option<(f64, f64, f64)> {
        let loc = Proxy::new(conn, GEOCLUE, path, "org.freedesktop.GeoClue2.Location").ok()?;
        let lat: f64 = loc.get_property("Latitude").ok()?;
        let lon: f64 = loc.get_property("Longitude").ok()?;
        let acc: f64 = loc.get_property("Accuracy").unwrap_or(1000.0);
        Some((lat, lon, acc))
    }
}

#[cfg(windows)]
mod imp {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};

    pub fn locate() -> Result<(f64, f64, f64), String> {
        // windows-future 0.3: `join()` blocks until the async operation finishes.
        let access: GeolocationAccessStatus = Geolocator::RequestAccessAsync()
            .and_then(|op| op.join())
            .map_err(|e| format!("Windows location isn't available ({e})"))?;
        if access != GeolocationAccessStatus::Allowed {
            return Err("location is off for Devti (Windows Settings → Privacy & security → Location)".into());
        }
        let locator = Geolocator::new().map_err(|e| e.to_string())?;
        let pos: windows::Devices::Geolocation::Geoposition =
            locator.GetGeopositionAsync().and_then(|op| op.join()).map_err(|e| e.to_string())?;
        let coord = pos.Coordinate().map_err(|e| e.to_string())?;
        let point = coord.Point().and_then(|p| p.Position()).map_err(|e| e.to_string())?;
        let accuracy = coord.Accuracy().unwrap_or(1000.0);
        Ok((point.Latitude, point.Longitude, accuracy))
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    pub fn locate() -> Result<(f64, f64, f64), String> {
        Err("location isn't supported here".into())
    }
}
