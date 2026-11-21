//! Live Bluetooth Low Energy scan (onboarding's "Accessories nearby" step).
//!
//! Phase 1 only *detects*: names, signal strength, maker and advertised standard
//! services. Pairing accessories and vendor controls arrive with the Bluetooth module
//! (see docs/DECISIONS.md D-26). Every outcome — including "no adapter" — is sent to
//! the UI as an honest status message.

use std::collections::HashMap;
use std::time::Duration;

use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use futures::StreamExt;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;

/// The UI scans for at most this long per visit.
const SCAN_LIMIT: Duration = Duration::from_secs(90);

#[derive(Default)]
pub struct Scanner {
    stop: Mutex<Option<watch::Sender<bool>>>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BtEvent {
    Adapter { available: bool, message: String },
    Device { device: BtDevice },
    Stopped,
}

#[derive(Serialize, Clone, PartialEq)]
struct BtDevice {
    id: String,
    name: Option<String>,
    rssi: Option<i16>,
    services: Vec<String>,
    manufacturer: Option<String>,
}

impl Scanner {
    pub fn start(&self, app: AppHandle) {
        let (tx, rx) = watch::channel(false);
        if let Some(old) = self.stop.lock().ok().and_then(|mut s| s.replace(tx)) {
            let _ = old.send(true);
        }
        tauri::async_runtime::spawn(async move {
            if let Err(msg) = scan(&app, rx).await {
                emit(&app, BtEvent::Adapter { available: false, message: msg });
            }
            emit(&app, BtEvent::Stopped);
        });
    }

    pub fn stop(&self) {
        if let Some(tx) = self.stop.lock().ok().and_then(|mut s| s.take()) {
            let _ = tx.send(true);
        }
    }
}

fn emit(app: &AppHandle, e: BtEvent) {
    let _ = app.emit("devti://bluetooth", e);
}

async fn scan(app: &AppHandle, mut stop: watch::Receiver<bool>) -> Result<(), String> {
    let manager = Manager::new().await.map_err(|e| unavailable(&e.to_string()))?;
    let adapters = manager.adapters().await.map_err(|e| unavailable(&e.to_string()))?;
    let adapter =
        adapters.into_iter().next().ok_or_else(|| "No Bluetooth adapter was found on this computer.".to_string())?;
    let mut events = adapter.events().await.map_err(|e| unavailable(&e.to_string()))?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| format!("Bluetooth is turned off or blocked ({e}). Turn it on and try again."))?;
    emit(app, BtEvent::Adapter { available: true, message: "Scanning".into() });

    let mut last: HashMap<String, BtDevice> = HashMap::new();
    let deadline = tokio::time::sleep(SCAN_LIMIT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            ev = events.next() => {
                let Some(ev) = ev else { break };
                let id = match ev {
                    CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => id,
                    _ => continue,
                };
                let Ok(p) = adapter.peripheral(&id).await else { continue };
                let Ok(Some(props)) = p.properties().await else { continue };
                let device = BtDevice {
                    id: id.to_string(),
                    name: props.local_name.filter(|n| !n.trim().is_empty()),
                    rssi: props.rssi,
                    services: props.services.iter().filter_map(service_label).map(str::to_string).collect(),
                    manufacturer: props.manufacturer_data.keys().find_map(|k| company(*k)).map(str::to_string),
                };
                // Skip anonymous beacons and unchanged updates to keep the list calm.
                if device.name.is_none() && device.manufacturer.is_none() {
                    continue;
                }
                let changed = last.get(&device.id).is_none_or(|old| {
                    old.name != device.name || old.rssi.zip(device.rssi).is_none_or(|(a, b)| (a - b).abs() >= 6)
                });
                if changed {
                    last.insert(device.id.clone(), device.clone());
                    emit(app, BtEvent::Device { device });
                }
            }
            _ = stop.changed() => break,
            _ = &mut deadline => break,
        }
    }
    let _ = adapter.stop_scan().await;
    Ok(())
}

fn unavailable(detail: &str) -> String {
    if cfg!(target_os = "linux") {
        format!("Bluetooth isn't available. Check that the bluetooth service is running ({detail}).")
    } else {
        format!("Bluetooth isn't available ({detail}).")
    }
}

/// Human labels for standard GATT services worth showing.
fn service_label(uuid: &uuid::Uuid) -> Option<&'static str> {
    let short = (uuid.as_u128() >> 96) as u32;
    let base = uuid.as_u128() & 0x0000_0000_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF;
    if base != 0x0000_0000_0000_1000_8000_0080_5F9B_34FB {
        return None;
    }
    Some(match short {
        0x180F => "Battery",
        0x180D => "Heart rate",
        0x1812 => "Keyboard or mouse",
        0x180A => "Device info",
        0x184E | 0x184F | 0x1850 | 0x1853 => "LE Audio",
        0xFE2C => "Fast Pair",
        0x1816 => "Cycling",
        _ => return None,
    })
}

/// Bluetooth SIG company identifiers for common accessory makers.
fn company(id: u16) -> Option<&'static str> {
    Some(match id {
        0x004C => "Apple",
        0x0075 => "Samsung",
        0x012D => "Sony",
        0x027D => "Huawei",
        0x0057 => "Harman (JBL)",
        0x00E0 => "Google",
        0x009E => "Bose",
        0x0006 => "Microsoft",
        0x0087 => "Garmin",
        0x038F => "Xiaomi",
        0x0310 => "Nothing",
        0x0171 => "Amazon",
        0x0499 => "Ruuvi",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        let battery = uuid::Uuid::from_u128(0x0000_180F_0000_1000_8000_0080_5F9B_34FB);
        assert_eq!(service_label(&battery), Some("Battery"));
        let custom = uuid::Uuid::from_u128(0x1234_5678_0000_1000_8000_0080_5F9B_34FC);
        assert_eq!(service_label(&custom), None);
        assert_eq!(company(0x004C), Some("Apple"));
        assert_eq!(company(0xFFFF), None);
    }
}
