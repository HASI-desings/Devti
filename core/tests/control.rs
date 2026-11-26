//! Phase 3 end-to-end tests: remote input, system actions, catalog, saved commands,
//! deck buttons and live screens, with every permission boundary — real QUIC.

use std::sync::Arc;
use std::time::Duration;

use devti_core::model::{DeviceKind, Permissions, TrustRole};
use devti_core::node::{NodeEvent, PairingRole};
use devti_core::protocol::{
    Catalog, CatalogApp, CatalogCommand, DeckButton, DeckPage, InputEvent, MouseButton, SystemAction,
};
use devti_core::secret::MemorySecretStore;
use devti_core::{Node, NodeConfig};
use tokio::sync::broadcast::Receiver;

const WAIT: Duration = Duration::from_secs(20);

struct T {
    node: Node,
    ev: Receiver<NodeEvent>,
    _dir: tempfile::TempDir,
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
    T { node, ev, _dir: dir }
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

async fn nothing_arrives(rx: &mut Receiver<NodeEvent>, what: &str, f: impl Fn(&NodeEvent) -> bool) {
    let end = tokio::time::Instant::now() + Duration::from_millis(600);
    while let Ok(Ok(e)) = tokio::time::timeout_at(end, rx.recv()).await {
        assert!(!f(&e), "{what} should not have arrived: {e:?}");
    }
}

fn catalog() -> Catalog {
    Catalog {
        apps: vec![CatalogApp { id: "firefox.desktop".into(), name: "Firefox".into() }],
        commands: vec![CatalogCommand { id: "backup".into(), name: "Back up photos".into(), confirm: true }],
        deck: vec![DeckPage {
            name: "Stream".into(),
            buttons: vec![DeckButton {
                id: "b1".into(),
                label: "Go live".into(),
                icon: "play".into(),
                voice: "coral".into(),
            }],
        }],
        system: vec!["lock".into(), "sleep".into(), "set_volume".into()],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_input_system_and_catalog_respect_permissions() {
    let mut hub = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let (hid, pid) = (hub.node.device_id(), phone.node.device_id());
    hub.node.set_catalog(catalog());

    // Touchpad + keyboard from the phone arrive on the hub, sanitized.
    phone
        .node
        .send_input(
            &hid,
            vec![
                InputEvent::Move { dx: 12.0, dy: -4.0 },
                InputEvent::Move { dx: f32::NAN, dy: 0.0 },
                InputEvent::Click { button: MouseButton::Left, count: 1 },
                InputEvent::Text { text: "hello".into() },
                InputEvent::Key { key: "tab".into(), mods: vec!["alt".into()] },
            ],
        )
        .unwrap();
    let events = wait_for(&mut hub.ev, "input", |e| match e {
        NodeEvent::RemoteInput { from, events } if *from == pid => Some(events.clone()),
        _ => None,
    })
    .await;
    assert_eq!(events.len(), 4, "the NaN move is dropped");
    assert_eq!(events[2], InputEvent::Text { text: "hello".into() });

    // Volume needs control, lock needs commands.
    phone.node.system_action(&hid, SystemAction::SetVolume { percent: 40 }).unwrap();
    wait_for(&mut hub.ev, "volume", |e| {
        matches!(e, NodeEvent::SystemRequested { action: SystemAction::SetVolume { percent: 40 }, .. }).then_some(())
    })
    .await;
    phone.node.system_action(&hid, SystemAction::Lock).unwrap();
    wait_for(&mut hub.ev, "lock", |e| {
        matches!(e, NodeEvent::SystemRequested { action: SystemAction::Lock, .. }).then_some(())
    })
    .await;

    // Full catalog for a trusted remote.
    phone.node.request_catalog(&hid).unwrap();
    let got = wait_for(&mut phone.ev, "catalog", |e| match e {
        NodeEvent::CatalogReceived { catalog, .. } => Some(catalog.clone()),
        _ => None,
    })
    .await;
    assert_eq!(got, catalog());

    // Launch: only apps in the catalog.
    phone.node.launch_app(&hid, "/bin/sh").unwrap();
    phone.node.launch_app(&hid, "firefox.desktop").unwrap();
    let app = wait_for(&mut hub.ev, "launch", |e| match e {
        NodeEvent::LaunchRequested { app, .. } => Some(app.clone()),
        _ => None,
    })
    .await;
    assert_eq!(app, "firefox.desktop");

    // Restrict the phone to control only: no power, no commands, a trimmed catalog.
    let mut perms = Permissions::for_role(TrustRole::Restricted);
    perms.control = true;
    hub.node.set_access(&pid, TrustRole::Restricted, perms).unwrap();
    phone.node.system_action(&hid, SystemAction::Shutdown).unwrap();
    phone.node.launch_app(&hid, "firefox.desktop").unwrap();
    phone.node.deck_press(&hid, "b1").unwrap();
    nothing_arrives(&mut hub.ev, "power/launch/deck", |e| {
        matches!(
            e,
            NodeEvent::SystemRequested { .. } | NodeEvent::LaunchRequested { .. } | NodeEvent::DeckPressed { .. }
        )
    })
    .await;
    phone.node.request_catalog(&hid).unwrap();
    let trimmed = wait_for(&mut phone.ev, "catalog", |e| match e {
        NodeEvent::CatalogReceived { catalog, .. } => Some(catalog.clone()),
        _ => None,
    })
    .await;
    assert!(trimmed.apps.is_empty() && trimmed.commands.is_empty() && trimmed.deck.is_empty());
    assert_eq!(trimmed.system, vec!["set_volume".to_string()]);

    // And with control off too, input is ignored.
    hub.node.set_access(&pid, TrustRole::Restricted, Permissions::for_role(TrustRole::Restricted)).unwrap();
    phone.node.send_input(&hid, vec![InputEvent::Move { dx: 1.0, dy: 1.0 }]).unwrap();
    nothing_arrives(&mut hub.ev, "input", |e| matches!(e, NodeEvent::RemoteInput { .. })).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn saved_commands_and_deck_round_trip() {
    let mut hub = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let (hid, pid) = (hub.node.device_id(), phone.node.device_id());
    hub.node.set_catalog(catalog());

    // Unknown command: refused with a readable result.
    phone.node.run_command(&hid, "rm-everything").unwrap();
    let (ok, out) = wait_for(&mut phone.ev, "refusal", |e| match e {
        NodeEvent::RunFinished { ok, output, .. } => Some((*ok, output.clone())),
        _ => None,
    })
    .await;
    assert!(!ok && out.contains("doesn't exist"));

    // Known command: the hub host runs it and reports back.
    phone.node.run_command(&hid, "backup").unwrap();
    let cmd = wait_for(&mut hub.ev, "run", |e| match e {
        NodeEvent::RunRequested { from, command, .. } if *from == pid => Some(command.clone()),
        _ => None,
    })
    .await;
    hub.node.report_run(&pid, &cmd, true, "copied 12 photos").unwrap();
    let out = wait_for(&mut phone.ev, "result", |e| match e {
        NodeEvent::RunFinished { ok: true, output, .. } => Some(output.clone()),
        _ => None,
    })
    .await;
    assert_eq!(out, "copied 12 photos");

    phone.node.deck_press(&hid, "nope").unwrap();
    phone.node.deck_press(&hid, "b1").unwrap();
    let b = wait_for(&mut hub.ev, "deck", |e| match e {
        NodeEvent::DeckPressed { button, .. } => Some(button.clone()),
        _ => None,
    })
    .await;
    assert_eq!(b, "b1");
    assert!(hub.node.activity(None, 100, None).unwrap().iter().any(|a| a.action == "command_run"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_screen_frames_flow_and_stop() {
    let mut hub = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let (hid, pid) = (hub.node.device_id(), phone.node.device_id());

    // The hub asks to watch the phone's screen.
    let mut frames = hub.node.subscribe_frames();
    hub.node.screen_start(&pid, 1280, 30).unwrap();
    wait_for(&mut phone.ev, "screen requested", |e| {
        matches!(e, NodeEvent::ScreenRequested { on: true, from, .. } if *from == hid).then_some(())
    })
    .await;
    assert_eq!(phone.node.screen_viewers(), vec![hid]);
    assert_eq!(phone.node.screen_target().max_width, 1280);

    // The phone's host pushes frames; the hub receives the latest ones in order.
    let pusher = {
        let node = phone.node.clone();
        tokio::spawn(async move {
            for i in 0..60u32 {
                let jpeg = vec![0xFF, 0xD8, (i % 250) as u8, 0xFF, 0xD9];
                node.push_screen_frame(720, 1600, jpeg).unwrap();
                tokio::time::sleep(Duration::from_millis(16)).await;
            }
        })
    };
    let mut last = 0u64;
    let mut count = 0;
    let deadline = tokio::time::Instant::now() + WAIT;
    while count < 10 {
        let f = tokio::time::timeout_at(deadline, frames.recv()).await.expect("frames").expect("open");
        assert_eq!(f.from, pid);
        assert!(f.seq > last, "frames arrive in order");
        assert_eq!((f.w, f.h), (720, 1600));
        last = f.seq;
        count += 1;
    }
    pusher.await.unwrap();
    wait_for(&mut hub.ev, "stats", |e| matches!(e, NodeEvent::ScreenStats { sharing: false, .. }).then_some(())).await;

    hub.node.screen_stop(&pid).unwrap();
    wait_for(&mut phone.ev, "stopped", |e| matches!(e, NodeEvent::ScreenRequested { on: false, .. }).then_some(()))
        .await;
    assert!(phone.node.screen_viewers().is_empty());

    // Without screen_view, a request is ignored.
    let mut perms = Permissions::for_role(TrustRole::Trusted);
    perms.screen_view = false;
    phone.node.set_access(&hid, TrustRole::Trusted, perms).unwrap();
    hub.node.screen_start(&pid, 1280, 30).unwrap();
    nothing_arrives(&mut phone.ev, "screen request", |e| matches!(e, NodeEvent::ScreenRequested { .. })).await;
    assert!(phone.node.screen_viewers().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn camera_and_screen_share_side_by_side_and_survive_a_second_request() {
    let mut hub = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let (hid, pid) = (hub.node.device_id(), phone.node.device_id());

    // An old screen frame is lying around from before anyone asked.
    phone.node.push_camera_frame(1, 1, vec![0xFF, 0xD8, 0xEE, 0xFF, 0xD9]).unwrap();

    let mut frames = hub.node.subscribe_frames();
    hub.node.camera_start_facing(&pid, 1280, 30, Some("back")).unwrap();
    let facing = wait_for(&mut phone.ev, "camera requested", |e| match e {
        NodeEvent::ScreenRequested { on: true, source, facing, .. } if source == "camera" => Some(facing.clone()),
        _ => None,
    })
    .await;
    assert_eq!(facing.as_deref(), Some("back"));
    // Clicking "Use camera" again must not end it.
    hub.node.camera_start(&pid, 1280, 30).unwrap();
    // Mirroring the same phone at the same time.
    hub.node.screen_start(&pid, 1280, 30).unwrap();
    wait_for(&mut phone.ev, "screen requested", |e| {
        matches!(e, NodeEvent::ScreenRequested { on: true, source, .. } if source == "screen").then_some(())
    })
    .await;
    assert_eq!(phone.node.camera_viewers(), vec![hid]);
    assert_eq!(phone.node.screen_viewers(), vec![hid]);

    let pusher = {
        let node = phone.node.clone();
        tokio::spawn(async move {
            for i in 0..80u32 {
                node.push_camera_frame(1280, 720, vec![0xFF, 0xD8, 1, (i % 250) as u8, 0xFF, 0xD9]).unwrap();
                node.push_screen_frame(720, 1600, vec![0xFF, 0xD8, 2, (i % 250) as u8, 0xFF, 0xD9]).unwrap();
                tokio::time::sleep(Duration::from_millis(16)).await;
            }
        })
    };
    let (mut cams, mut screens) = (0, 0);
    let deadline = tokio::time::Instant::now() + WAIT;
    while cams < 5 || screens < 5 {
        let f = tokio::time::timeout_at(deadline, frames.recv()).await.expect("frames").expect("open");
        assert_ne!(f.jpeg[2], 0xEE, "the stale frame from before the request never arrives");
        match f.source {
            "camera" => {
                assert_eq!(f.jpeg[2], 1);
                assert_eq!((f.w, f.h), (1280, 720));
                cams += 1;
            }
            _ => {
                assert_eq!(f.jpeg[2], 2);
                screens += 1;
            }
        }
    }
    pusher.await.unwrap();

    // Leaving the mirror keeps the camera running.
    hub.node.screen_stop(&pid).unwrap();
    wait_for(&mut phone.ev, "screen stopped", |e| {
        matches!(e, NodeEvent::ScreenRequested { on: false, source, .. } if source == "screen").then_some(())
    })
    .await;
    assert!(phone.node.screen_viewers().is_empty());
    assert_eq!(phone.node.camera_viewers(), vec![hid]);
    hub.node.camera_stop(&pid).unwrap();
    wait_for(&mut phone.ev, "camera stopped", |e| {
        matches!(e, NodeEvent::ScreenRequested { on: false, source, .. } if source == "camera").then_some(())
    })
    .await;
    assert!(phone.node.camera_viewers().is_empty());
}
