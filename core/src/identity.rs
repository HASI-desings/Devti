//! Device identity: an Ed25519 key pair plus a self-signed X.509 certificate.
//!
//! The certificate only exists because TLS 1.3 (and therefore QUIC) transports public
//! keys inside certificates. Nothing about it is trusted by itself: trust comes from
//! pinning its SPKI fingerprint at pairing time. Validity dates are ignored by our
//! verifiers, so the certificate never expires out from under a pairing.

use std::path::Path;

use ring::digest::{SHA256, digest};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::error::{Error, Result};
use crate::model::{DeviceId, Fingerprint};
use crate::secret::SecretStore;

/// Key under which the PKCS#8 private key is stored in the [`SecretStore`].
pub const KEY_SECRET_NAME: &str = "device-identity-ed25519-v1";
/// File (public, not secret) holding the certificate in the data directory.
pub const CERT_FILE_NAME: &str = "identity.cert.der";

/// This device's long-term identity.
pub struct Identity {
    key_pkcs8: Vec<u8>,
    cert_der: CertificateDer<'static>,
    fingerprint: Fingerprint,
}

/// Where the identity came from — the UI warns on `Regenerated`, because every
/// existing pairing becomes invalid when the key changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityOrigin {
    Created,
    Loaded,
    /// A certificate existed but its private key was gone (keychain wiped).
    Regenerated,
}

impl Identity {
    /// Generates a brand-new Ed25519 identity.
    pub fn generate() -> Result<Self> {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519).map_err(crypto)?;
        let pkcs8 = key.serialize_der();
        Self::from_key(pkcs8)
    }

    /// Rebuilds the identity (and a fresh self-signed certificate) from a PKCS#8 key.
    pub fn from_key(key_pkcs8: Vec<u8>) -> Result<Self> {
        let key = rcgen::KeyPair::try_from(key_pkcs8.as_slice()).map_err(crypto)?;
        let mut params = rcgen::CertificateParams::new(vec!["devti".to_string()]).map_err(crypto)?;
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.distinguished_name.push(rcgen::DnType::CommonName, "Devti device");
        params.not_before = rcgen::date_time_ymd(2024, 1, 1);
        params.not_after = rcgen::date_time_ymd(2124, 1, 1);
        let cert = params.self_signed(&key).map_err(crypto)?;
        let cert_der = cert.der().clone();
        Self::from_parts(key_pkcs8, cert_der.to_vec())
    }

    /// Assembles an identity from stored parts, verifying the key matches the cert.
    pub fn from_parts(key_pkcs8: Vec<u8>, cert_der: Vec<u8>) -> Result<Self> {
        use rcgen::PublicKeyData;
        let key = rcgen::KeyPair::try_from(key_pkcs8.as_slice()).map_err(crypto)?;
        let cert_der = CertificateDer::from(cert_der);
        let fingerprint = fingerprint_of(&cert_der)?;
        let key_fp = Fingerprint(sha256(&key.subject_public_key_info()));
        if key_fp != fingerprint {
            return Err(Error::Crypto("private key does not match certificate".into()));
        }
        Ok(Self { key_pkcs8, cert_der, fingerprint })
    }

    /// Loads the identity from `store` + `data_dir`, creating one on first run.
    pub fn load_or_create(store: &dyn SecretStore, data_dir: &Path) -> Result<(Self, IdentityOrigin)> {
        std::fs::create_dir_all(data_dir)?;
        let cert_path = data_dir.join(CERT_FILE_NAME);
        let key = store.get(KEY_SECRET_NAME)?;
        let cert = match std::fs::read(&cert_path) {
            Ok(c) => Some(c),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };

        let (identity, origin) = match (key, cert) {
            (Some(key), Some(cert)) => match Self::from_parts(key.clone(), cert) {
                Ok(id) => (id, IdentityOrigin::Loaded),
                // Cert corrupted or from another key: keep the key (and so the
                // device ID); just re-issue the certificate.
                Err(_) => (Self::from_key(key)?, IdentityOrigin::Loaded),
            },
            (Some(key), None) => (Self::from_key(key)?, IdentityOrigin::Loaded),
            (None, Some(_)) => (Self::generate()?, IdentityOrigin::Regenerated),
            (None, None) => (Self::generate()?, IdentityOrigin::Created),
        };

        if origin != IdentityOrigin::Loaded {
            store.set(KEY_SECRET_NAME, &identity.key_pkcs8)?;
        }
        std::fs::write(&cert_path, identity.cert_der.as_ref())?;
        Ok((identity, origin))
    }

    pub fn device_id(&self) -> DeviceId {
        self.fingerprint.device_id()
    }

    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    pub fn cert_der(&self) -> CertificateDer<'static> {
        self.cert_der.clone()
    }

    /// The 32-byte Ed25519 seed inside the PKCS#8 key. The "connect from anywhere"
    /// endpoint uses the same key pair, so a device has one identity on every transport.
    pub fn ed25519_seed(&self) -> Result<[u8; 32]> {
        // PKCS#8 wraps the seed as OCTET STRING { OCTET STRING (32 bytes) }: 04 22 04 20 <seed>.
        let k = &self.key_pkcs8;
        let at = k
            .windows(4)
            .position(|w| w == [0x04, 0x22, 0x04, 0x20])
            .ok_or_else(|| Error::Crypto("identity key is not an Ed25519 PKCS#8 key".into()))?;
        k.get(at + 4..at + 36)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::Crypto("identity key is truncated".into()))
    }

    pub fn private_key_der(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_pkcs8.clone()))
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.debug_struct("Identity").field("id", &self.device_id()).finish_non_exhaustive()
    }
}

