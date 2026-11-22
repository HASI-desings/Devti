//! Phase 7 reach: reconnecting when devices aren't on the same local network.
//!
//! With remote access on, a device tells its peers every other address it can be reached
//! at: addresses of private overlay networks found on its interfaces (Tailscale or
//! Headscale, ZeroTier, WireGuard…) and addresses the user typed (a forwarded port or a
//! DNS name). Peers keep the list and dial it when the device isn't found on the local
//! network. Every connection still proves both identities against the pinned
//! certificates, so a wrong or hostile address can't stand in for a device. Devti runs no
//! relay or rendezvous server of its own.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::{Inner, Node, NodeEvent};
use crate::model::{ConnectionKind, DeviceId};
use crate::protocol::ControlMsg;
use crate::{Error, Result};

/// Addresses kept per peer.
pub const MAX_REACH: usize = 16;
/// Addresses a user can type.
pub const MAX_EXTRA: usize = 8;
const SETTING_ENABLED: &str = "reach_enabled";
const SETTING_EXTRA: &str = "reach_extra";
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(3);

fn peer_key(id: &DeviceId) -> String {
    format!("reach:{id}")
}

pub(crate) struct State {
    peers: RwLock<HashMap<DeviceId, Vec<String>>>,
}

impl State {
    pub fn new() -> Self {
        Self { peers: RwLock::new(HashMap::new()) }
    }

    pub fn of(&self, id: &DeviceId) -> Vec<String> {
        self.peers.read().get(id).cloned().unwrap_or_default()
    }
}

/// One address this device can be reached at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachAddress {
    pub address: String,
    /// "lan", "vpn", "internet" or "manual".
    pub kind: String,
    /// The network interface it was found on (empty for typed ones).
    #[serde(default)]
    pub interface: String,
}

/// What the "Reach from anywhere" settings show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachInfo {
    pub enabled: bool,
    pub port: u16,
    /// Every address found here; only non-LAN ones (and typed ones) are sent to peers.
    pub addresses: Vec<ReachAddress>,
    /// Addresses the user typed.
    pub extra: Vec<String>,
}

/// Whether `s` is an address Devti will store and dial: "ip:port", "[v6]:port" or
/// "host.name:port" with a non-zero port.
pub fn valid_entry(s: &str) -> bool {
    if s.is_empty() || s.len() > 260 {
        return false;
    }
    if let Ok(a) = s.parse::<SocketAddr>() {
        return a.port() != 0 && !a.ip().is_unspecified() && !a.ip().is_multicast();
    }
    let Some((host, port)) = s.rsplit_once(':') else { return false };
    let port_ok = port.parse::<u16>().is_ok_and(|p| p != 0);
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && !host.starts_with('.')
        && host.split('.').all(|l| !l.is_empty() && l.len() <= 63)
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    port_ok && host_ok
}

fn kind_name(k: ConnectionKind) -> &'static str {
    match k {
        ConnectionKind::Lan => "lan",
        ConnectionKind::Vpn => "vpn",
        ConnectionKind::Internet => "internet",
    }
}

/// Addresses on this machine's interfaces, for the endpoint's address family.
fn interface_addresses(port: u16, ipv4: bool) -> Vec<ReachAddress> {
    let mut out: Vec<ReachAddress> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && i.ip().is_ipv4() == ipv4)
        .filter(|i| match i.ip() {
            IpAddr::V4(v4) => !v4.is_link_local() && !v4.is_unspecified(),
            IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) != 0xfe80 && !v6.is_unspecified(),
        })
        .map(|i| {
            let addr = SocketAddr::new(i.ip(), port);
            ReachAddress {
                address: addr.to_string(),
                kind: kind_name(ConnectionKind::of(&addr)).into(),
                interface: i.name,
            }
        })
        .collect();
    out.sort_by(|a, b| a.address.cmp(&b.address));
    out.dedup_by(|a, b| a.address == b.address);
    out
}

fn extra(inner: &Inner) -> Vec<String> {
    inner
        .store
        .setting(SETTING_EXTRA)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

fn enabled(inner: &Inner) -> bool {
    inner.store.setting(SETTING_ENABLED).ok().flatten().as_deref() == Some("true")
}

/// What this device tells its peers (empty when remote access is off).
pub(crate) fn own_reach(inner: &Inner) -> Vec<String> {
    if !enabled(inner) {
        return Vec::new();
    }
    let mut out: Vec<String> = interface_addresses(inner.local_addr.port(), inner.local_addr.is_ipv4())
        .into_iter()
        .filter(|a| a.kind != "lan")
        .map(|a| a.address)
        .collect();
    for e in extra(inner) {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out.truncate(MAX_REACH);
    out
}

/// Loads what peers told us earlier.
pub(crate) fn load(inner: &Inner) {
    let ids: Vec<DeviceId> = inner.peers.read().keys().copied().collect();
    let mut map = inner.reach.peers.write();
    for id in ids {
        if let Some(list) = inner
            .store
            .setting(&peer_key(&id))
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        {
            map.insert(id, list.into_iter().filter(|e| valid_entry(e)).take(MAX_REACH).collect());
        }
    }
}

/// A session came up: tell the peer where we can be reached (or that we can't).
pub(crate) fn on_session(inner: &Arc<Inner>, peer: DeviceId) {
    let _ = inner.send_to(&peer, ControlMsg::Reach { addrs: own_reach(inner) });
}

/// A peer told us where it can be reached.
pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, addrs: Vec<String>) {
    let clean: Vec<String> = addrs.into_iter().filter(|e| valid_entry(e)).take(MAX_REACH).collect();
    let changed = inner.reach.of(&peer) != clean;
    if !changed {
        return;
    }
    if let Ok(json) = serde_json::to_string(&clean) {
        let _ = inner.store.set_setting(&peer_key(&peer), &json);
    }
    let name = inner.peer_name(&peer);
    inner.log(Some((&peer, &name)), "reach_updated", &clean.len().to_string());
    if clean.is_empty() {
        inner.reach.peers.write().remove(&peer);
    } else {
        inner.reach.peers.write().insert(peer, clean);
    }
    inner.emit(NodeEvent::ReachChanged { peer });
}

pub(crate) fn forget(inner: &Inner, peer: &DeviceId) {
    inner.reach.peers.write().remove(peer);
    let _ = inner.store.set_setting(&peer_key(peer), "[]");
}

/// Turns stored entries into socket addresses (DNS names are looked up now, so a
/// dynamic-DNS name follows its address), keeping the endpoint's address family.
pub(crate) async fn resolve(entries: &[String], ipv4: bool) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    for e in entries {
        let found: Vec<SocketAddr> = if let Ok(a) = e.parse::<SocketAddr>() {
            vec![a]
        } else {
            match tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host(e.as_str())).await {
                Ok(Ok(it)) => it.collect(),
                _ => Vec::new(),
            }
        };
        for a in found {
            if a.is_ipv4() == ipv4 && !out.contains(&a) {
                out.push(a);
            }
        }
    }
    out
}

