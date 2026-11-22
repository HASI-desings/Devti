//! A headless simulated device for development and QA.
//!
//! It runs a real Devti node (same crypto, same QUIC), pairs with a Hub and then
//! reports a draining battery, so you can exercise the desktop app without a phone.
//!
//! ```text
//! cargo run -p devti-core --example sim_device -- --uri 'devti://pair?...' [--name "Pixel 8"] [--kind phone]
//! cargo run -p devti-core --example sim_device -- --code 192.168.1.20:47810 482913
//! ```
//!
//! It answers "the codes match" automatically, so only use it on your own network.

use std::sync::Arc;
use std::time::Duration;

use devti_core::model::{BatteryState, DeviceKind, DeviceStatus, Platform};
use devti_core::protocol::{NotifAction, NotificationInfo};
use devti_core::secret::MemorySecretStore;
use devti_core::{Node, NodeConfig, NodeEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let name = arg("--name").unwrap_or_else(|| "Simulated Phone".into());
    let kind = DeviceKind::parse(&arg("--kind").unwrap_or_else(|| "phone".into()));
    let seconds: u64 = arg("--seconds").and_then(|s| s.parse().ok()).unwrap_or(600);
    let send_path = arg("--send");
    let mut greeted = false;
    let mut hub: Option<devti_core::model::DeviceId> = None;
    let mut ran = false;
    let watch = args.iter().any(|a| a == "--watch");

    let dir = std::env::temp_dir().join(format!("devti-sim-{}", std::process::id()));
    let mut cfg = NodeConfig::new(&dir, &name, kind);
    cfg.platform = Platform::Android;
    cfg.os_version = "Android 15 (simulated)".into();
    cfg.model = Some("Devti simulator".into());
    cfg.port = 0;
    cfg.discovery = arg("--mdns").is_some();
    let node = Node::start(cfg, Arc::new(MemorySecretStore::new())).await?;
    let mut events = node.subscribe();
    let mut frames = node.subscribe_frames();
    let mut frame_count = 0u32;

    let pairing_id = if let Some(uri) = arg("--uri") {
        node.pair_with_uri(&uri)?
    } else if let Some(i) = args.iter().position(|a| a == "--code") {
        let addr = args.get(i + 1).ok_or("--code needs ADDRESS CODE")?.parse()?;
        let code = args.get(i + 2).ok_or("--code needs ADDRESS CODE")?;
        node.pair_with_code_at_address(addr, code)?
    } else {
        eprintln!("usage: sim_device --uri <devti://pair link> | --code <ip:port> <code>");
        std::process::exit(2);
    };
    println!("pairing {pairing_id} as \"{name}\"");

    let mut battery = 87u8;
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(NodeEvent::PairingVerify { pairing_id, peer, sas, .. }) => {
                    println!("verify: {} shows {sas} — confirming", peer.name);
                    node.confirm_pairing(pairing_id, true)?;
                }
                Ok(NodeEvent::PairingCompleted { peer, .. }) => println!("paired with {}", peer.info.name),
                Ok(NodeEvent::PairingFailed { message, .. }) => {
                    println!("pairing failed: {message}");
                    std::process::exit(1);
                }
                Ok(NodeEvent::PeerChanged { peer }) => {
                    println!("peer {} online={}", peer.info.name, peer.online);
                    // Once connected: share a notification and (optionally) a file, like a phone would.
                    if peer.online && !greeted {
                        greeted = true;
                        hub = Some(peer.info.id);
                        if watch {
                            // Ask to see the Hub's screen, like the phone's second-screen tab does.
                            match node.screen_start(&peer.info.id, 960, 10) {
                                Ok(()) => println!("watching {}", peer.info.name),
                                Err(e) => println!("watch failed: {e}"),
                            }
                        }
                        node.publish_notification(NotificationInfo {
                            key: "sim:1".into(),
                            app_id: "org.example.chat".into(),
                            app_name: "Messages".into(),
                            title: "Simulated phone".into(),
                            text: "Hello from the simulator".into(),
                            ts: devti_core::net::now_ms(),
                            actions: vec![NotifAction { id: "reply".into(), label: "Reply".into(), reply: true }],
                            icon_png: None,
                            dismissable: true,
                        });
                        if let Some(path) = &send_path {
                            match node.send_file(&peer.info.id, std::path::Path::new(path)) {
                                Ok(id) => println!("sending {path} as transfer {id}"),
                                Err(e) => println!("send failed: {e}"),
                            }
                        }
                    }
                }
                Ok(NodeEvent::ClipboardReceived { text, from_name, .. }) => println!("clipboard from {from_name}: {text}"),
                Ok(NodeEvent::RingRequested { on, from_name, .. }) => println!("ring from {from_name}: {on}"),
                Ok(NodeEvent::NotificationActionRequested { action, reply, .. }) => {
                    println!("notification action {action} reply={}", reply.unwrap_or_default())
                }
                Ok(NodeEvent::Transfer { transfer, .. }) if transfer.state == "done" => {
                    println!("transfer {} done", transfer.name)
                }
                Ok(NodeEvent::CatalogReceived { from, catalog }) => {
                    if !ran && catalog.commands.iter().any(|c| c.id == "hello") {
                        ran = true;
                        println!("catalog commands={}", catalog.commands.len());
                        let _ = node.run_command(&from, "hello");
                        let _ = node.send_input(&from, vec![devti_core::protocol::InputEvent::Move { dx: 5.0, dy: 5.0 }]);
                    }
                }
                Ok(NodeEvent::RemoteInput { events, .. }) => {
                    let kinds: Vec<String> = events
                        .iter()
                        .map(|e| serde_json::to_value(e).ok().and_then(|v| v["k"].as_str().map(str::to_string)).unwrap_or_default())
                        .collect();
                    println!("remote input: {}", kinds.join(","));
                }
                Ok(NodeEvent::RunFinished { command, ok, output, .. }) => {
                    println!("run {command} ok={ok} out={output}")
                }
                Ok(NodeEvent::PeerRemoved { .. }) => {
                    println!("removed by the hub");
                    break;
                }
                Ok(_) => {}
                Err(_) => {}
            },
            f = frames.recv() => if let Ok(f) = f {
                frame_count += 1;
                if frame_count == 5 {
                    let jpeg = f.jpeg.len() > 2 && f.jpeg[0] == 0xFF && f.jpeg[1] == 0xD8;
                    println!("screen frames={frame_count} size={}x{} jpeg={jpeg}", f.w, f.h);
                }
            },
            _ = tick.tick() => {
                battery = battery.saturating_sub(1).max(5);
                node.set_status(DeviceStatus { battery: Some(BatteryState::new(battery, false)) });
                // Look for the Hub's saved commands (the smoke test adds one while running).
                if let (Some(h), false) = (hub, ran) {
                    let _ = node.request_catalog(&h);
                }
            }
            _ = &mut deadline => break,
        }
    }
    node.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}
