//! End-to-end tests: real nodes, real QUIC, real TLS, real PAKE — over loopback.

use std::sync::Arc;
use std::time::Duration;

use devti_core::model::{BatteryState, DeviceKind, DeviceStatus, Permissions, TrustRole};
use devti_core::node::{NodeEvent, PairingRole, RemovalReason};
use devti_core::protocol::PairAbortReason;
use devti_core::secret::MemorySecretStore;
use devti_core::{Node, NodeConfig};
use tokio::sync::broadcast::Receiver;

const WAIT: Duration = Duration::from_secs(15);

struct TestNode {
    node: Node,
    events: Receiver<NodeEvent>,
    _dir: Option<tempfile::TempDir>,
}

async fn start_in(
    dir: &std::path::Path,
    name: &str,
    kind: DeviceKind,
    secrets: Arc<MemorySecretStore>,
) -> (Node, Receiver<NodeEvent>) {
    let mut cfg = NodeConfig::new(dir, name, kind);
    cfg.bind_ip = "127.0.0.1".parse().unwrap();
    cfg.port = 0;
    cfg.discovery = false;
    let node = Node::start(cfg, secrets).await.expect("node starts");
    let events = node.subscribe();
    (node, events)
}

async fn spawn(name: &str, kind: DeviceKind) -> TestNode {
    let dir = tempfile::tempdir().unwrap();
    let (node, events) = start_in(dir.path(), name, kind, Arc::new(MemorySecretStore::new())).await;
    TestNode { node, events, _dir: Some(dir) }
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

async fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !f() {
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Runs the whole QR ceremony and waits until both sides are connected.
async fn pair(hub: &mut TestNode, joiner: &mut TestNode) {
    let window = hub.node.open_pairing_window().unwrap();
    let jid = joiner.node.pair_with_uri(&window.uri).unwrap();

    let hub_sas = wait_for(&mut hub.events, "hub verify", |e| match e {
        NodeEvent::PairingVerify { pairing_id, role: PairingRole::Hub, sas, .. } => Some((*pairing_id, sas.clone())),
        _ => None,
    })
    .await;
    let joiner_sas = wait_for(&mut joiner.events, "joiner verify", |e| match e {
        NodeEvent::PairingVerify { pairing_id, role: PairingRole::Joiner, sas, .. } if *pairing_id == jid => {
            Some(sas.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(hub_sas.1, joiner_sas, "both screens must show the same code");
    assert_eq!(hub_sas.1.len(), 7, "formatted as 'ddd ddd'");

    hub.node.confirm_pairing(hub_sas.0, true).unwrap();
    joiner.node.confirm_pairing(jid, true).unwrap();

    wait_for(&mut hub.events, "hub completed", |e| matches!(e, NodeEvent::PairingCompleted { .. }).then_some(())).await;
    wait_for(&mut joiner.events, "joiner completed", |e| matches!(e, NodeEvent::PairingCompleted { .. }).then_some(()))
        .await;
    let (h, j) = (hub.node.clone(), joiner.node.clone());
    let (hid, jid) = (h.device_id(), j.device_id());
    eventually("both online", || h.peer(&jid).is_some_and(|p| p.online) && j.peer(&hid).is_some_and(|p| p.online))
        .await;
}

#[tokio::test]
async fn qr_pairing_connects_and_syncs_state() {
    let mut hub = spawn("Studio PC", DeviceKind::Desktop).await;
    let mut phone = spawn("Pixel", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;

    // Window closes after a successful pairing.
    assert!(hub.node.pairing_window().is_none());

    // Both sides know each other with full details.
    let p = hub.node.peer(&phone.node.device_id()).unwrap();
    assert_eq!(p.info.name, "Pixel");
    assert_eq!(p.info.kind, DeviceKind::Phone);
    assert_eq!(p.role, TrustRole::Trusted);
    assert!(p.connection.is_some());
    let h = phone.node.peer(&hub.node.device_id()).unwrap();
    assert_eq!(h.info.name, "Studio PC");

    // Battery travels to the hub.
    phone.node.set_status(DeviceStatus { battery: Some(BatteryState::new(42, true)) });
    let pid = phone.node.device_id();
    let pct = wait_for(&mut hub.events, "battery", |e| match e {
        NodeEvent::PeerChanged { peer } if peer.info.id == pid => peer.status.battery.map(|b| b.percent),
        _ => None,
    })
    .await;
    assert_eq!(pct, 42);

    // Rename travels too.
    phone.node.rename("Hussnain's Pixel").unwrap();
    let name = wait_for(&mut hub.events, "rename", |e| match e {
        NodeEvent::PeerChanged { peer } if peer.info.name != "Pixel" => Some(peer.info.name.clone()),
        _ => None,
    })
    .await;
    assert_eq!(name, "Hussnain's Pixel");
    assert_eq!(phone.node.local().info.name, "Hussnain's Pixel");

    // The activity log recorded the story.
    let log = hub.node.activity(None, 50, None).unwrap();
    assert!(log.iter().any(|e| e.action == "paired"));
    assert!(log.iter().any(|e| e.action == "connected"));
}

#[tokio::test]
async fn code_pairing_at_address_works() {
    let mut hub = spawn("Laptop A", DeviceKind::Laptop).await;
    let mut other = spawn("Laptop B", DeviceKind::Laptop).await;
    let window = hub.node.open_pairing_window().unwrap();
    let id = other.node.pair_with_code_at_address(hub.node.local_addr(), &window.code).unwrap();
    let pid = wait_for(&mut hub.events, "hub verify", |e| match e {
        NodeEvent::PairingVerify { pairing_id, .. } => Some(*pairing_id),
        _ => None,
    })
    .await;
    wait_for(&mut other.events, "joiner verify", |e| matches!(e, NodeEvent::PairingVerify { .. }).then_some(())).await;
    other.node.confirm_pairing(id, true).unwrap();
    hub.node.confirm_pairing(pid, true).unwrap();
    wait_for(&mut other.events, "joiner completed", |e| matches!(e, NodeEvent::PairingCompleted { .. }).then_some(()))
        .await;
    assert_eq!(other.node.peers().len(), 1);
}

#[tokio::test]
async fn wrong_codes_fail_and_burn_the_window() {
    let mut hub = spawn("Hub", DeviceKind::Desktop).await;
    let mut intruder = spawn("Intruder", DeviceKind::Laptop).await;
    let window = hub.node.open_pairing_window().unwrap();
    let real: String = window.code.chars().filter(|c| c.is_ascii_digit()).collect();
    let wrong = if real == "000000" { "111111" } else { "000000" };

    for attempt in 1..=3 {
        let id = intruder.node.pair_with_code_at_address(hub.node.local_addr(), wrong).unwrap();
        let reason = wait_for(&mut intruder.events, "failure", |e| match e {
            NodeEvent::PairingFailed { pairing_id, reason, .. } if *pairing_id == id => Some(*reason),
            _ => None,
        })
        .await;
        assert_eq!(reason, PairAbortReason::BadSecret, "attempt {attempt}");
    }
    wait_for(&mut hub.events, "window burned", |e| {
        matches!(e, NodeEvent::PairingWindowChanged { window: None }).then_some(())
    })
    .await;
    assert!(hub.node.pairing_window().is_none());
    assert!(hub.node.peers().is_empty());
    assert!(intruder.node.peers().is_empty());

    // With the window closed even the right code can't get in.
    let id = intruder.node.pair_with_code_at_address(hub.node.local_addr(), &real).unwrap();
    wait_for(&mut intruder.events, "refused", |e| match e {
        NodeEvent::PairingFailed { pairing_id, .. } if *pairing_id == id => Some(()),
        _ => None,
    })
    .await;
}

#[tokio::test]
async fn rejecting_the_code_pairs_nobody() {
    let mut hub = spawn("Hub", DeviceKind::Desktop).await;
    let mut phone = spawn("Phone", DeviceKind::Phone).await;
    let window = hub.node.open_pairing_window().unwrap();
    let jid = phone.node.pair_with_uri(&window.uri).unwrap();
    let hid = wait_for(&mut hub.events, "verify", |e| match e {
        NodeEvent::PairingVerify { pairing_id, .. } => Some(*pairing_id),
        _ => None,
    })
    .await;
    wait_for(&mut phone.events, "verify", |e| matches!(e, NodeEvent::PairingVerify { .. }).then_some(())).await;
    hub.node.confirm_pairing(hid, false).unwrap();
    let reason = wait_for(&mut phone.events, "rejected", |e| match e {
        NodeEvent::PairingFailed { pairing_id, reason, .. } if *pairing_id == jid => Some(*reason),
        _ => None,
    })
    .await;
    assert_eq!(reason, PairAbortReason::Rejected);
    assert!(hub.node.peers().is_empty());
    assert!(phone.node.peers().is_empty());
    assert!(hub.node.confirm_pairing(hid, true).is_err(), "decision slot is consumed");
}

#[tokio::test]
async fn qr_with_wrong_fingerprint_is_refused_by_tls() {
    let hub = spawn("Hub", DeviceKind::Desktop).await;
    let impostor = spawn("Impostor", DeviceKind::Desktop).await;
    let mut phone = spawn("Phone", DeviceKind::Phone).await;

    // A QR that claims to be the impostor but points at the real hub's address.
    let window = hub.node.open_pairing_window().unwrap();
    let mut forged = devti_core::pairing::PairingUri::parse(&window.uri).unwrap();
    let impostor_window = impostor.node.open_pairing_window().unwrap();
    let impostor_uri = devti_core::pairing::PairingUri::parse(&impostor_window.uri).unwrap();
    forged.fingerprint = impostor_uri.fingerprint;
    forged.hub_id = impostor_uri.hub_id;

    let id = phone.node.pair_with_uri(&forged.to_uri()).unwrap();
    wait_for(&mut phone.events, "tls refusal", |e| match e {
        NodeEvent::PairingFailed { pairing_id, .. } if *pairing_id == id => Some(()),
        _ => None,
    })
    .await;
    assert!(phone.node.peers().is_empty());
}

#[tokio::test]
async fn revoking_disconnects_and_removes_on_both_sides() {
    let mut hub = spawn("Hub", DeviceKind::Desktop).await;
    let mut phone = spawn("Phone", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let pid = phone.node.device_id();
    hub.node.revoke(&pid).unwrap();
    let reason = wait_for(&mut phone.events, "removed by peer", |e| match e {
        NodeEvent::PeerRemoved { reason, .. } => Some(*reason),
        _ => None,
    })
    .await;
    assert_eq!(reason, RemovalReason::RevokedByPeer);
    assert!(hub.node.peers().is_empty());
    assert!(phone.node.peers().is_empty());
    assert!(hub.node.revoke(&pid).is_err());
}

#[tokio::test]
async fn reconnects_after_restart() {
    let hub_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();
    let hub_secrets = Arc::new(MemorySecretStore::new());
    let phone_secrets = Arc::new(MemorySecretStore::new());
    let (h, he) = start_in(hub_dir.path(), "Hub", DeviceKind::Desktop, hub_secrets.clone()).await;
    let (p, pe) = start_in(phone_dir.path(), "Phone", DeviceKind::Phone, phone_secrets.clone()).await;
    let mut hub = TestNode { node: h, events: he, _dir: None };
    let mut phone = TestNode { node: p, events: pe, _dir: None };
    pair(&mut hub, &mut phone).await;
    let pid = phone.node.device_id();

    phone.node.shutdown().await;
    let hub_node = hub.node.clone();
    eventually("hub sees phone offline", || hub_node.peer(&pid).is_some_and(|p| !p.online)).await;

    // Same identity, same database, new port: the phone dials the hub's known address.
    let (phone2, _ev) = start_in(phone_dir.path(), "Phone", DeviceKind::Phone, phone_secrets).await;
    assert_eq!(phone2.device_id(), pid);
    assert_eq!(phone2.peers().len(), 1);
    eventually("reconnected", || hub_node.peer(&pid).is_some_and(|p| p.online)).await;
    let hid = hub.node.device_id();
    eventually("phone sees hub", || phone2.peer(&hid).is_some_and(|p| p.online)).await;
}

#[tokio::test]
async fn kill_switch_pauses_and_resumes() {
    let mut hub = spawn("Hub", DeviceKind::Desktop).await;
    let mut phone = spawn("Phone", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let (hid, pid) = (hub.node.device_id(), phone.node.device_id());

    hub.node.set_paused(true).unwrap();
    assert!(hub.node.is_paused());
    assert!(hub.node.open_pairing_window().is_err());
    let p = phone.node.clone();
    eventually("phone sees hub offline", || p.peer(&hid).is_some_and(|x| !x.online)).await;
    // Stays offline while paused, even though the phone keeps retrying.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!phone.node.peer(&hid).unwrap().online);

    hub.node.set_paused(false).unwrap();
    phone.node.reconnect_now();
    let h = hub.node.clone();
    eventually("back online", || h.peer(&pid).is_some_and(|x| x.online)).await;
}

#[tokio::test]
async fn permissions_can_be_tuned_and_persist() {
    let mut hub = spawn("Hub", DeviceKind::Desktop).await;
    let mut phone = spawn("Phone", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let pid = phone.node.device_id();
    let mut perms = Permissions::for_role(TrustRole::Restricted);
    perms.screen_view = true;
    let snap = hub.node.set_access(&pid, TrustRole::Restricted, perms).unwrap();
    assert_eq!(snap.role, TrustRole::Restricted);
    assert!(snap.permissions.screen_view && !snap.permissions.control);
    assert_eq!(hub.node.peer(&pid).unwrap().permissions, perms);
    let unknown = devti_core::model::DeviceId::from_bytes([0; 10]);
    assert!(hub.node.set_access(&unknown, TrustRole::Trusted, perms).is_err());
}

#[tokio::test]
async fn strangers_cannot_open_sessions() {
    let hub = spawn("Hub", DeviceKind::Desktop).await;
    let mut stranger = spawn("Stranger", DeviceKind::Laptop).await;
    // Grab a valid link, then close the window: the link is now worthless.
    let window = hub.node.open_pairing_window().unwrap();
    hub.node.close_pairing_window();
    let id = stranger.node.pair_with_uri(&window.uri).unwrap();
    wait_for(&mut stranger.events, "refusal", |e| match e {
        NodeEvent::PairingFailed { pairing_id, .. } if *pairing_id == id => Some(()),
        _ => None,
    })
    .await;
    assert!(hub.node.peers().is_empty());
    assert!(stranger.node.confirm_pairing(id, true).is_err());
}

#[tokio::test]
async fn local_snapshot_and_settings() {
    let n = spawn("  Desk\u{7} ", DeviceKind::Desktop).await;
    let local = n.node.local();
    assert_eq!(local.info.name, "Desk");
    assert_eq!(local.identity_origin, "created");
    assert!(!local.paused);
    assert_eq!(local.addresses, vec!["127.0.0.1".to_string()]);
    n.node.set_setting("theme", "dark").unwrap();
    assert_eq!(n.node.setting("theme").unwrap().as_deref(), Some("dark"));
    assert!(n.node.settings().unwrap().iter().any(|(k, v)| k == "theme" && v == "dark"));
    assert!(n.node.nearby().is_empty());
    assert!(n.node.pair_with_uri("devti://nope").is_err());
    assert!(n.node.pair_with_code_at_address(n.node.local_addr(), "12").is_err());
    assert!(n.node.pair_with_code(&devti_core::model::DeviceId::from_bytes([1; 10]), "123456").is_err());
}

#[test]
fn api_is_callable_from_threads_outside_the_runtime() {
    // UI frameworks (Tauri sync commands, Android main thread) call the node from
    // plain threads. Nothing may assume an ambient Tokio runtime.
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let node = rt.block_on(async {
        let mut cfg = NodeConfig::new(dir.path(), "Threaded", DeviceKind::Desktop);
        cfg.bind_ip = "127.0.0.1".parse().unwrap();
        cfg.port = 0;
        cfg.discovery = false;
        Node::start(cfg, Arc::new(MemorySecretStore::new())).await.unwrap()
    });
    let handle = std::thread::spawn(move || {
        let w = node.open_pairing_window().unwrap();
        node.close_pairing_window();
        assert!(node.pair_with_uri(&w.uri).is_ok());
        node.set_paused(true).unwrap();
        node.set_paused(false).unwrap();
        node.set_status(DeviceStatus { battery: Some(BatteryState::new(50, false)) });
        node.rename("Still fine").unwrap();
        node
    });
    let node = handle.join().expect("no panic outside the runtime");
    rt.block_on(node.shutdown());
}