impl Node {
    /// This device's remote-access settings and every address it was found at.
    pub fn reach_info(&self) -> ReachInfo {
        let inner = &self.inner;
        let mut addresses = interface_addresses(inner.local_addr.port(), inner.local_addr.is_ipv4());
        let extra = extra(inner);
        addresses.extend(extra.iter().map(|e| ReachAddress {
            address: e.clone(),
            kind: "manual".into(),
            interface: String::new(),
        }));
        ReachInfo { enabled: enabled(inner), port: inner.local_addr.port(), addresses, extra }
    }

    /// Turns remote access on or off and sets the typed addresses; tells online peers.
    pub fn set_reach(&self, on: bool, extra: Vec<String>) -> Result<()> {
        let inner = &self.inner;
        let extra: Vec<String> = extra.into_iter().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
        if extra.len() > MAX_EXTRA {
            return Err(Error::InvalidInput(format!("at most {MAX_EXTRA} addresses")));
        }
        if let Some(bad) = extra.iter().find(|e| !valid_entry(e)) {
            return Err(Error::InvalidInput(format!(
                "“{bad}” isn't an address Devti can dial — use host:port, for example home.example.org:{}",
                inner.local_addr.port()
            )));
        }
        inner.store.set_setting(SETTING_EXTRA, &serde_json::to_string(&extra)?)?;
        inner.store.set_setting(SETTING_ENABLED, if on { "true" } else { "false" })?;
        inner.log(None, if on { "remote_access_on" } else { "remote_access_off" }, "");
        let online: Vec<DeviceId> =
            inner.peers.read().iter().filter(|(_, p)| p.session.is_some()).map(|(id, _)| *id).collect();
        let addrs = own_reach(inner);
        for id in online {
            let _ = inner.send_to(&id, ControlMsg::Reach { addrs: addrs.clone() });
        }
        Ok(())
    }

    /// Where `peer` said it can be reached away from the local network.
    pub fn peer_reach(&self, peer: &DeviceId) -> Vec<String> {
        self.inner.reach.of(peer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_validated() {
        for ok in ["100.101.1.2:47810", "[fd7a:115c:a1e0::1]:47810", "home.example.org:47810", "nas:1"] {
            assert!(valid_entry(ok), "{ok}");
        }
        for bad in [
            "",
            "100.101.1.2",
            "100.101.1.2:0",
            "0.0.0.0:47810",
            "224.0.0.1:47810",
            "home example.org:47810",
            "-x.org:1",
            ".x.org:1",
            "a..b:1",
            "x.org:99999",
            "http://x.org:1",
            "x.org:1/path",
        ] {
            assert!(!valid_entry(bad), "{bad}");
        }
        assert!(!valid_entry(&format!("{}:1", "a".repeat(300))));
    }

    #[test]
    fn connection_kinds_follow_the_address() {
        let k = |s: &str| ConnectionKind::of(&s.parse().unwrap());
        assert_eq!(k("192.168.1.5:1"), ConnectionKind::Lan);
        assert_eq!(k("10.0.0.2:1"), ConnectionKind::Lan);
        assert_eq!(k("127.0.0.1:1"), ConnectionKind::Lan);
        assert_eq!(k("100.101.102.103:1"), ConnectionKind::Vpn);
        assert_eq!(k("100.63.0.1:1"), ConnectionKind::Internet);
        assert_eq!(k("8.8.8.8:1"), ConnectionKind::Internet);
        assert_eq!(k("[fd7a:115c:a1e0::5]:1"), ConnectionKind::Vpn);
        assert_eq!(k("[fd00::5]:1"), ConnectionKind::Lan);
        assert_eq!(k("[fe80::1]:1"), ConnectionKind::Lan);
        assert_eq!(k("[2001:db8::1]:1"), ConnectionKind::Internet);
    }

    #[tokio::test]
    async fn resolving_keeps_the_family_and_skips_failures() {
        let entries = vec![
            "127.0.0.1:47810".to_string(),
            "[::1]:47810".to_string(),
            "localhost:47811".to_string(),
            "no-such-host.invalid:1".to_string(),
        ];
        let v4 = resolve(&entries, true).await;
        assert!(v4.contains(&"127.0.0.1:47810".parse().unwrap()));
        assert!(v4.iter().all(|a| a.is_ipv4()));
        assert!(v4.iter().any(|a| a.port() == 47811), "localhost resolves");
        assert!(!v4.iter().any(|a| a.port() == 1));
    }
}
