//! Find my device: where every device is, on a map, with its IP addresses.
//!
//! ```text
//! finder                                   device
//!   |-- LocateRequest ------------------------>|  needs `locate` on the device
//!   |                                          |  host gets a fix (GPS, Wi-Fi, OS location)
//!   |<-------------- Location {lat, lon, …} ---|  core adds its public and local IPs
//! ```
//!
//! A device can also send `Location` on its own (stolen mode reports every 30 seconds).
//! Every device keeps the last location it heard from each peer, so the map shows where a
//! device was last seen even while it's offline. Locations never leave the paired
//! devices: Devti uses no location service of its own. The public IP comes from the
//! connect-from-anywhere endpoint, not a lookup website.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{Inner, Node, NodeEvent};
use crate::error::Result;
use crate::model::DeviceId;
use crate::net::now_ms;
use crate::protocol::ControlMsg;

fn setting(id: &DeviceId) -> String {
    format!("loc:{id}")
}

/// Where a device is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Location {
    /// Degrees (WGS 84); absent when the device couldn't get a fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    /// Radius of the 68 % confidence circle, in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accuracy_m: Option<f32>,
    /// When the fix was taken (ms since the epoch).
    pub ts: i64,
    /// "gps", "network", "os" or "none".
    #[serde(default)]
    pub source: String,
    /// The device's network as seen from the internet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_ip: Option<String>,
    /// Addresses on the device's own interfaces.
    #[serde(default)]
    pub local_ips: Vec<String>,
    /// Wi-Fi name or mobile carrier, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<u8>,
    /// Why there's no fix ("location is off", "no permission"…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Sent by stolen mode rather than asked for.
    #[serde(default)]
    pub live: bool,
}

impl Location {
    /// Keeps values in range and strings short (it came from the network).
    pub fn sanitized(mut self) -> Self {
        let ok = |v: Option<f64>, max: f64| v.filter(|x| x.is_finite() && x.abs() <= max);
        self.lat = ok(self.lat, 90.0);
        self.lon = ok(self.lon, 180.0);
        if self.lat.is_none() || self.lon.is_none() {
            self.lat = None;
            self.lon = None;
        }
        self.accuracy_m = self.accuracy_m.filter(|a| a.is_finite() && *a >= 0.0).map(|a| a.min(1_000_000.0));
        let short = |s: String, n: usize| s.chars().take(n).collect::<String>();
        self.source = short(self.source, 16);
        self.public_ip = self.public_ip.map(|s| short(s, 64));
        self.network = self.network.map(|s| short(s, 64));
        self.error = self.error.map(|s| short(s, 200));
        self.local_ips = self.local_ips.into_iter().take(16).map(|s| short(s, 64)).collect();
        self.battery = self.battery.map(|b| b.min(100));
        self
    }
}

/// What the host app knows; the core adds the IP addresses.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LocationFix {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub accuracy_m: Option<f32>,
    /// When the fix was taken; 0 means now.
    #[serde(default)]
    pub ts: i64,
    #[serde(default)]
    pub source: String,
    pub network: Option<String>,
    pub error: Option<String>,
}

/// A device's last known location, for the map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceLocation {
    pub device: DeviceId,
    pub location: Location,
}

pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, msg: ControlMsg) {
    match msg {
        ControlMsg::LocateRequest => {
            let Some(perms) = inner.permissions_of(&peer) else { return };
            if !perms.locate {
                return;
            }
            let name = inner.peer_name(&peer);
            inner.log(Some((&peer, &name)), "locate_requested", "");
            inner.emit(NodeEvent::LocateRequested { from: peer, from_name: name });
        }
        ControlMsg::Location { location } => {
            let location = location.sanitized();
            if let Ok(json) = serde_json::to_string(&location) {
                let _ = inner.store.set_setting(&setting(&peer), &json);
            }
            if !location.live {
                let name = inner.peer_name(&peer);
                inner.log(Some((&peer, &name)), "located", &location.source);
            }
            inner.emit(NodeEvent::LocationReceived { from: peer, location });
        }
        _ => {}
    }
}

/// Completes a fix with what the core knows: IP addresses and battery.
pub(crate) fn complete(inner: &Inner, fix: LocationFix, live: bool) -> Location {
    let public_ip = inner.anywhere.endpoint().and_then(|ep| {
        ep.addr()
            .ip_addrs()
            .find(|a| crate::model::ConnectionKind::of(a) == crate::model::ConnectionKind::Internet)
            .map(|a| a.ip().to_string())
    });
    Location {
        lat: fix.lat,
        lon: fix.lon,
        accuracy_m: fix.accuracy_m,
        ts: if fix.ts > 0 { fix.ts } else { now_ms() },
        source: if fix.source.is_empty() {
            if fix.lat.is_some() { "os".into() } else { "none".into() }
        } else {
            fix.source
        },
        public_ip,
        local_ips: crate::net::local_addresses().iter().map(|a| a.to_string()).collect(),
        network: fix.network,
        battery: inner.status.read().battery.map(|b| b.percent),
        error: fix.error,
        live,
    }
    .sanitized()
}

impl Node {
    /// Asks `peer` where it is; the answer arrives as `LocationReceived`.
    pub fn locate(&self, peer: &DeviceId) -> Result<()> {
        let name = self.inner.peer_name(peer);
        self.inner.send_to(peer, ControlMsg::LocateRequest)?;
        self.inner.log(Some((peer, &name)), "locate_sent", "");
        Ok(())
    }

    /// Answers a `LocateRequested` from `peer` with this device's fix.
    pub fn report_location(&self, peer: &DeviceId, fix: LocationFix) -> Result<()> {
        let location = complete(&self.inner, fix, false);
        self.inner.send_to(peer, ControlMsg::Location { location })
    }

    /// Sends this device's location to `peer` without being asked (stolen mode).
    pub fn push_location(&self, peer: &DeviceId, fix: LocationFix) -> Result<()> {
        let location = complete(&self.inner, fix, true);
        self.inner.send_to(peer, ControlMsg::Location { location })
    }

    /// This device's own location record, with its IPs (for its own marker on the map).
    pub fn own_location(&self, fix: LocationFix) -> Location {
        complete(&self.inner, fix, false)
    }

    /// The last location heard from every paired device.
    pub fn locations(&self) -> Vec<DeviceLocation> {
        let ids: Vec<DeviceId> = self.inner.peers.read().keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| {
                let json = self.inner.store.setting(&setting(&id)).ok().flatten()?;
                let location = serde_json::from_str::<Location>(&json).ok()?;
                Some(DeviceLocation { device: id, location })
            })
            .collect()
    }
}

pub(crate) fn forget(inner: &Inner, peer: &DeviceId) {
    let _ = inner.store.set_setting(&setting(peer), "");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_locations_are_cleaned() {
        let l = Location {
            lat: Some(f64::NAN),
            lon: Some(74.3),
            accuracy_m: Some(-5.0),
            network: Some("x".repeat(500)),
            local_ips: vec!["1.2.3.4".into(); 100],
            battery: Some(250),
            ..Default::default()
        }
        .sanitized();
        assert_eq!((l.lat, l.lon), (None, None), "half a coordinate is no coordinate");
        assert_eq!(l.accuracy_m, None);
        assert_eq!(l.network.unwrap().len(), 64);
        assert_eq!(l.local_ips.len(), 16);
        assert_eq!(l.battery, Some(100));
        let ok = Location { lat: Some(31.52), lon: Some(74.36), ..Default::default() }.sanitized();
        assert_eq!((ok.lat, ok.lon), (Some(31.52), Some(74.36)));
    }
}