/// SHA-256 over the certificate's SubjectPublicKeyInfo.
pub fn fingerprint_of(cert: &CertificateDer<'_>) -> Result<Fingerprint> {
    let (_, parsed) = x509_parser::parse_x509_certificate(cert.as_ref())
        .map_err(|e| Error::Crypto(format!("invalid certificate: {e}")))?;
    Ok(Fingerprint(sha256(parsed.public_key().raw)))
}

pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    let d = digest(&SHA256, data);
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_ref());
    out
}

fn crypto(e: impl std::fmt::Display) -> Error {
    Error::Crypto(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::MemorySecretStore;

    #[test]
    fn generated_identity_is_consistent() {
        let id = Identity::generate().unwrap();
        assert_eq!(fingerprint_of(&id.cert_der()).unwrap(), id.fingerprint());
        assert_eq!(id.device_id(), id.fingerprint().device_id());
        assert!(!format!("{id:?}").contains("key_pkcs8"));
        assert!(matches!(id.private_key_der(), PrivateKeyDer::Pkcs8(_)));
    }

    #[test]
    fn mismatched_key_and_cert_are_rejected() {
        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        let err = Identity::from_parts(a.key_pkcs8.clone(), b.cert_der.to_vec()).unwrap_err();
        assert!(err.to_string().contains("does not match"));
        assert!(Identity::from_parts(vec![1, 2, 3], a.cert_der.to_vec()).is_err());
        assert!(fingerprint_of(&CertificateDer::from(vec![0u8; 8])).is_err());
    }

    #[test]
    fn identity_persists_across_loads() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemorySecretStore::new();
        let (first, origin) = Identity::load_or_create(&store, dir.path()).unwrap();
        assert_eq!(origin, IdentityOrigin::Created);
        let (second, origin) = Identity::load_or_create(&store, dir.path()).unwrap();
        assert_eq!(origin, IdentityOrigin::Loaded);
        assert_eq!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn lost_certificate_is_reissued_with_same_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemorySecretStore::new();
        let (first, _) = Identity::load_or_create(&store, dir.path()).unwrap();
        std::fs::remove_file(dir.path().join(CERT_FILE_NAME)).unwrap();
        let (second, origin) = Identity::load_or_create(&store, dir.path()).unwrap();
        assert_eq!(origin, IdentityOrigin::Loaded);
        assert_eq!(first.device_id(), second.device_id());

        // A corrupted certificate is re-issued too.
        std::fs::write(dir.path().join(CERT_FILE_NAME), b"garbage").unwrap();
        let (third, origin) = Identity::load_or_create(&store, dir.path()).unwrap();
        assert_eq!(origin, IdentityOrigin::Loaded);
        assert_eq!(first.device_id(), third.device_id());
    }

    #[test]
    fn lost_key_regenerates_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemorySecretStore::new();
        let (first, _) = Identity::load_or_create(&store, dir.path()).unwrap();
        store.delete(KEY_SECRET_NAME).unwrap();
        let (second, origin) = Identity::load_or_create(&store, dir.path()).unwrap();
        assert_eq!(origin, IdentityOrigin::Regenerated);
        assert_ne!(first.device_id(), second.device_id());
    }
}
