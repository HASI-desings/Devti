//! Phase 6 end-to-end tests: SMS, calls, handoff, live audio and the camera as a webcam,
//! with their permission boundaries — real QUIC.

use std::sync::Arc;
use std::time::Duration;

use devti_core::model::{DeviceKind, Permissions, TrustRole};
use devti_core::node::{NodeEvent, PairingRole};
use devti_core::protocol::{CallInfo, HandoffItem, SmsMessage, SmsThread};
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

fn sms(id: &str, body: &str) -> SmsMessage {
    SmsMessage {
        id: id.into(),
        thread: "t1".into(),
        address: "+92300000000".into(),
        name: "Ali".into(),
        body: body.into(),
        ts: 1,
        outgoing: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sms_threads_messages_and_sending() {
    let mut hub = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let (hid, pid) = (hub.node.device_id(), phone.node.device_id());

    // The hub asks; the phone's host answers with its conversations.
    hub.node.request_sms_threads(&pid).unwrap();
    wait_for(&mut phone.ev, "threads requested", |e| matches!(e, NodeEvent::SmsThreadsRequested { .. }).then_some(()))
        .await;
    let thread = SmsThread {
        id: "t1".into(),
        addresses: vec!["+92300000000".into()],
        names: vec!["Ali".into()],
        snippet: "See you".into(),
        ts: 5,
        unread: 1,
    };
    phone.node.answer_sms_threads(&hid, vec![thread.clone()]).unwrap();
    let got = wait_for(&mut hub.ev, "threads", |e| match e {
        NodeEvent::SmsThreads { threads, .. } => Some(threads.clone()),
        _ => None,
    })
    .await;
    assert_eq!(got, vec![thread]);

    hub.node.request_sms_messages(&pid, "t1").unwrap();
    let t = wait_for(&mut phone.ev, "messages requested", |e| match e {
        NodeEvent::SmsMessagesRequested { thread, .. } => Some(thread.clone()),
        _ => None,
    })
    .await;
    phone.node.answer_sms_messages(&hid, &t, vec![sms("1", "See you"), sms("2", "at 5")]).unwrap();
    let msgs = wait_for(&mut hub.ev, "messages", |e| match e {
        NodeEvent::SmsMessages { messages, .. } => Some(messages.clone()),
        _ => None,
    })
    .await;
    assert_eq!(msgs.len(), 2);

    // Sending: the phone's host is asked, and reports back.
    hub.node.send_sms(&pid, vec!["+92300000000".into()], "On my way").unwrap();
    let (to, text) = wait_for(&mut phone.ev, "send requested", |e| match e {
        NodeEvent::SmsSendRequested { to, text, .. } => Some((to.clone(), text.clone())),
        _ => None,
    })
    .await;
    assert_eq!((to[0].as_str(), text.as_str()), ("+92300000000", "On my way"));
    phone.node.report_sms_result(&hid, true, None).unwrap();
    wait_for(&mut hub.ev, "result", |e| matches!(e, NodeEvent::SmsResult { ok: true, .. }).then_some(())).await;

    // A new text on the phone reaches the hub.
    phone.node.publish_sms(sms("3", "Where are you?"));
    let m = wait_for(&mut hub.ev, "incoming", |e| match e {
        NodeEvent::Sms { message, .. } => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(m.body, "Where are you?");

    // Without `commands`, sending is refused with a readable answer.
    let mut perms = Permissions::for_role(TrustRole::Restricted);
    perms.notifications = true;
    phone.node.set_access(&hid, TrustRole::Restricted, perms).unwrap();
    hub.node.send_sms(&pid, vec!["+1".into()], "hi").unwrap();
    let err = wait_for(&mut hub.ev, "refusal", |e| match e {
        NodeEvent::SmsResult { ok: false, error, .. } => Some(error.clone()),
        _ => None,
    })
    .await;
    assert!(err.unwrap().contains("Not allowed"));
    assert!(hub.node.send_sms(&pid, vec!["+1".into()], "   ").is_err(), "empty text refused locally");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn calls_and_handoff() {
    let mut hub = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let (hid, pid) = (hub.node.device_id(), phone.node.device_id());

    phone.node.publish_call(CallInfo {
        id: "c1".into(),
        state: "ringing".into(),
        number: "+44 20".into(),
        name: "Mum".into(),
        incoming: true,
        ts: 1,
    });
    let call = wait_for(&mut hub.ev, "call", |e| match e {
        NodeEvent::Call { call, .. } => Some(call.clone()),
        _ => None,
    })
    .await;
    assert_eq!((call.state.as_str(), call.name.as_str()), ("ringing", "Mum"));
    hub.node.call_action(&pid, "c1", "answer").unwrap();
    hub.node.call_action(&pid, "c1", "format_disk").unwrap();
    let action = wait_for(&mut phone.ev, "call action", |e| match e {
        NodeEvent::CallActionRequested { action, .. } => Some(action.clone()),
        _ => None,
    })
    .await;
    assert_eq!(action, "answer");

    // Handoff: a web link at a media position goes through; other schemes don't.
    let item = HandoffItem {
        kind: "media".into(),
        url: "https://example.com/v".into(),
        title: "Talk".into(),
        text: String::new(),
        position_ms: Some(83_000),
    };
    hub.node.handoff(&pid, item.clone()).unwrap();
    let got = wait_for(&mut phone.ev, "handoff", |e| match e {
        NodeEvent::Handoff { item, .. } => Some(item.clone()),
        _ => None,
    })
    .await;
    assert_eq!(got, item);
    let bad = HandoffItem {
        kind: "url".into(),
        url: "file:///etc/passwd".into(),
        title: String::new(),
        text: String::new(),
        position_ms: None,
    };
    assert!(hub.node.handoff(&pid, bad).is_err());
    let _ = hid;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_audio_and_camera_as_webcam() {
    let mut hub = spawn("Studio Laptop", DeviceKind::Laptop).await;
    let mut phone = spawn("Pixel 8", DeviceKind::Phone).await;
    pair(&mut hub, &mut phone).await;
    let (hid, pid) = (hub.node.device_id(), phone.node.device_id());

    // The phone streams its microphone to the hub.
    let mut audio = hub.node.subscribe_audio();
    let tx = phone.node.start_audio(&hid, "mic", 48_000, 1).unwrap();
    let (kind, rate) = wait_for(&mut hub.ev, "audio started", |e| match e {
        NodeEvent::AudioStarted { kind, sample_rate, .. } => Some((kind.clone(), *sample_rate)),
        _ => None,
    })
    .await;
    assert_eq!((kind.as_str(), rate), ("mic", 48_000));
    let pusher = tokio::spawn(async move {
        for i in 0..20u8 {
            tx.push(vec![i; 1920]);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        drop(tx);
    });
    let mut got = 0;
    while got < 5 {
        let p = tokio::time::timeout(WAIT, audio.recv()).await.unwrap().unwrap();
        assert_eq!((p.from, p.channels, p.pcm.len()), (pid, 1, 1920));
        got += 1;
    }
    pusher.await.unwrap();
    wait_for(&mut hub.ev, "audio ended", |e| matches!(e, NodeEvent::AudioEnded { .. }).then_some(())).await;
    assert!(phone.node.start_audio(&hid, "speaker-hack", 48_000, 2).is_err());

    // The camera: the phone offers, the hub accepts as a camera viewer.
    phone.node.camera_offer(&hid).unwrap();
    wait_for(&mut hub.ev, "offer", |e| matches!(e, NodeEvent::CameraOffered { .. }).then_some(())).await;
    hub.node.camera_start(&pid, 1280, 30).unwrap();
    let source = wait_for(&mut phone.ev, "camera requested", |e| match e {
        NodeEvent::ScreenRequested { on: true, source, .. } => Some(source.clone()),
        _ => None,
    })
    .await;
    assert_eq!(source, "camera");
    assert_eq!(phone.node.screen_sources(), vec![(hid, "camera".to_string())]);

    // Without `bluetooth`, audio isn't accepted.
    let mut perms = Permissions::for_role(TrustRole::Trusted);
    perms.bluetooth = false;
    hub.node.set_access(&pid, TrustRole::Trusted, perms).unwrap();
    let tx = phone.node.start_audio(&hid, "relay", 48_000, 2).unwrap();
    tx.push(vec![1; 100]);
    let end = tokio::time::Instant::now() + Duration::from_millis(800);
    while let Ok(Ok(e)) = tokio::time::timeout_at(end, hub.ev.recv()).await {
        assert!(!matches!(e, NodeEvent::AudioStarted { .. }), "audio accepted without permission");
    }
}
