//! One connection to a peer, whichever way it travels.
//!
//! * [`Link::Quic`]: Devti's own QUIC endpoint, used on the local network, through a VPN
//!   or to a forwarded port. The peer is authenticated by its pinned certificate.
//! * [`Link::Iroh`]: the "connect from anywhere" endpoint. iroh punches through NATs and
//!   falls back to an encrypted relay. Peers are addressed by an Ed25519 public key, and
//!   a Devti identity *is* an Ed25519 key, so the peer's key gives the same fingerprint
//!   its certificate would. Trust checks don't change.
//!
//! Everything above this module (sessions, transfers, screens, audio, pairing) works on
//! [`Link`], [`LinkSend`] and [`LinkRecv`] and doesn't care which transport is underneath.

use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::error::{Error, Result};
use crate::identity::{fingerprint_of, sha256};
use crate::model::{ConnectionKind, Fingerprint};

/// DER prefix of an Ed25519 SubjectPublicKeyInfo; the 32-byte key follows it.
pub(crate) const ED25519_SPKI_PREFIX: [u8; 12] =
    [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];

/// The fingerprint a device with this Ed25519 public key has (SHA-256 of its SPKI).
pub(crate) fn fingerprint_of_key(key: &[u8; 32]) -> Fingerprint {
    let mut spki = Vec::with_capacity(44);
    spki.extend_from_slice(&ED25519_SPKI_PREFIX);
    spki.extend_from_slice(key);
    Fingerprint(sha256(&spki))
}

/// The raw Ed25519 public key inside a certificate, if it holds one.
pub(crate) fn key_of_cert(cert: &[u8]) -> Option<[u8; 32]> {
    let (_, parsed) = x509_parser::parse_x509_certificate(cert).ok()?;
    let raw = parsed.public_key().raw;
    if raw.len() == 44 && raw[..12] == ED25519_SPKI_PREFIX { raw[12..].try_into().ok() } else { None }
}

/// A live connection to one peer.
#[derive(Clone, Debug)]
pub(crate) enum Link {
    Quic(quinn::Connection),
    Iroh(iroh::endpoint::Connection),
}

/// How a link is currently travelling.
#[derive(Clone, Debug)]
pub(crate) struct LinkPath {
    pub kind: ConnectionKind,
    /// Shown to the user: an `ip:port`, or the relay's host name.
    pub address: String,
    /// The peer's socket address, when the path is direct.
    pub socket: Option<SocketAddr>,
    pub rtt: Duration,
}

/// Why a link closed.
#[derive(Clone, Debug)]
pub(crate) struct LinkClosed {
    /// The peer's application close code, when it closed the link on purpose.
    pub app_code: Option<u64>,
    pub text: String,
}

impl std::fmt::Display for LinkClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl Link {
    pub async fn open_bi(&self) -> Result<(LinkSend, LinkRecv)> {
        match self {
            Link::Quic(c) => {
                let (s, r) = c.open_bi().await.map_err(conn_err)?;
                Ok((LinkSend::Quic(s), LinkRecv::Quic(r)))
            }
            Link::Iroh(c) => {
                let (s, r) = c.open_bi().await.map_err(conn_err)?;
                Ok((LinkSend::Iroh(s), LinkRecv::Iroh(r)))
            }
        }
    }

    pub async fn accept_bi(&self) -> Result<(LinkSend, LinkRecv)> {
        match self {
            Link::Quic(c) => {
                let (s, r) = c.accept_bi().await.map_err(conn_err)?;
                Ok((LinkSend::Quic(s), LinkRecv::Quic(r)))
            }
            Link::Iroh(c) => {
                let (s, r) = c.accept_bi().await.map_err(conn_err)?;
                Ok((LinkSend::Iroh(s), LinkRecv::Iroh(r)))
            }
        }
    }

    pub fn close(&self, code: u32, reason: &[u8]) {
        match self {
            Link::Quic(c) => c.close(quinn::VarInt::from_u32(code), reason),
            Link::Iroh(c) => c.close(iroh::endpoint::VarInt::from_u32(code), reason),
        }
    }

    pub fn is_closed(&self) -> bool {
        match self {
            Link::Quic(c) => c.close_reason().is_some(),
            Link::Iroh(c) => c.close_reason().is_some(),
        }
    }

