//! Connect from anywhere (D-54).
//!
//! Next to its own QUIC endpoint for the local network, every node runs an
//! [iroh](https://iroh.computer) endpoint with the same Ed25519 key pair:
//!
//! * Devices find each other by public key through iroh's address lookup.
//! * iroh punches a direct path through both routers when it can, including mobile data
//!   and carrier-grade NAT.
//! * When it can't, traffic goes through an encrypted relay: iroh's free public relays,
//!   or the user's own relay set in Settings. A relay forwards encrypted QUIC packets and
//!   can't read them.
//!
//! Admission is the same as on the local network. A peer's public key gives its
//! fingerprint, so only paired devices (or a device pairing during an open window) get
//! past the first stream.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::link::{Link, fingerprint_of_key};
use super::{Inner, Node, NodeEvent};
use crate::error::{Error, Result};
use crate::model::{DeviceId, Fingerprint};
use crate::protocol::close_code;
use crate::tls::ClientPolicy;

/// ALPN of Devti's protocol on the anywhere endpoint.
pub const ALPN: &[u8] = b"devti/1";
const SETTING_ENABLED: &str = "anywhere_enabled";
const SETTING_RELAY: &str = "anywhere_relay";
const DIAL_TIMEOUT: Duration = Duration::from_secs(20);

fn key_setting(id: &DeviceId) -> String {
    format!("netkey:{id}")
}

fn relay_setting(id: &DeviceId) -> String {
    format!("netrelay:{id}")
}

pub(crate) struct State {
    endpoint: RwLock<Option<iroh::Endpoint>>,
    keys: RwLock<HashMap<DeviceId, [u8; 32]>>,
    relays: RwLock<HashMap<DeviceId, String>>,
    error: RwLock<Option<String>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            endpoint: RwLock::new(None),
            keys: RwLock::new(HashMap::new()),
            relays: RwLock::new(HashMap::new()),
            error: RwLock::new(None),
        }
    }

    pub fn endpoint(&self) -> Option<iroh::Endpoint> {
        self.endpoint.read().clone()
    }

    pub fn key_of(&self, id: &DeviceId) -> Option<[u8; 32]> {
        self.keys.read().get(id).copied()
    }
}

/// What Settings shows about connecting from anywhere.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnywhereInfo {
    pub enabled: bool,
    /// The endpoint is running.
    pub running: bool,
    /// Connected to a relay, so devices can find this one from anywhere.
    pub online: bool,
    /// Host name of the relay this device is reachable through.
    pub relay: Option<String>,
    /// The user's own relay URL (empty: iroh's public relays).
    pub custom_relay: String,
    /// This network's public address, as seen from the internet.
    pub public_ip: Option<String>,
    /// Why the endpoint isn't running, when it should be.
    pub error: Option<String>,
}

pub(crate) fn enabled(inner: &Inner) -> bool {
    !matches!(inner.anywhere_mode, super::AnywhereMode::Off)
        && inner.store.setting(SETTING_ENABLED).ok().flatten().as_deref() != Some("false")
}

fn custom_relay(inner: &Inner) -> String {
    inner.store.setting(SETTING_RELAY).ok().flatten().unwrap_or_default()
}

/// Loads the keys and relay hints stored for paired devices.
pub(crate) fn load(inner: &Inner) {
    let peers: Vec<(DeviceId, Fingerprint)> =
        inner.peers.read().iter().map(|(id, p)| (*id, p.record.fingerprint)).collect();
    for (id, fp) in peers {
        if let Some(key) = inner
            .store
            .setting(&key_setting(&id))
            .ok()
            .flatten()
            .and_then(|h| data_encoding::HEXLOWER.decode(h.as_bytes()).ok())
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            && fingerprint_of_key(&key) == fp
        {
            inner.anywhere.keys.write().insert(id, key);
        }
        if let Some(r) = inner.store.setting(&relay_setting(&id)).ok().flatten().filter(|r| valid_relay(r)) {
            inner.anywhere.relays.write().insert(id, r);
        }
    }
}

