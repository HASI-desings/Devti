//! Connecting from anywhere: two nodes that can reach each other *only* through an iroh
//! relay (direct IP transports are switched off), pairing by QR code and reconnecting
//! after a restart, all over a local test relay.

use std::sync::Arc;
use std::time::Duration;

use devti_core::model::{ConnectionKind, DeviceKind};
use devti_core::node::{NodeEvent, PairingRole};
use devti_core::pairing::PairingUri;
use devti_core::secret::MemorySecretStore;
use devti_core::{AnywhereMode, Node, NodeConfig};
use iroh::address_lookup::MemoryLookup;
use iroh::{EndpointAddr, PublicKey, RelayMap, RelayMode, RelayUrl};
use tokio::sync::broadcast::Receiver;

const WAIT: Duration = Duration::from_secs(30);

struct Net {
    map: RelayMap,
    url: RelayUrl,
    lookup: MemoryLookup,
}

impl Net {
    /// Relay-only endpoints that find each other through a shared in-memory lookup.
    fn mode(&self) -> AnywhereMode {
        let (map, lookup) = (self.map.clone(), self.lookup.clone());
        AnywhereMode::Custom(Arc::new(move |b: iroh::endpoint::Builder| {
            b.clear_address_lookup()
                .address_lookup(lookup.clone())
                .relay_mode(RelayMode::Custom(map.clone()))
                .ca_tls_config(iroh::tls::CaTlsConfig::insecure_skip_verify())
                .clear_ip_transports()
        }))
    }

    fn publish(&self, node: &Node) {
        let key = PublicKey::from_bytes(&node.device_key().unwrap()).unwrap();
        self.lookup.add_endpoint_info(EndpointAddr::new(key).with_relay_url(self.url.clone()));
    }
}

async fn start(
    net: &Net,
    dir: &std::path::Path,
    name: &str,
    kind: DeviceKind,
    secrets: Arc<MemorySecretStore>,
) -> (Node, Receiver<NodeEvent>) {
    let mut cfg = NodeConfig::new(dir, name, kind);
    cfg.bind_ip = "127.0.0.1".parse().unwrap();
    cfg.port = 0;
    cfg.discovery = false;
    cfg.anywhere = net.mode();
    cfg.app_version = "1.1.0".into();
    let node = Node::start(cfg, secrets).await.unwrap();
    let mut ev = node.subscribe();
    let n = node.clone();
    eventually("anywhere endpoint online", || n.anywhere_info().online, &mut ev).await;
    net.publish(&node);
    (node, ev)
}

