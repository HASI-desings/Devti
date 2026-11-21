//! The cryptographic heart of pairing: SPAKE2 + key confirmation + SAS.
//!
//! Both devices know a low- or high-entropy secret (6-digit code or the QR secret).
//! SPAKE2 turns it into a strong shared key `K` without ever revealing the secret, and
//! an eavesdropper or active attacker gets exactly one online guess per attempt.
//!
//! `K` is then bound to the TLS session it ran on via the *transcript*:
//!
//! ```text
//! transcript = "devti-pair-v1" ‖ tls_exporter(32) ‖ hub_fp ‖ joiner_fp ‖ len‖msgA ‖ len‖msgB
//! keys       = HKDF-SHA256(salt = SHA256(transcript), ikm = K)
//! ```
//!
//! Each side proves knowledge of `K` with an HMAC over the transcript. A man in the
//! middle terminates two TLS sessions with different exporters and fingerprints, so
//! the MACs can never verify on both legs. The 6-digit SAS is derived from the same
//! keys and shown on both screens as a final human check.

use ring::{hkdf, hmac};
use spake2::{Ed25519Group, Identity as SpakeIdentity, Password, Spake2};

use crate::error::{Error, Result};
use crate::identity::sha256;
use crate::model::Fingerprint;

const TRANSCRIPT_LABEL: &[u8] = b"devti-pair-v1";
const ID_HUB: &[u8] = b"devti-hub";
const ID_JOINER: &[u8] = b"devti-joiner";
/// Label for the RFC 5705/8446 TLS exporter.
pub const EXPORTER_LABEL: &[u8] = b"EXPORTER-devti-pairing";

/// Which end of the pairing this device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Opened the pairing window (shows the QR / code).
    Hub,
    /// Scanned / typed the secret and dialled in.
    Joiner,
}

/// Binds the PAKE to one concrete TLS session.
#[derive(Clone, Debug)]
pub struct Binding {
    pub exporter: [u8; 32],
    pub hub_fp: Fingerprint,
    pub joiner_fp: Fingerprint,
}

/// In-progress PAKE.
pub struct PakeState {
    side: Side,
    inner: Spake2<Ed25519Group>,
    outbound: Vec<u8>,
}

impl PakeState {
    /// Starts SPAKE2 with the given password bytes.
    pub fn start(side: Side, password: &[u8]) -> Self {
        let pw = Password::new(password);
        let (a, b) = (SpakeIdentity::new(ID_HUB), SpakeIdentity::new(ID_JOINER));
        let (inner, outbound) = match side {
            Side::Hub => Spake2::<Ed25519Group>::start_a(&pw, &a, &b),
            Side::Joiner => Spake2::<Ed25519Group>::start_b(&pw, &a, &b),
        };
        Self { side, inner, outbound }
    }

    /// Our SPAKE2 message, to send to the peer.
    pub fn message(&self) -> &[u8] {
        &self.outbound
    }

    /// Completes SPAKE2 with the peer's message and derives the confirmation keys.
    pub fn finish(self, peer_msg: &[u8], binding: &Binding) -> Result<PairKeys> {
        let (msg_hub, msg_joiner) = match self.side {
            Side::Hub => (self.outbound.clone(), peer_msg.to_vec()),
            Side::Joiner => (peer_msg.to_vec(), self.outbound.clone()),
        };
        let k = self.inner.finish(peer_msg).map_err(|e| Error::Pairing(format!("malformed PAKE message: {e:?}")))?;

        let mut transcript = Vec::with_capacity(256);
        transcript.extend_from_slice(TRANSCRIPT_LABEL);
        transcript.extend_from_slice(&binding.exporter);
        transcript.extend_from_slice(&binding.hub_fp.0);
        transcript.extend_from_slice(&binding.joiner_fp.0);
        for m in [&msg_hub, &msg_joiner] {
            transcript.extend_from_slice(&(m.len() as u32).to_be_bytes());
            transcript.extend_from_slice(m);
        }

        let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, &sha256(&transcript));
        let prk = salt.extract(&k);
        let hub_key: [u8; 32] = expand(&prk, b"devti hub confirm")?;
        let joiner_key: [u8; 32] = expand(&prk, b"devti joiner confirm")?;
        let sas_bytes: [u8; 4] = expand(&prk, b"devti sas")?;
        let sas_num = u32::from_be_bytes(sas_bytes) % 1_000_000;

        Ok(PairKeys {
            side: self.side,
            hub_key: hmac::Key::new(hmac::HMAC_SHA256, &hub_key),
            joiner_key: hmac::Key::new(hmac::HMAC_SHA256, &joiner_key),
            transcript,
            sas: format!("{sas_num:06}"),
        })
    }
}