/// Starts the endpoint (when enabled) in the background; the node works without it.
pub(crate) fn start(inner: &Arc<Inner>) {
    if !enabled(inner) {
        return;
    }
    let inner = inner.clone();
    let rt = inner.rt.clone();
    rt.spawn(async move {
        match bind(&inner).await {
            Ok(ep) => {
                tracing::info!(id = %ep.id(), "connect-from-anywhere endpoint started");
                *inner.anywhere.error.write() = None;
                *inner.anywhere.endpoint.write() = Some(ep.clone());
                inner.rt.spawn(accept_loop(inner.clone(), ep.clone()));
                // Peers waiting on a key we already know can be dialled now.
                inner.wake_reconnect();
                // Tell the UI once the relay connection is up.
                let watcher = inner.clone();
                inner.rt.spawn(async move {
                    if tokio::time::timeout(Duration::from_secs(30), ep.online()).await.is_ok() {
                        watcher.emit(NodeEvent::AnywhereChanged);
                        super::reach::announce(&watcher);
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "connect-from-anywhere unavailable; local network still works");
                *inner.anywhere.error.write() = Some(e.to_string());
            }
        }
        inner.emit(NodeEvent::AnywhereChanged);
    });
}

async fn bind(inner: &Arc<Inner>) -> Result<iroh::Endpoint> {
    let secret = iroh::SecretKey::from_bytes(&inner.identity.ed25519_seed()?);
    let mut builder =
        iroh::Endpoint::builder(iroh::endpoint::presets::N0).secret_key(secret).alpns(vec![ALPN.to_vec()]);
    let custom = custom_relay(inner);
    if !custom.is_empty() {
        let url: iroh::RelayUrl = custom.parse().map_err(|_| Error::InvalidInput("relay address".into()))?;
        builder = builder.relay_mode(iroh::RelayMode::Custom(iroh::RelayMap::from(url)));
    }
    if let super::AnywhereMode::Custom(customise) = &inner.anywhere_mode {
        builder = customise(builder);
    }
    builder.bind().await.map_err(|e| Error::Connection(format!("could not start: {e}")))
}

/// Stops the endpoint (when turned off, or before restarting with another relay).
async fn stop(inner: &Arc<Inner>) {
    let ep = inner.anywhere.endpoint.write().take();
    if let Some(ep) = ep {
        ep.close().await;
    }
}

async fn accept_loop(inner: Arc<Inner>, ep: iroh::Endpoint) {
    let mut shutdown = inner.shutdown.subscribe();
    loop {
        let incoming = tokio::select! {
            i = ep.accept() => i,
            _ = shutdown.changed() => break,
        };
        let Some(incoming) = incoming else { break };
        if inner.paused.load(Ordering::SeqCst) {
            incoming.refuse();
            continue;
        }
        let inner = inner.clone();
        let rt = inner.rt.clone();
        rt.spawn(async move {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!(error = %e, "anywhere handshake failed");
                    return;
                }
            };
            let key = *conn.remote_id().as_bytes();
            let fp = fingerprint_of_key(&key);
            let link = Link::Iroh(conn);
            // The same admission rule the TLS verifier applies on the local network.
            if !inner.trust.allow(&fp) {
                link.close(close_code::UNTRUSTED, b"not paired");
                return;
            }
            super::session::serve_connection(inner, link, fp).await;
        });
    }
}

/// Remembers a paired device's public key (learned from its certificate or its
/// anywhere connection) once it's checked against the pinned fingerprint.
pub(crate) fn learn_key(inner: &Inner, peer: &DeviceId, key: [u8; 32], fp: &Fingerprint) {
    if fingerprint_of_key(&key) != *fp || inner.anywhere.key_of(peer) == Some(key) {
        return;
    }
    inner.anywhere.keys.write().insert(*peer, key);
    let _ = inner.store.set_setting(&key_setting(peer), &data_encoding::HEXLOWER.encode(&key));
}

pub(crate) fn forget(inner: &Inner, peer: &DeviceId) {
    inner.anywhere.keys.write().remove(peer);
    inner.anywhere.relays.write().remove(peer);
    let _ = inner.store.set_setting(&key_setting(peer), "");
    let _ = inner.store.set_setting(&relay_setting(peer), "");
}

fn valid_relay(r: &str) -> bool {
    r.len() <= 200 && (r.starts_with("https://") || r.starts_with("http://")) && r.parse::<iroh::RelayUrl>().is_ok()
}

/// The relay this device can be reached through, sent to peers as a dialling hint.
pub(crate) fn home_relay(inner: &Inner) -> Option<String> {
    let ep = inner.anywhere.endpoint()?;
    ep.addr().relay_urls().next().map(|u| u.to_string())
}

/// A peer told us which relay it's reachable through.
pub(crate) fn on_relay_hint(inner: &Inner, peer: &DeviceId, relay: Option<String>) {
    let Some(r) = relay.filter(|r| valid_relay(r)) else { return };
    if inner.anywhere.relays.read().get(peer) == Some(&r) {
        return;
    }
    let _ = inner.store.set_setting(&relay_setting(peer), &r);
    inner.anywhere.relays.write().insert(*peer, r);
}