    /// Resolves when the link closes, for whatever reason.
    pub async fn closed(&self) -> LinkClosed {
        match self {
            Link::Quic(c) => {
                let e = c.closed().await;
                let app_code = match &e {
                    quinn::ConnectionError::ApplicationClosed(a) => Some(a.error_code.into_inner()),
                    _ => None,
                };
                LinkClosed { app_code, text: e.to_string() }
            }
            Link::Iroh(c) => {
                let e = c.closed().await;
                let app_code = match &e {
                    iroh::endpoint::ConnectionError::ApplicationClosed(a) => Some(a.error_code.into_inner()),
                    _ => None,
                };
                LinkClosed { app_code, text: e.to_string() }
            }
        }
    }

    /// Whether this link goes through Devti's own QUIC endpoint.
    pub fn is_quic(&self) -> bool {
        matches!(self, Link::Quic(_))
    }

    /// The path the link currently uses.
    pub fn path(&self) -> LinkPath {
        match self {
            Link::Quic(c) => {
                let a = c.remote_address();
                LinkPath { kind: ConnectionKind::of(&a), address: a.to_string(), socket: Some(a), rtt: c.rtt() }
            }
            Link::Iroh(c) => {
                let paths = c.paths();
                let selected = paths.iter().find(|p| p.is_selected()).or_else(|| paths.iter().next());
                match selected {
                    Some(p) => match p.remote_addr() {
                        iroh::TransportAddr::Ip(a) => LinkPath {
                            kind: match ConnectionKind::of(a) {
                                ConnectionKind::Lan => ConnectionKind::Lan,
                                ConnectionKind::Vpn => ConnectionKind::Vpn,
                                _ => ConnectionKind::Internet,
                            },
                            address: a.to_string(),
                            socket: Some(*a),
                            rtt: p.rtt(),
                        },
                        iroh::TransportAddr::Relay(url) => LinkPath {
                            kind: ConnectionKind::Relay,
                            address: url.host_str().unwrap_or("relay").to_string(),
                            socket: None,
                            rtt: p.rtt(),
                        },
                        _ => LinkPath {
                            kind: ConnectionKind::Internet,
                            address: "internet".into(),
                            socket: None,
                            rtt: p.rtt(),
                        },
                    },
                    None => LinkPath {
                        kind: ConnectionKind::Relay,
                        address: "relay".into(),
                        socket: None,
                        rtt: Duration::ZERO,
                    },
                }
            }
        }
    }

    /// RFC 5705 keying material bound to this link's TLS session (used by pairing).
    pub fn export_keying_material(&self, out: &mut [u8], label: &[u8], context: &[u8]) -> Result<()> {
        let ok = match self {
            Link::Quic(c) => c.export_keying_material(out, label, context).is_ok(),
            Link::Iroh(c) => c.export_keying_material(out, label, context).is_ok(),
        };
        if ok { Ok(()) } else { Err(Error::Tls("TLS exporter unavailable".into())) }
    }

    /// The fingerprint the peer authenticated with.
    pub fn peer_fingerprint(&self) -> Result<Fingerprint> {
        match self {
            Link::Quic(c) => {
                let certs = c
                    .peer_identity()
                    .and_then(|id| id.downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>().ok())
                    .ok_or_else(|| Error::Tls("peer presented no certificate".into()))?;
                let first = certs.first().ok_or_else(|| Error::Tls("peer presented no certificate".into()))?;
                fingerprint_of(first)
            }
            Link::Iroh(c) => Ok(fingerprint_of_key(c.remote_id().as_bytes())),
        }
    }

    /// The peer's Ed25519 public key (what the anywhere endpoint dials).
    pub fn peer_key(&self) -> Option<[u8; 32]> {
        match self {
            Link::Quic(c) => {
                let certs = c.peer_identity()?.downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>().ok()?;
                key_of_cert(certs.first()?.as_ref())
            }
            Link::Iroh(c) => Some(*c.remote_id().as_bytes()),
        }
    }
}

fn conn_err(e: impl std::fmt::Display) -> Error {
    Error::Connection(e.to_string())
}

/// The sending half of a stream.
pub(crate) enum LinkSend {
    Quic(quinn::SendStream),
    Iroh(iroh::endpoint::SendStream),
}

