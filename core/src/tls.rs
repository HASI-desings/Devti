//! TLS 1.3 configuration for QUIC: mutual authentication with pinned fingerprints.
//!
//! There is no certificate authority anywhere in Devti. Instead:
//!
//! * The **client** verifier accepts the server only if its SPKI fingerprint equals the
//!   one we expect — from the trust store for known devices, from the QR link while
//!   pairing. The only exception is code-based pairing, where the fingerprint isn't
//!   known yet; there the PAKE (bound to both fingerprints and the TLS exporter)
//!   authenticates the server instead.
//! * The **server** verifier asks a [`ClientPolicy`] whether the client's fingerprint
//!   is trusted, or whether a pairing window is open for strangers.
//!
//! In every case rustls still verifies the handshake signature, so the peer has proven
//! it holds the private key matching the certificate.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::error::Result;
use crate::identity::{Identity, fingerprint_of};
use crate::model::Fingerprint;
use crate::protocol::ALPN;

/// The TLS server name used on every connection. Ignored by our verifiers but required
/// by the TLS API.
pub const SERVER_NAME: &str = "devti";

/// The crypto provider used everywhere (ring, see DECISIONS D-11).
pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Decides whether an incoming client certificate may complete the handshake.
pub trait ClientPolicy: Send + Sync + std::fmt::Debug {
    fn allow(&self, fingerprint: &Fingerprint) -> bool;
}

/// What the client expects the server to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerExpectation {
    /// Must match exactly.
    Pinned(Fingerprint),
    /// Code-based pairing only: authenticity comes from the PAKE.
    AnyForPairing,
}

#[derive(Debug)]
struct PinnedServerVerifier {
    expect: ServerExpectation,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let fp = fingerprint_of(end_entity).map_err(|_| bad_cert())?;
        match self.expect {
            ServerExpectation::Pinned(expected) if expected == fp => Ok(ServerCertVerified::assertion()),
            ServerExpectation::Pinned(_) => Err(bad_cert()),
            ServerExpectation::AnyForPairing => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        // TLS 1.2 is never negotiated (QUIC is TLS 1.3 only).
        Err(rustls::Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

#[derive(Debug)]
struct PolicyClientVerifier {
    policy: Arc<dyn ClientPolicy>,
    algs: WebPkiSupportedAlgorithms,
}

impl ClientCertVerifier for PolicyClientVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        let fp = fingerprint_of(end_entity).map_err(|_| bad_cert())?;
        if self.policy.allow(&fp) { Ok(ClientCertVerified::assertion()) } else { Err(bad_cert()) }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerIncompatible(rustls::PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

fn bad_cert() -> rustls::Error {
    rustls::Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure)
}

/// rustls server config: our cert, mandatory client certs judged by `policy`.
pub fn server_config(identity: &Identity, policy: Arc<dyn ClientPolicy>) -> Result<rustls::ServerConfig> {
    let provider = provider();
    let algs = provider.signature_verification_algorithms;
    let verifier = Arc::new(PolicyClientVerifier { policy, algs });
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![identity.cert_der()], identity.private_key_der())?;
    cfg.alpn_protocols = vec![ALPN.to_vec()];
    Ok(cfg)
}

/// rustls client config: our cert for client auth, server judged by `expect`.
pub fn client_config(identity: &Identity, expect: ServerExpectation) -> Result<rustls::ClientConfig> {
    let provider = provider();
    let algs = provider.signature_verification_algorithms;
    let verifier = Arc::new(PinnedServerVerifier { expect, algs });
    let mut cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(vec![identity.cert_der()], identity.private_key_der())?;
    cfg.alpn_protocols = vec![ALPN.to_vec()];
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct AllowOnly(Fingerprint);
    impl ClientPolicy for AllowOnly {
        fn allow(&self, fp: &Fingerprint) -> bool {
            *fp == self.0
        }
    }

    #[test]
    fn verifiers_enforce_pins() {
        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        let algs = provider().signature_verification_algorithms;
        let name = ServerName::try_from(SERVER_NAME).unwrap();
        let now = UnixTime::now();

        let pinned = PinnedServerVerifier { expect: ServerExpectation::Pinned(a.fingerprint()), algs };
        assert!(pinned.verify_server_cert(&a.cert_der(), &[], &name, &[], now).is_ok());
        assert!(pinned.verify_server_cert(&b.cert_der(), &[], &name, &[], now).is_err());
        assert!(pinned.verify_server_cert(&CertificateDer::from(vec![1u8]), &[], &name, &[], now).is_err());
        assert!(!pinned.supported_verify_schemes().is_empty());

        let any = PinnedServerVerifier { expect: ServerExpectation::AnyForPairing, algs };
        assert!(any.verify_server_cert(&b.cert_der(), &[], &name, &[], now).is_ok());

        let client = PolicyClientVerifier { policy: Arc::new(AllowOnly(a.fingerprint())), algs };
        assert!(client.offer_client_auth() && client.client_auth_mandatory());
        assert!(client.root_hint_subjects().is_empty());
        assert!(client.verify_client_cert(&a.cert_der(), &[], now).is_ok());
        assert!(client.verify_client_cert(&b.cert_der(), &[], now).is_err());
        assert!(!client.supported_verify_schemes().is_empty());
    }

    #[test]
    fn configs_build_with_alpn() {
        let a = Identity::generate().unwrap();
        let s = server_config(&a, Arc::new(AllowOnly(a.fingerprint()))).unwrap();
        assert_eq!(s.alpn_protocols, vec![ALPN.to_vec()]);
        let c = client_config(&a, ServerExpectation::AnyForPairing).unwrap();
        assert_eq!(c.alpn_protocols, vec![ALPN.to_vec()]);
    }
}