async fn eventually(what: &str, mut f: impl FnMut() -> bool, _ev: &mut Receiver<NodeEvent>) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !f() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for<T>(rx: &mut Receiver<NodeEvent>, what: &str, mut f: impl FnMut(&NodeEvent) -> Option<T>) -> T {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => {
                if let Some(v) = f(&ev) {
                    return v;
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(e)) => panic!("event channel closed while waiting for {what}: {e}"),
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pairs_and_reconnects_through_a_relay_only() {
    let (map, url, _relay) = iroh::test_utils::run_relay_server().await.unwrap();
    let net = Net { map, url, lookup: MemoryLookup::new() };

    let hub_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let phone_secrets = Arc::new(MemorySecretStore::new());
    let (hub, mut hub_ev) =
        start(&net, hub_dir.path(), "Laptop", DeviceKind::Laptop, Arc::new(MemorySecretStore::new())).await;
    let (phone, mut phone_ev) = start(&net, phone_dir.path(), "Phone", DeviceKind::Phone, phone_secrets.clone()).await;

    // The QR code carries the hub's key; strip its local addresses so only the relay works.
    let window = hub.open_pairing_window().unwrap();
    let mut uri = PairingUri::parse(&window.uri).unwrap();
    assert!(uri.key.is_some(), "the QR code carries the key when connecting from anywhere is on");
    uri.addrs.clear();
    let jid = phone.pair_with_uri(&uri.to_uri()).unwrap();

    let (hub_pid, hub_sas) = wait_for(&mut hub_ev, "hub verify", |e| match e {
        NodeEvent::PairingVerify { pairing_id, role: PairingRole::Hub, sas, .. } => Some((*pairing_id, sas.clone())),
        _ => None,
    })
    .await;
    let phone_sas = wait_for(&mut phone_ev, "phone verify", |e| match e {
        NodeEvent::PairingVerify { pairing_id, sas, .. } if *pairing_id == jid => Some(sas.clone()),
        _ => None,
    })
    .await;
    assert_eq!(hub_sas, phone_sas, "both screens show the same code over the relay");
    hub.confirm_pairing(hub_pid, true).unwrap();
    phone.confirm_pairing(jid, true).unwrap();

    let (hid, pid) = (hub.device_id(), phone.device_id());
    let (h, p) = (hub.clone(), phone.clone());
    eventually(
        "both online",
        || h.peer(&pid).is_some_and(|x| x.online) && p.peer(&hid).is_some_and(|x| x.online),
        &mut hub_ev,
    )
    .await;
    let conn = hub.peer(&pid).unwrap().connection.unwrap();
    assert_eq!(conn.kind, ConnectionKind::Relay, "travelled through the relay: {conn:?}");

    // Control messages flow: ring the phone from the laptop.
    hub.ring(&pid, true).unwrap();
    wait_for(&mut phone_ev, "ring", |e| matches!(e, NodeEvent::RingRequested { on: true, .. }).then_some(())).await;

    // Sound over the relay travels as Opus and arrives as PCM.
    let mut audio = hub.subscribe_audio();
    let sender = phone.start_audio(&hid, "relay", 48_000, 2).unwrap();
    assert!(sender.opus, "compressed away from the local network");
    let chunk: Vec<u8> = (0..960 * 2).flat_map(|i| (((i as f32 * 0.05).sin() * 8000.0) as i16).to_le_bytes()).collect();
    let pusher = tokio::spawn(async move {
        for _ in 0..30 {
            sender.push(chunk.clone());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        sender
    });
    let pkt = tokio::time::timeout(WAIT, audio.recv()).await.unwrap().unwrap();
    assert_eq!((pkt.sample_rate, pkt.channels), (48_000, 2));
    assert_eq!(pkt.pcm.len(), 960 * 2 * 2, "one decoded 20 ms frame of stereo PCM");
    drop(pusher.await.unwrap());

    // Restart the phone: it has no local address for the laptop, only its key.
    phone.shutdown().await;
    drop(phone);
    let h = hub.clone();
    eventually("laptop sees the phone leave", || h.peer(&pid).is_some_and(|x| !x.online), &mut hub_ev).await;
    let (phone, _ev) = start(&net, phone_dir.path(), "Phone", DeviceKind::Phone, phone_secrets).await;
    let (h, p) = (hub.clone(), phone.clone());
    eventually(
        "reconnected from anywhere",
        || h.peer(&pid).is_some_and(|x| x.online) && p.peer(&hid).is_some_and(|x| x.online),
        &mut hub_ev,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn strangers_are_refused_once_the_window_closes() {
    let (map, url, _relay) = iroh::test_utils::run_relay_server().await.unwrap();
    let net = Net { map, url, lookup: MemoryLookup::new() };
    let d1 = tempfile::tempdir().unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let (hub, _hub_ev) = start(&net, d1.path(), "Laptop", DeviceKind::Laptop, Arc::new(MemorySecretStore::new())).await;
    let (stranger, mut ev) =
        start(&net, d2.path(), "Stranger", DeviceKind::Phone, Arc::new(MemorySecretStore::new())).await;

    let window = hub.open_pairing_window().unwrap();
    hub.close_pairing_window();
    let mut uri = PairingUri::parse(&window.uri).unwrap();
    uri.addrs.clear();
    stranger.pair_with_uri(&uri.to_uri()).unwrap();
    wait_for(&mut ev, "pairing refused", |e| matches!(e, NodeEvent::PairingFailed { .. }).then_some(())).await;
    assert!(hub.peers().is_empty());
}
