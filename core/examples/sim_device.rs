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
use devti_core::secret::MemorySecretStore;
use devti_core::{Node, NodeConfig, NodeEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let name = arg("--name").unwrap_or_else(|| "Simulated Phone".into());
    let kind = DeviceKind::parse(&arg("--kind").unwrap_or_else(|| "phone".into()));
    let seconds: u64 = arg("--seconds").and_then(|s| s.parse().ok()).unwrap_or(600);

    let dir = std::env::temp_dir().join(format!("devti-sim-{}", std::process::id()));
    let mut cfg = NodeConfig::new(&dir, &name, kind);
    cfg.platform = Platform::Android;
    cfg.os_version = "Android 15 (simulated)".into();
    cfg.model = Some("Devti simulator".into());
    cfg.port = 0;
    cfg.discovery = arg("--mdns").is_some();
    let node = Node::start(cfg, Arc::new(MemorySecretStore::new())).await?;
    let mut events = node.subscribe();

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
                Ok(NodeEvent::PeerChanged { peer }) => println!("peer {} online={}", peer.info.name, peer.online),
                Ok(NodeEvent::PeerRemoved { .. }) => {
                    println!("removed by the hub");
                    break;
                }
                Ok(_) => {}
                Err(_) => {}
            },
            _ = tick.tick() => {
                battery = battery.saturating_sub(1).max(5);
                node.set_status(DeviceStatus { battery: Some(BatteryState::new(battery, false)) });
            }
            _ = &mut deadline => break,
        }
    }
    node.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}
