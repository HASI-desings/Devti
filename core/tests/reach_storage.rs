//! Phase 7 end-to-end tests: remote addresses travel between devices, and shared folders
//! can be browsed and downloaded only within what the owner shares and allows — real QUIC.

use std::sync::Arc;
use std::time::Duration;

use devti_core::model::{DeviceKind, Permissions, TrustRole};
use devti_core::node::{NodeEvent, PairingRole, SharedFolder};
use devti_core::secret::MemorySecretStore;
use devti_core::{Node, NodeConfig};
use tokio::sync::broadcast::Receiver;

const WAIT: Duration = Duration::from_secs(20);

struct T {
    node: Node,
    ev: Receiver<NodeEvent>,
    dir: tempfile::TempDir,
}

async fn spawn(name: &str, kind: DeviceKind) -> T {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = NodeConfig::new(dir.path(), name, kind);
    cfg.bind_ip = "127.0.0.1".parse().unwrap();
    cfg.port = 0;
    cfg.discovery = false;
    cfg.anywhere = devti_core::AnywhereMode::Off;
    cfg.downloads_dir = dir.path().join("downloads");
    let node = Node::start(cfg, Arc::new(MemorySecretStore::new())).await.unwrap();
    let ev = node.subscribe();
    T { node, ev, dir }
}

async fn wait_for<R>(rx: &mut Receiver<NodeEvent>, what: &str, mut f: impl FnMut(&NodeEvent) -> Option<R>) -> R {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Ok(e)) => {
                if let Some(v) = f(&e) {
                    return v;
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(e)) => panic!("channel closed waiting for {what}: {e}"),
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

async fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !f() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn pair(hub: &mut T, other: &mut T) {
    let w = hub.node.open_pairing_window().unwrap();
    let jid = other.node.pair_with_uri(&w.uri).unwrap();
    let hid = wait_for(&mut hub.ev, "hub verify", |e| match e {
        NodeEvent::PairingVerify { pairing_id, role: PairingRole::Hub, .. } => Some(*pairing_id),
        _ => None,
    })
    .await;
    wait_for(&mut other.ev, "joiner verify", |e| matches!(e, NodeEvent::PairingVerify { .. }).then_some(())).await;
    hub.node.confirm_pairing(hid, true).unwrap();
    other.node.confirm_pairing(jid, true).unwrap();
    let (h, p) = (hub.node.clone(), other.node.clone());
    let (hid, pid) = (h.device_id(), p.device_id());
    eventually("online", || h.peer(&pid).is_some_and(|x| x.online) && p.peer(&hid).is_some_and(|x| x.online)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_addresses_reach_paired_devices() {
    let mut desk = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut desk, &mut phone).await;
    let did = desk.node.device_id();

    // Off by default: nothing is advertised.
    assert!(!desk.node.reach_info().enabled);
    assert!(phone.node.peer_reach(&did).is_empty());

    // Typed addresses are validated…
    assert!(desk.node.set_reach(true, vec!["not an address".into()]).is_err());
    // …and valid ones reach the phone at once.
    desk.node.set_reach(true, vec!["home.example.org:47810".into()]).unwrap();
    wait_for(&mut phone.ev, "reach", |e| matches!(e, NodeEvent::ReachChanged { peer } if *peer == did).then_some(()))
        .await;
    assert!(phone.node.peer_reach(&did).contains(&"home.example.org:47810".to_string()));
    let info = desk.node.reach_info();
    assert!(info.enabled);
    assert!(info.addresses.iter().any(|a| a.kind == "manual" && a.address == "home.example.org:47810"));

    // Turning it off clears what the phone keeps.
    desk.node.set_reach(false, vec![]).unwrap();
    wait_for(&mut phone.ev, "reach cleared", |e| matches!(e, NodeEvent::ReachChanged { .. }).then_some(())).await;
    assert!(phone.node.peer_reach(&did).is_empty());

    desk.node.shutdown().await;
    phone.node.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_folders_browse_and_download_within_bounds() {
    let mut desk = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut desk, &mut phone).await;
    let (did, pid) = (desk.node.device_id(), phone.node.device_id());

    let docs = desk.dir.path().join("Documents");
    std::fs::create_dir_all(docs.join("Trips")).unwrap();
    std::fs::write(docs.join("plan.txt"), b"the plan").unwrap();
    std::fs::write(desk.dir.path().join("private.txt"), b"not shared").unwrap();

    // Nothing is shared yet.
    phone.node.fs_list(&did, "/").unwrap();
    let empty = wait_for(&mut phone.ev, "root", |e| match e {
        NodeEvent::FsListing { path, entries, .. } if path == "/" => Some(entries.clone()),
        _ => None,
    })
    .await;
    assert!(empty.is_empty());

    assert!(
        desk.node
            .set_shared_folders(vec![SharedFolder { name: "../x".into(), path: docs.to_string_lossy().into() }])
            .is_err()
    );
    desk.node
        .set_shared_folders(vec![SharedFolder { name: "Documents".into(), path: docs.to_string_lossy().into() }])
        .unwrap();

    phone.node.fs_list(&did, "/Documents").unwrap();
    let entries = wait_for(&mut phone.ev, "listing", |e| match e {
        NodeEvent::FsListing { path, entries, error: None, .. } if path == "/Documents" => Some(entries.clone()),
        _ => None,
    })
    .await;
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Trips", "plan.txt"]);

    // Climbing out is refused with a reason.
    phone.node.fs_list(&did, "/Documents/..").unwrap();
    let err = wait_for(&mut phone.ev, "refusal", |e| match e {
        NodeEvent::FsListing { path, error: Some(err), .. } if path == "/Documents/.." => Some(err.clone()),
        _ => None,
    })
    .await;
    assert!(err.contains("isn't allowed"));

    // Downloading arrives as a normal transfer.
    phone.node.fs_get(&did, "/Documents/plan.txt").unwrap();
    let p = phone.node.clone();
    eventually("download", || {
        p.transfers().unwrap().iter().any(|t| t.incoming && t.state == "done" && t.name == "plan.txt")
    })
    .await;
    let t = phone.node.transfers().unwrap().into_iter().find(|t| t.name == "plan.txt").unwrap();
    assert_eq!(std::fs::read(&t.path).unwrap(), b"the plan");

    phone.node.fs_get(&did, "/Documents/../private.txt").unwrap();
    wait_for(&mut phone.ev, "get refused", |e| matches!(e, NodeEvent::FsError { .. }).then_some(())).await;

    // Without `browse`, nothing is listed.
    let mut perms = Permissions::for_role(TrustRole::Trusted);
    perms.browse = false;
    desk.node.set_access(&pid, TrustRole::Trusted, perms).unwrap();
    phone.node.fs_list(&did, "/Documents").unwrap();
    let denied = wait_for(&mut phone.ev, "denied", |e| match e {
        NodeEvent::FsListing { error: Some(err), .. } => Some(err.clone()),
        _ => None,
    })
    .await;
    assert!(denied.contains("doesn't let you browse"));

    desk.node.shutdown().await;
    phone.node.shutdown().await;
}
