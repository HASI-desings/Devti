//! Finding devices on the local network with mDNS / DNS-SD.
//!
//! Each running Devti node advertises `<device id>._devti._udp.local.` with a small TXT
//! record and browses for everyone else. Discovery only ever produces *hints*: an
//! address is never trusted because mDNS said so — the TLS handshake with pinned
//! fingerprints decides who a device really is.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::model::{DeviceId, DeviceKind, Fingerprint, Platform, sanitize_name};

/// DNS-SD service type (QUIC runs on UDP, see DECISIONS D-12).
pub const SERVICE_TYPE: &str = "_devti._udp.local.";

/// What this node announces about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Advertisement {
    pub id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    pub platform: Platform,
    pub port: u16,
    pub fingerprint: Fingerprint,
    /// Whether a pairing window is open (lets other devices offer "pair with code").
    pub pairing_open: bool,
}

impl Advertisement {
    /// TXT properties. Kept short: the whole record must fit one mDNS packet.
    pub fn txt(&self) -> Vec<(String, String)> {
        vec![
            ("v".into(), "1".into()),
            ("id".into(), self.id.to_string()),
            ("n".into(), self.name.clone()),
            ("k".into(), self.kind.as_str().into()),
            ("p".into(), self.platform.as_str().into()),
            ("f".into(), self.fingerprint.to_b64()),
            ("po".into(), if self.pairing_open { "1".into() } else { "0".into() }),
        ]
    }
}

/// A device seen on the network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    pub id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    pub platform: Platform,
    pub addrs: Vec<SocketAddr>,
    /// Advertised fingerprint (a hint; verified during the TLS handshake).
    pub fingerprint: Option<String>,
    pub pairing_open: bool,
}

impl DiscoveredDevice {
    /// Builds a device from TXT properties + resolved addresses. Returns `None` for
    /// records that aren't ours or are malformed.
    pub fn from_txt(get: impl Fn(&str) -> Option<String>, ips: &[IpAddr], port: u16) -> Option<Self> {
        if get("v").as_deref() != Some("1") || port == 0 {
            return None;
        }
        let id: DeviceId = get("id")?.parse().ok()?;
        let fingerprint = get("f").filter(|f| Fingerprint::from_b64(f).map(|fp| fp.device_id() == id).unwrap_or(false));
        let mut addrs: Vec<SocketAddr> =
            ips.iter().filter(|ip| usable_ip(ip)).map(|ip| SocketAddr::new(*ip, port)).collect();
        addrs.sort_by_key(|a| (a.is_ipv6(), a.ip()));
        addrs.dedup();
        addrs.truncate(8);
        if addrs.is_empty() {
            return None;
        }
        Some(Self {
            id,
            name: sanitize_name(&get("n").unwrap_or_default()),
            kind: DeviceKind::parse(&get("k").unwrap_or_default()),
            platform: Platform::parse(&get("p").unwrap_or_default()),
            addrs,
            fingerprint,
            pairing_open: get("po").as_deref() == Some("1"),
        })
    }
}

/// Addresses worth dialling: no loopback/unspecified/multicast, and no IPv6 link-local
/// (those need a scope id the QUIC layer can't carry portably).
fn usable_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_unspecified() && !v4.is_multicast() && !v4.is_broadcast(),
        IpAddr::V6(v6) => {
            !v6.is_loopback() && !v6.is_unspecified() && !v6.is_multicast() && (v6.segments()[0] & 0xffc0) != 0xfe80
        }
    }
}

/// Discovery events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryEvent {
    Found(DiscoveredDevice),
    Lost(DeviceId),
}

/// Running mDNS responder + browser.
pub struct Discovery {
    daemon: ServiceDaemon,
    fullname: String,
    own_id: DeviceId,
}