/// Keys derived from a completed PAKE.
pub struct PairKeys {
    side: Side,
    hub_key: hmac::Key,
    joiner_key: hmac::Key,
    transcript: Vec<u8>,
    sas: String,
}

impl PairKeys {
    /// Our key-confirmation MAC.
    pub fn my_mac(&self) -> Vec<u8> {
        let key = match self.side {
            Side::Hub => &self.hub_key,
            Side::Joiner => &self.joiner_key,
        };
        hmac::sign(key, &self.transcript).as_ref().to_vec()
    }

    /// Verifies the peer's MAC in constant time.
    pub fn verify_peer_mac(&self, mac: &[u8]) -> bool {
        let key = match self.side {
            Side::Hub => &self.joiner_key,
            Side::Joiner => &self.hub_key,
        };
        hmac::verify(key, &self.transcript, mac).is_ok()
    }

    /// Six-digit short authentication string, e.g. `"048213"`.
    pub fn sas(&self) -> &str {
        &self.sas
    }
}

struct Len(usize);

impl hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn expand<const N: usize>(prk: &hkdf::Prk, info: &[u8]) -> Result<[u8; N]> {
    let info = [info];
    let okm = prk.expand(&info, Len(N)).map_err(|_| Error::Crypto("hkdf expand".into()))?;
    let mut out = [0u8; N];
    okm.fill(&mut out).map_err(|_| Error::Crypto("hkdf fill".into()))?;
    Ok(out)
}

/// Password bytes for a pairing method. The prefix stops a QR secret from ever being
/// accepted as a typed code and vice versa.
pub fn password_for_qr(secret: &[u8]) -> Vec<u8> {
    [b"qr:".as_slice(), secret].concat()
}

/// See [`password_for_qr`].
pub fn password_for_code(code: &str) -> Vec<u8> {
    let digits: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    [b"code:".as_slice(), digits.as_bytes()].concat()
}

/// Formats a 6-digit code/SAS for display: `"482913"` → `"482 913"`.
pub fn format_code(code: &str) -> String {
    if code.len() == 6 { format!("{} {}", &code[..3], &code[3..]) } else { code.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> Binding {
        Binding { exporter: [5; 32], hub_fp: Fingerprint([1; 32]), joiner_fp: Fingerprint([2; 32]) }
    }

    fn run(pw_hub: &[u8], pw_joiner: &[u8], b_hub: &Binding, b_joiner: &Binding) -> (PairKeys, PairKeys) {
        let hub = PakeState::start(Side::Hub, pw_hub);
        let joiner = PakeState::start(Side::Joiner, pw_joiner);
        let (hm, jm) = (hub.message().to_vec(), joiner.message().to_vec());
        (hub.finish(&jm, b_hub).unwrap(), joiner.finish(&hm, b_joiner).unwrap())
    }

    #[test]
    fn same_secret_same_binding_confirms() {
        let b = binding();
        let (h, j) = run(b"code:123456", b"code:123456", &b, &b);
        assert!(h.verify_peer_mac(&j.my_mac()));
        assert!(j.verify_peer_mac(&h.my_mac()));
        assert_eq!(h.sas(), j.sas());
        assert_eq!(h.sas().len(), 6);
        // A side can't replay its own MAC as the peer's.
        assert!(!h.verify_peer_mac(&h.my_mac()));
    }

    #[test]
    fn wrong_secret_fails_confirmation() {
        let b = binding();
        let (h, j) = run(b"code:123456", b"code:123457", &b, &b);
        assert!(!h.verify_peer_mac(&j.my_mac()));
        assert!(!j.verify_peer_mac(&h.my_mac()));
    }

    #[test]
    fn different_tls_sessions_fail_confirmation() {
        // Simulates a MITM: each leg has its own exporter and sees a different fingerprint.
        let b1 = binding();
        let mut b2 = binding();
        b2.exporter = [6; 32];
        let (h, j) = run(b"qr:x", b"qr:x", &b1, &b2);
        assert!(!h.verify_peer_mac(&j.my_mac()));
        let mut b3 = binding();
        b3.hub_fp = Fingerprint([9; 32]);
        let (h, j) = run(b"qr:x", b"qr:x", &b1, &b3);
        assert!(!h.verify_peer_mac(&j.my_mac()));
    }

    #[test]
    fn malformed_peer_message_is_an_error() {
        let hub = PakeState::start(Side::Hub, b"x");
        assert!(hub.finish(&[1, 2, 3], &binding()).is_err());
    }

    #[test]
    fn password_helpers() {
        assert_eq!(password_for_code("482 913"), b"code:482913".to_vec());
        assert_eq!(password_for_qr(&[1]), b"qr:\x01".to_vec());
        assert_ne!(password_for_code("1"), password_for_qr(b"1"));
        assert_eq!(format_code("482913"), "482 913");
        assert_eq!(format_code("12"), "12");
    }
}
