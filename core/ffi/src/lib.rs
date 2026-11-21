//! UniFFI surface of Devti Core for the mobile companion apps.
//!
//! The surface is deliberately small: one `DevtiMobile` object, two callback
//! interfaces (events out, secure storage in) and JSON for rich data. The JSON is the
//! exact `NodeEvent` / `PeerSnapshot` shape the desktop UI consumes, so all three apps
//! speak one schema (documented in `docs/PROTOCOL.md` § Host API).

use std::path::PathBuf;
use std::sync::Arc;

use devti_core::model::{BatteryState, Capability, DeviceId, DeviceKind, DeviceStatus, Platform};
use devti_core::secret::SecretStore;
use devti_core::{Node, NodeConfig};

uniffi::setup_scaffolding!();

/// Errors surfaced to Kotlin/Swift as exceptions.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum DevtiError {
    #[error("{message}")]
    InvalidInput { message: String },
    #[error("{message}")]
    Failed { message: String },
    #[error("the ecosystem is paused")]
    Paused,
}

impl From<devti_core::Error> for DevtiError {
    fn from(e: devti_core::Error) -> Self {
        match e {
            devti_core::Error::Paused => DevtiError::Paused,
            devti_core::Error::InvalidInput(m) | devti_core::Error::InvalidPairingUri(m) => {
                DevtiError::InvalidInput { message: m }
            }
            other => DevtiError::Failed { message: other.to_string() },
        }
    }
}

impl From<uniffi::UnexpectedUniFFICallbackError> for DevtiError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        DevtiError::Failed { message: e.reason }
    }
}

/// Receives every node event as JSON (`{"type": "...", ...}`), on a background thread.
#[uniffi::export(callback_interface)]
pub trait EventListener: Send + Sync {
    fn on_event(&self, event_json: String);
}

/// Platform secure storage: Android Keystore-wrapped values / iOS Keychain items.
#[uniffi::export(callback_interface)]
pub trait PlatformSecretStore: Send + Sync {
    fn get(&self, key: String) -> Result<Option<Vec<u8>>, DevtiError>;
    fn set(&self, key: String, value: Vec<u8>) -> Result<(), DevtiError>;
    fn delete(&self, key: String) -> Result<(), DevtiError>;
    fn backend_name(&self) -> String;
}

struct SecretBridge(Box<dyn PlatformSecretStore>);

impl SecretStore for SecretBridge {
    fn get(&self, key: &str) -> devti_core::Result<Option<Vec<u8>>> {
        self.0.get(key.to_string()).map_err(|e| devti_core::Error::SecretStore(e.to_string()))
    }
    fn set(&self, key: &str, value: &[u8]) -> devti_core::Result<()> {
        self.0.set(key.to_string(), value.to_vec()).map_err(|e| devti_core::Error::SecretStore(e.to_string()))
    }
    fn delete(&self, key: &str) -> devti_core::Result<()> {
        self.0.delete(key.to_string()).map_err(|e| devti_core::Error::SecretStore(e.to_string()))
    }
    fn backend(&self) -> String {
        self.0.backend_name()
    }
    fn is_os_protected(&self) -> bool {
        true
    }
}

/// Static description of the phone/tablet running the app.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileConfig {
    pub data_dir: String,
    pub device_name: String,
    /// `"phone"` or `"tablet"`.
    pub kind: String,
    /// `"android"`, `"ios"` or `"ipados"`.
    pub platform: String,
    pub os_version: String,
    pub model: Option<String>,
    pub app_version: String,
    pub discovery: bool,
}

/// A running Devti node inside the mobile app.
#[derive(uniffi::Object)]
pub struct DevtiMobile {
    runtime: tokio::runtime::Runtime,
    node: Node,
}

#[uniffi::export]
impl DevtiMobile {
    /// Starts the node. Blocks briefly while the identity and database load.
    #[uniffi::constructor]
    pub fn new(
        config: MobileConfig,
        secrets: Box<dyn PlatformSecretStore>,
        listener: Box<dyn EventListener>,
    ) -> Result<Arc<Self>, DevtiError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("devti-core")
            .enable_all()
            .build()
            .map_err(|e| DevtiError::Failed { message: e.to_string() })?;

        let mut cfg =
            NodeConfig::new(PathBuf::from(&config.data_dir), &config.device_name, DeviceKind::parse(&config.kind));
        cfg.platform = Platform::parse(&config.platform);
        cfg.os_version = config.os_version;
        cfg.model = config.model;
        cfg.app_version = config.app_version;
        cfg.discovery = config.discovery;
        cfg.capabilities = vec![Capability::Battery];

        let secrets: Arc<dyn SecretStore> = Arc::new(SecretBridge(secrets));
        let node = runtime.block_on(Node::start(cfg, secrets))?;