impl LinkSend {
    pub async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match self {
            LinkSend::Quic(s) => s.write_all(buf).await.map_err(conn_err),
            LinkSend::Iroh(s) => s.write_all(buf).await.map_err(conn_err),
        }
    }

    /// Ends the stream cleanly once everything written has been delivered.
    pub fn finish(&mut self) -> Result<()> {
        match self {
            LinkSend::Quic(s) => s.finish().map_err(conn_err),
            LinkSend::Iroh(s) => s.finish().map_err(conn_err),
        }
    }

    /// Abandons the stream; the peer sees `code`.
    pub fn reset(&mut self, code: u32) -> Result<()> {
        match self {
            LinkSend::Quic(s) => s.reset(quinn::VarInt::from_u32(code)).map_err(conn_err),
            LinkSend::Iroh(s) => s.reset(iroh::endpoint::VarInt::from_u32(code)).map_err(conn_err),
        }
    }
}

impl AsyncWrite for LinkSend {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            LinkSend::Quic(s) => AsyncWrite::poll_write(Pin::new(s), cx, buf),
            LinkSend::Iroh(s) => AsyncWrite::poll_write(Pin::new(s), cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            LinkSend::Quic(s) => AsyncWrite::poll_flush(Pin::new(s), cx),
            LinkSend::Iroh(s) => AsyncWrite::poll_flush(Pin::new(s), cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            LinkSend::Quic(s) => AsyncWrite::poll_shutdown(Pin::new(s), cx),
            LinkSend::Iroh(s) => AsyncWrite::poll_shutdown(Pin::new(s), cx),
        }
    }
}

/// Why reading a stream failed.
#[derive(Debug)]
pub(crate) enum LinkReadError {
    /// The sender reset the stream with this code.
    Reset(u64),
    Other(String),
}

impl std::fmt::Display for LinkReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkReadError::Reset(c) => write!(f, "stream reset by peer (code {c})"),
            LinkReadError::Other(s) => f.write_str(s),
        }
    }
}

/// The receiving half of a stream.
pub(crate) enum LinkRecv {
    Quic(quinn::RecvStream),
    Iroh(iroh::endpoint::RecvStream),
}

impl LinkRecv {
    /// Reads some bytes; `Ok(None)` at the end of the stream.
    pub async fn read(&mut self, buf: &mut [u8]) -> std::result::Result<Option<usize>, LinkReadError> {
        match self {
            LinkRecv::Quic(s) => s.read(buf).await.map_err(|e| match e {
                quinn::ReadError::Reset(c) => LinkReadError::Reset(c.into_inner()),
                e => LinkReadError::Other(e.to_string()),
            }),
            LinkRecv::Iroh(s) => s.read(buf).await.map_err(|e| match e {
                iroh::endpoint::ReadError::Reset(c) => LinkReadError::Reset(c.into_inner()),
                e => LinkReadError::Other(e.to_string()),
            }),
        }
    }

    /// Tells the sender to stop; it sees `code`.
    pub fn stop(&mut self, code: u32) -> Result<()> {
        match self {
            LinkRecv::Quic(s) => s.stop(quinn::VarInt::from_u32(code)).map_err(conn_err),
            LinkRecv::Iroh(s) => s.stop(iroh::endpoint::VarInt::from_u32(code)).map_err(conn_err),
        }
    }
}

impl AsyncRead for LinkRecv {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            LinkRecv::Quic(s) => AsyncRead::poll_read(Pin::new(s), cx, buf),
            LinkRecv::Iroh(s) => AsyncRead::poll_read(Pin::new(s), cx, buf),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    #[test]
    fn key_fingerprint_matches_certificate_fingerprint() {
        let id = Identity::generate().unwrap();
        let key = key_of_cert(id.cert_der().as_ref()).expect("Devti identities are Ed25519");
        assert_eq!(fingerprint_of_key(&key), id.fingerprint());
    }

    #[test]
    fn identity_seed_gives_the_certificate_key() {
        let id = Identity::generate().unwrap();
        let secret = iroh::SecretKey::from_bytes(&id.ed25519_seed().unwrap());
        let key = key_of_cert(id.cert_der().as_ref()).unwrap();
        assert_eq!(secret.public().as_bytes(), &key);
    }
}
