//! The `devti` CLI against a real Hub node with the local API: devices, ring, clipboard,
//! files, saved commands (here and on another device), auth and the event stream.

use std::sync::Arc;
use std::time::Duration;

use devti_cli::{Endpoint, run, watch};
use devti_core::api::{self, LocalRunner};
use devti_core::model::DeviceKind;
use devti_core::node::{NodeEvent, PairingRole};
use devti_core::protocol::{Catalog, CatalogCommand};
use devti_core::secret::MemorySecretStore;
use devti_core::{Node, NodeConfig};
use tokio::sync::broadcast::Receiver;

const WAIT: Duration = Duration::from_secs(20);

#[allow(dead_code)]
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

async fn pair(hub: &mut T, phone: &mut T) {
    let w = hub.node.open_pairing_window().unwrap();
    let jid = phone.node.pair_with_uri(&w.uri).unwrap();
    let hid = wait_for(&mut hub.ev, "hub verify", |e| match e {
        NodeEvent::PairingVerify { pairing_id, role: PairingRole::Hub, .. } => Some(*pairing_id),
        _ => None,
    })
    .await;
    wait_for(&mut phone.ev, "joiner verify", |e| matches!(e, NodeEvent::PairingVerify { .. }).then_some(())).await;
    hub.node.confirm_pairing(hid, true).unwrap();
    phone.node.confirm_pairing(jid, true).unwrap();
    let (h, p) = (hub.node.clone(), phone.node.clone());
    let (hid, pid) = (h.device_id(), p.device_id());
    eventually("online", || h.peer(&pid).is_some_and(|x| x.online) && p.peer(&hid).is_some_and(|x| x.online)).await;
}

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

async fn cli(api: &Endpoint, a: &str) -> Result<String, String> {
    let (api, a) = (api.clone(), args(a));
    tokio::task::spawn_blocking(move || run(&a, &api)).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cli_drives_the_hub_through_the_local_api() {
    let mut hub = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let pid = phone.node.device_id();

    hub.node.set_catalog(Catalog {
        commands: vec![CatalogCommand { id: "hello".into(), name: "Say hello".into(), confirm: false }],
        ..Default::default()
    });
    phone.node.set_catalog(Catalog {
        commands: vec![CatalogCommand { id: "torch".into(), name: "Torch".into(), confirm: false }],
        ..Default::default()
    });
    let runner: LocalRunner = Arc::new(|id: String| Box::pin(async move { (true, format!("ran {id}")) }));
    let server = api::start(hub.node.clone(), hub.dir.path(), 0, Some(runner)).await.unwrap();
    let token = std::fs::read_to_string(&server.token_path).unwrap();
    assert_eq!(token.len(), 64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&server.token_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "token readable only by the user");
    }
    let api = Endpoint { port: server.addr.port(), token: token.trim().to_string() };

    // Wrong token: refused.
    let bad = Endpoint { port: api.port, token: "0".repeat(64) };
    assert!(cli(&bad, "devices").await.unwrap_err().contains("token"));

    let list = cli(&api, "devices").await.unwrap();
    assert!(list.contains("Pixel 8") && list.contains("●"), "{list}");

    cli(&api, &format!("ring {pid}")).await.unwrap();
    wait_for(&mut phone.ev, "ring", |e| matches!(e, NodeEvent::RingRequested { on: true, .. }).then_some(())).await;

    cli(&api, "clip from the terminal").await.unwrap();
    let text = wait_for(&mut phone.ev, "clip", |e| match e {
        NodeEvent::ClipboardReceived { text, .. } => Some(text.clone()),
        _ => None,
    })
    .await;
    assert_eq!(text, "from the terminal");

    let file = hub.dir.path().join("report.txt");
    std::fs::write(&file, b"quarterly numbers").unwrap();
    let out = cli(&api, &format!("send {pid} {}", file.display())).await.unwrap();
    assert!(out.contains("Sending 1 file"));
    wait_for(&mut phone.ev, "file", |e| {
        matches!(e, NodeEvent::Transfer { transfer, .. } if transfer.incoming && transfer.state == "done").then_some(())
    })
    .await;

    assert!(cli(&api, "commands").await.unwrap().contains("Say hello"));
    assert_eq!(cli(&api, "run hello").await.unwrap(), "ran hello");
    assert!(cli(&api, "run nope").await.is_err());

    // A saved command on the phone: the phone's host answers RunRequested.
    let answer = {
        let node = phone.node.clone();
        let mut ev = phone.node.subscribe();
        tokio::spawn(async move {
            while let Ok(e) = ev.recv().await {
                if let NodeEvent::RunRequested { from, command, .. } = e {
                    node.report_run(&from, &command, true, "torch on").unwrap();
                    break;
                }
            }
        })
    };
    assert_eq!(cli(&api, &format!("run torch --on {pid}")).await.unwrap(), "torch on");
    answer.await.unwrap();

    // The event stream sees a ring from the phone.
    let watcher = {
        let api = api.clone();
        tokio::task::spawn_blocking(move || {
            let mut seen = false;
            watch(&api, |line| {
                seen = line.contains("ring_requested");
                !seen
            })
            .unwrap();
            seen
        })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    phone.node.ring(&hub.node.device_id(), true).unwrap();
    assert!(tokio::time::timeout(WAIT, watcher).await.unwrap().unwrap());

    // DNS-rebinding guard: a non-loopback Host header is refused.
    let raw = tokio::task::spawn_blocking({
        let api = api.clone();
        move || {
            use std::io::{Read, Write};
            let mut s = std::net::TcpStream::connect(("127.0.0.1", api.port)).unwrap();
            write!(s, "GET /v1/devices HTTP/1.1\r\nHost: evil.example:80\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n", api.token).unwrap();
            let mut buf = String::new();
            s.read_to_string(&mut buf).unwrap();
            buf
        }
    })
    .await
    .unwrap();
    assert!(raw.starts_with("HTTP/1.1 403"), "{raw}");
    server.stop();
}