        let mut events = node.subscribe();
        let listener: Arc<dyn EventListener> = Arc::from(listener);
        runtime.spawn(async move {
            loop {
                match events.recv().await {
                    Ok(ev) => {
                        if let Ok(json) = serde_json::to_string(&ev) {
                            listener.on_event(json);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        listener.on_event(r#"{"type":"resync"}"#.to_string());
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Arc::new(Self { runtime, node }))
    }

    /// This device as JSON (`LocalSnapshot`).
    pub fn local_json(&self) -> String {
        serde_json::to_string(&self.node.local()).unwrap_or_default()
    }

    /// Paired devices as a JSON array of `PeerSnapshot`.
    pub fn peers_json(&self) -> String {
        serde_json::to_string(&self.node.peers()).unwrap_or_default()
    }

    /// Unpaired Devti devices on the network (JSON array of `DiscoveredDevice`).
    pub fn nearby_json(&self) -> String {
        serde_json::to_string(&self.node.nearby()).unwrap_or_default()
    }

    /// Starts pairing from a scanned `devti://pair?...` link. Returns the pairing id.
    pub fn pair_with_uri(&self, uri: String) -> Result<u64, DevtiError> {
        let _g = self.runtime.enter();
        Ok(self.node.pair_with_uri(&uri)?)
    }

    /// Starts pairing with a discovered device using its 6-digit code.
    pub fn pair_with_code(&self, device_id: String, code: String) -> Result<u64, DevtiError> {
        let _g = self.runtime.enter();
        let id: DeviceId = device_id.parse()?;
        Ok(self.node.pair_with_code(&id, &code)?)
    }

    /// Answers "do the codes match?".
    pub fn confirm_pairing(&self, pairing_id: u64, accept: bool) -> Result<(), DevtiError> {
        Ok(self.node.confirm_pairing(pairing_id, accept)?)
    }

    pub fn set_battery(&self, percent: u8, charging: bool) {
        let _g = self.runtime.enter();
        self.node.set_status(DeviceStatus { battery: Some(BatteryState::new(percent, charging)) });
    }

    pub fn rename(&self, name: String) -> Result<(), DevtiError> {
        let _g = self.runtime.enter();
        self.node.rename(&name)?;
        Ok(())
    }

    pub fn revoke(&self, device_id: String) -> Result<(), DevtiError> {
        let _g = self.runtime.enter();
        let id: DeviceId = device_id.parse()?;
        Ok(self.node.revoke(&id)?)
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), DevtiError> {
        let _g = self.runtime.enter();
        Ok(self.node.set_paused(paused)?)
    }

    /// Call when the network changes or the app returns to the foreground.
    pub fn reconnect_now(&self) {
        self.node.reconnect_now();
    }

    /// Graceful stop (says goodbye to peers).
    pub fn shutdown(&self) {
        self.runtime.block_on(self.node.shutdown());
    }
}

/// Devti Core version string.
#[uniffi::export]
pub fn core_version() -> String {
    devti_core::VERSION.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemStore(Mutex<HashMap<String, Vec<u8>>>);
    impl PlatformSecretStore for MemStore {
        fn get(&self, key: String) -> Result<Option<Vec<u8>>, DevtiError> {
            Ok(self.0.lock().unwrap().get(&key).cloned())
        }
        fn set(&self, key: String, value: Vec<u8>) -> Result<(), DevtiError> {
            self.0.lock().unwrap().insert(key, value);
            Ok(())
        }
        fn delete(&self, key: String) -> Result<(), DevtiError> {
            self.0.lock().unwrap().remove(&key);
            Ok(())
        }
        fn backend_name(&self) -> String {
            "test".into()
        }
    }

    struct Sink(Arc<Mutex<Vec<String>>>);
    impl EventListener for Sink {
        fn on_event(&self, json: String) {
            self.0.lock().unwrap().push(json);
        }
    }

    #[test]
    fn mobile_node_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let m = DevtiMobile::new(
            MobileConfig {
                data_dir: dir.path().to_string_lossy().into(),
                device_name: "Test Phone".into(),
                kind: "phone".into(),
                platform: "android".into(),
                os_version: "Android 15".into(),
                model: Some("Pixel 8".into()),
                app_version: "0.1.0".into(),
                discovery: false,
            },
            Box::new(MemStore::default()),
            Box::new(Sink(events.clone())),
        )
        .unwrap();
        let local: serde_json::Value = serde_json::from_str(&m.local_json()).unwrap();
        assert_eq!(local["info"]["name"], "Test Phone");
        assert_eq!(local["info"]["platform"], "android");
        assert_eq!(local["secret_backend"], "test");
        assert_eq!(m.peers_json(), "[]");
        assert_eq!(m.nearby_json(), "[]");
        m.set_battery(80, true);
        m.rename("Renamed".into()).unwrap();
        assert!(matches!(m.pair_with_uri("garbage".into()), Err(DevtiError::InvalidInput { .. })));
        assert!(m.pair_with_code("bad".into(), "123456".into()).is_err());
        assert!(m.confirm_pairing(1, true).is_err());
        assert!(m.revoke("AAAAAAAAAAAAAAAA".into()).is_err());
        m.set_paused(true).unwrap();
        assert!(matches!(
            m.pair_with_uri("devti://pair".into()),
            Err(DevtiError::InvalidInput { .. }) | Err(DevtiError::Paused)
        ));
        m.set_paused(false).unwrap();
        m.reconnect_now();
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(events.lock().unwrap().iter().any(|e| e.contains("local_changed")));
        m.shutdown();
        assert!(!core_version().is_empty());
    }
}