impl Discovery {
    /// Starts advertising and browsing. Events arrive on `events`.
    pub fn start(ad: &Advertisement, events: mpsc::UnboundedSender<DiscoveryEvent>) -> Result<Self> {
        let daemon = ServiceDaemon::new().map_err(disc)?;
        let fullname = register(&daemon, ad)?;
        let receiver = daemon.browse(SERVICE_TYPE).map_err(disc)?;
        let own_id = ad.id;
        tokio::spawn(async move {
            // fullname -> id, so removals (which only carry the name) can be mapped back.
            let mut names: HashMap<String, DeviceId> = HashMap::new();
            while let Ok(event) = receiver.recv_async().await {
                match event {
                    ServiceEvent::ServiceResolved(svc) => {
                        let ips: Vec<IpAddr> = svc.get_addresses().iter().map(|a| a.to_ip_addr()).collect();
                        let props = svc.get_properties();
                        let get = |k: &str| props.get_property_val_str(k).map(str::to_string);
                        if let Some(dev) = DiscoveredDevice::from_txt(get, &ips, svc.get_port()) {
                            if dev.id == own_id {
                                continue;
                            }
                            names.insert(svc.get_fullname().to_string(), dev.id);
                            if events.send(DiscoveryEvent::Found(dev)).is_err() {
                                break;
                            }
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        if let Some(id) = names.remove(&fullname)
                            && events.send(DiscoveryEvent::Lost(id)).is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(Self { daemon, fullname, own_id })
    }

    /// Re-announces with new data (rename, pairing window opened/closed).
    pub fn update(&mut self, ad: &Advertisement) -> Result<()> {
        debug_assert_eq!(ad.id, self.own_id);
        self.fullname = register(&self.daemon, ad)?;
        Ok(())
    }

    /// Sends goodbye packets and stops the daemon.
    pub fn stop(self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

fn register(daemon: &ServiceDaemon, ad: &Advertisement) -> Result<String> {
    let host = format!("devti-{}.local.", ad.id.to_string().to_lowercase());
    let txt = ad.txt();
    let props: Vec<(&str, &str)> = txt.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let info = ServiceInfo::new(SERVICE_TYPE, &ad.id.to_string(), &host, "", ad.port, &props[..])
        .map_err(disc)?
        .enable_addr_auto();
    let fullname = info.get_fullname().to_string();
    daemon.register(info).map_err(disc)?;
    Ok(fullname)
}

fn disc(e: impl std::fmt::Display) -> Error {
    Error::Discovery(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ad() -> Advertisement {
        let fp = Fingerprint([4; 32]);
        Advertisement {
            id: fp.device_id(),
            name: "Studio PC".into(),
            kind: DeviceKind::Desktop,
            platform: Platform::Windows,
            port: 47810,
            fingerprint: fp,
            pairing_open: true,
        }
    }

    fn lookup(txt: &[(String, String)]) -> impl Fn(&str) -> Option<String> + '_ {
        move |k| txt.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone())
    }

    #[test]
    fn txt_roundtrip() {
        let a = ad();
        let txt = a.txt();
        let ips: Vec<IpAddr> = vec![
            "192.168.1.9".parse().unwrap(),
            "127.0.0.1".parse().unwrap(),
            "fe80::2".parse().unwrap(),
            "2001:db8::5".parse().unwrap(),
        ];
        let d = DiscoveredDevice::from_txt(lookup(&txt), &ips, 47810).unwrap();
        assert_eq!(d.id, a.id);
        assert_eq!(d.name, "Studio PC");
        assert_eq!(d.kind, DeviceKind::Desktop);
        assert_eq!(d.platform, Platform::Windows);
        assert!(d.pairing_open);
        assert_eq!(d.fingerprint.as_deref(), Some(a.fingerprint.to_b64().as_str()));
        // Loopback and link-local are filtered, IPv4 sorted first.
        assert_eq!(d.addrs.len(), 2);
        assert_eq!(d.addrs[0].to_string(), "192.168.1.9:47810");
    }

    #[test]
    fn rejects_foreign_or_broken_records() {
        let a = ad();
        let txt = a.txt();
        let ips: Vec<IpAddr> = vec!["10.0.0.1".parse().unwrap()];
        assert!(DiscoveredDevice::from_txt(lookup(&txt), &ips, 0).is_none());
        assert!(DiscoveredDevice::from_txt(lookup(&txt), &[], 1).is_none());
        assert!(DiscoveredDevice::from_txt(|_| None, &ips, 1).is_none());
        let mut bad_v = txt.clone();
        bad_v[0].1 = "9".into();
        assert!(DiscoveredDevice::from_txt(lookup(&bad_v), &ips, 1).is_none());
        let mut bad_id = txt.clone();
        bad_id[1].1 = "nope".into();
        assert!(DiscoveredDevice::from_txt(lookup(&bad_id), &ips, 1).is_none());
        // A fingerprint that doesn't match the id is dropped, not trusted.
        let mut wrong_fp = txt.clone();
        wrong_fp[5].1 = Fingerprint([8; 32]).to_b64();
        assert_eq!(DiscoveredDevice::from_txt(lookup(&wrong_fp), &ips, 1).unwrap().fingerprint, None);
    }

    #[test]
    fn usable_ip_rules() {
        for bad in ["127.0.0.1", "0.0.0.0", "224.0.0.251", "255.255.255.255", "::1", "::", "ff02::fb", "fe80::1"] {
            assert!(!usable_ip(&bad.parse().unwrap()), "{bad}");
        }
        for good in ["192.168.0.4", "10.1.2.3", "2001:db8::1", "fd00::1"] {
            assert!(usable_ip(&good.parse().unwrap()), "{good}");
        }
    }

    #[tokio::test]
    async fn daemon_starts_updates_and_stops() {
        // Multicast may be unavailable in CI containers; the daemon must still start
        // and shut down cleanly.
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut a = ad();
        let Ok(mut d) = Discovery::start(&a, tx) else {
            eprintln!("mDNS unavailable in this environment; skipping");
            return;
        };
        a.pairing_open = false;
        d.update(&a).unwrap();
        d.stop();
    }
}