/// Whether `peer` can be dialled from anywhere right now.
pub(crate) fn can_dial(inner: &Inner, peer: &DeviceId) -> bool {
    inner.anywhere.endpoint.read().is_some() && inner.anywhere.key_of(peer).is_some()
}

/// Dials a device by its public key, wherever it is.
pub(crate) async fn dial_key(inner: &Arc<Inner>, key: [u8; 32], relay: Option<String>) -> Result<Link> {
    let ep = inner.anywhere.endpoint().ok_or_else(|| Error::Connection("connecting from anywhere is off".into()))?;
    let id = iroh::PublicKey::from_bytes(&key).map_err(|_| Error::Connection("bad device key".into()))?;
    let mut addr = iroh::EndpointAddr::new(id);
    if let Some(url) = relay.and_then(|r| r.parse::<iroh::RelayUrl>().ok()) {
        addr = addr.with_relay_url(url);
    }
    let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, ALPN))
        .await
        .map_err(|_| Error::Timeout("no answer over the internet".into()))?
        .map_err(|e| Error::Connection(e.to_string()))?;
    Ok(Link::Iroh(conn))
}

/// Dials a paired device from anywhere.
pub(crate) async fn dial(inner: &Arc<Inner>, peer: &DeviceId) -> Result<Link> {
    let key = inner.anywhere.key_of(peer).ok_or_else(|| Error::Connection("no key for that device yet".into()))?;
    let relay = inner.anywhere.relays.read().get(peer).cloned();
    dial_key(inner, key, relay).await
}

impl Inner {
    /// Retries offline devices now.
    pub(crate) fn wake_reconnect(&self) {
        let now = std::time::Instant::now();
        for p in self.peers.write().values_mut() {
            if p.session.is_none() {
                p.attempts = 0;
                p.next_attempt = now;
            }
        }
    }
}

impl Node {
    /// Status of connecting from anywhere.
    pub fn anywhere_info(&self) -> AnywhereInfo {
        let inner = &self.inner;
        let ep = inner.anywhere.endpoint();
        let (online, relay, public_ip) = match &ep {
            Some(ep) => {
                use iroh::Watcher;
                let relays = ep.home_relay_status().get();
                let online = relays.iter().any(|r| r.is_connected());
                let relay = relays.first().and_then(|r| r.url().host_str().map(str::to_string));
                // The address iroh found this network at from the outside (QUIC address discovery).
                let ip = ep
                    .addr()
                    .ip_addrs()
                    .find(|a| crate::model::ConnectionKind::of(a) == crate::model::ConnectionKind::Internet)
                    .map(|a| a.ip().to_string());
                (online, relay, ip)
            }
            None => (false, None, None),
        };
        AnywhereInfo {
            enabled: enabled(inner),
            running: ep.is_some(),
            online,
            relay,
            custom_relay: custom_relay(inner),
            public_ip,
            error: inner.anywhere.error.read().clone(),
        }
    }

    /// This device's Ed25519 public key: its address when connecting from anywhere.
    pub fn device_key(&self) -> Option<[u8; 32]> {
        super::link::key_of_cert(self.inner.identity.cert_der().as_ref())
    }

    /// This network's public IP address, when the anywhere endpoint has measured it.
    pub fn public_ip(&self) -> Option<String> {
        self.anywhere_info().public_ip
    }

    /// Turns connecting from anywhere on or off, and sets the relay (empty: iroh's free
    /// public relays). Restarts the endpoint when needed.
    pub async fn set_anywhere(&self, on: bool, relay: &str) -> Result<()> {
        let relay = relay.trim().trim_end_matches('/').to_string();
        if !relay.is_empty() && !valid_relay(&format!("{relay}/")) && !valid_relay(&relay) {
            return Err(Error::InvalidInput("the relay address looks like https://relay.example.org".into()));
        }
        let inner = &self.inner;
        inner.store.set_setting(SETTING_ENABLED, if on { "true" } else { "false" })?;
        inner.store.set_setting(SETTING_RELAY, &relay)?;
        inner.log(None, if on { "anywhere_on" } else { "anywhere_off" }, &relay);
        stop(inner).await;
        if on {
            start(inner);
        } else {
            inner.emit(NodeEvent::AnywhereChanged);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_urls_are_validated() {
        assert!(valid_relay("https://relay.example.org/"));
        assert!(valid_relay("https://euc1-1.relay.n0.iroh.iroh.link./"));
        assert!(!valid_relay("relay.example.org"));
        assert!(!valid_relay("ftp://relay.example.org"));
        assert!(!valid_relay(&format!("https://{}.org", "a".repeat(300))));
    }
}
