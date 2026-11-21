//! Where the device's private key lives.
//!
//! The core never decides on its own where secrets go: the host app injects a
//! [`SecretStore`]. Desktop uses the OS keychain ([`KeychainSecretStore`], feature
//! `keychain`) and falls back to a permission-locked file ([`FileSecretStore`]) when no
//! keychain is running. Android/iOS implement the trait in Kotlin/Swift on top of the
//! Android Keystore / iOS Keychain and pass it through UniFFI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use crate::error::{Error, Result};

/// A small key-value vault for secrets (bytes).
pub trait SecretStore: Send + Sync {
    /// Reads a secret, `Ok(None)` when absent.
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    /// Creates or replaces a secret.
    fn set(&self, key: &str, value: &[u8]) -> Result<()>;
    /// Deletes a secret; deleting a missing key is not an error.
    fn delete(&self, key: &str) -> Result<()>;
    /// Human-readable backend name shown in Settings → Security.
    fn backend(&self) -> String;
    /// `true` when secrets are protected by the OS (keychain/keystore) rather than a file.
    fn is_os_protected(&self) -> bool;
}

/// In-memory store, for tests and ephemeral sessions.
#[derive(Default)]
pub struct MemorySecretStore {
    inner: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.inner.lock().get(key).cloned())
    }
    fn set(&self, key: &str, value: &[u8]) -> Result<()> {
        self.inner.lock().insert(key.to_string(), value.to_vec());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.inner.lock().remove(key);
        Ok(())
    }
    fn backend(&self) -> String {
        "memory (ephemeral)".into()
    }
    fn is_os_protected(&self) -> bool {
        false
    }
}

/// One file per secret inside a private directory (`0700` dir, `0600` files on Unix).
///
/// This is the honest fallback when no keychain exists. The UI states that this
/// backend is in use, so the user knows the key is only protected by file permissions.
pub struct FileSecretStore {
    dir: PathBuf,
}

impl FileSecretStore {
    pub fn new(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        restrict_dir(&dir)?;
        Ok(Self { dir })
    }

    fn path_for(&self, key: &str) -> Result<PathBuf> {
        if key.is_empty()
            || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            || key.starts_with('.')
        {
            return Err(Error::SecretStore(format!("invalid secret key name '{key}'")));
        }
        Ok(self.dir.join(format!("{key}.secret")))
    }
}

impl SecretStore for FileSecretStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.path_for(key)?;
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::SecretStore(e.to_string())),
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<()> {
        let path = self.path_for(key)?;
        let tmp = path.with_extension("tmp");
        write_private(&tmp, value)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        let path = self.path_for(key)?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::SecretStore(e.to_string())),
        }
    }

    fn backend(&self) -> String {
        "protected file (no system keychain available)".into()
    }

    fn is_os_protected(&self) -> bool {
        false
    }
}

#[cfg(unix)]
fn restrict_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_dir(_dir: &Path) -> Result<()> {
    // On Windows the per-user AppData ACL already restricts access to the user.
    Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    Ok(())
}

/// The operating system's credential vault.
#[cfg(feature = "keychain")]
pub struct KeychainSecretStore {
    service: String,
}

#[cfg(feature = "keychain")]
impl KeychainSecretStore {
    /// Opens the OS keychain and proves it works with a write/read/delete probe.
    /// Returns an error (so the caller can fall back) when no keychain is reachable —
    /// e.g. a Linux session without gnome-keyring or KWallet.
    pub fn open(service: &str) -> Result<Self> {
        let store = Self { service: service.to_string() };
        let probe = b"devti-probe";
        store.set("probe", probe)?;
        let read = store.get("probe")?;
        store.delete("probe")?;
        if read.as_deref() != Some(probe.as_slice()) {
            return Err(Error::SecretStore("keychain probe returned different data".into()));
        }
        Ok(store)
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key).map_err(|e| Error::SecretStore(e.to_string()))
    }
}

#[cfg(feature = "keychain")]
impl SecretStore for KeychainSecretStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self.entry(key)?.get_secret() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::SecretStore(e.to_string())),
        }
    }
    fn set(&self, key: &str, value: &[u8]) -> Result<()> {
        self.entry(key)?.set_secret(value).map_err(|e| Error::SecretStore(e.to_string()))
    }
    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::SecretStore(e.to_string())),
        }
    }
    fn backend(&self) -> String {
        if cfg!(target_os = "windows") {
            "Windows Credential Manager".into()
        } else if cfg!(target_os = "macos") {
            "macOS Keychain".into()
        } else {
            "Secret Service (GNOME Keyring / KWallet)".into()
        }
    }
    fn is_os_protected(&self) -> bool {
        true
    }
}

/// Picks the best available store: OS keychain first, protected file otherwise.
pub fn best_available(service: &str, fallback_dir: impl AsRef<Path>) -> Result<Box<dyn SecretStore>> {
    #[cfg(feature = "keychain")]
    {
        match KeychainSecretStore::open(service) {
            Ok(store) => return Ok(Box::new(store)),
            Err(e) => tracing::warn!(error = %e, "OS keychain unavailable, using protected file store"),
        }
    }
    let _ = service;
    Ok(Box::new(FileSecretStore::new(fallback_dir)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(store: &dyn SecretStore) {
        assert_eq!(store.get("device-key").unwrap(), None);
        store.set("device-key", b"one").unwrap();
        assert_eq!(store.get("device-key").unwrap().unwrap(), b"one");
        store.set("device-key", b"two").unwrap();
        assert_eq!(store.get("device-key").unwrap().unwrap(), b"two");
        store.delete("device-key").unwrap();
        store.delete("device-key").unwrap();
        assert_eq!(store.get("device-key").unwrap(), None);
        assert!(!store.backend().is_empty());
    }

    #[test]
    fn memory_store_works() {
        let s = MemorySecretStore::new();
        exercise(&s);
        assert!(!s.is_os_protected());
    }

    #[test]
    fn file_store_works_and_rejects_bad_keys() {
        let dir = tempfile::tempdir().unwrap();
        let s = FileSecretStore::new(dir.path().join("secrets")).unwrap();
        exercise(&s);
        assert!(s.set("../escape", b"x").is_err());
        assert!(s.set("", b"x").is_err());
        assert!(s.get(".hidden").is_err());
        assert!(s.delete("a/b").is_err());
        assert!(!s.is_os_protected());
    }

    #[cfg(unix)]
    #[test]
    fn file_store_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path().join("secrets");
        let s = FileSecretStore::new(&sdir).unwrap();
        s.set("k", b"v").unwrap();
        let mode = std::fs::metadata(sdir.join("k.secret")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dmode = std::fs::metadata(&sdir).unwrap().permissions().mode();
        assert_eq!(dmode & 0o777, 0o700);
    }

    #[test]
    fn best_available_always_returns_a_store() {
        let dir = tempfile::tempdir().unwrap();
        let s = best_available("devti-test", dir.path()).unwrap();
        exercise(s.as_ref());
    }
}
