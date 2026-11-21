//! Pairing: turning a stranger into a trusted device.
//!
//! * [`uri`] — the QR-code link format.
//! * [`pake`] — SPAKE2 + key confirmation + short authentication string.
//!
//! The network choreography that drives these lives in [`crate::node`].

pub mod pake;
pub mod uri;

pub use pake::{Binding, PairKeys, PakeState, Side, format_code};
pub use uri::{PairingUri, QR_SECRET_LEN};

use crate::error::{Error, Result};

/// Fresh 128-bit QR secret.
pub fn new_qr_secret() -> Result<[u8; QR_SECRET_LEN]> {
    let mut s = [0u8; QR_SECRET_LEN];
    getrandom::fill(&mut s).map_err(|e| Error::Crypto(e.to_string()))?;
    Ok(s)
}

/// Uniformly random 6-digit code (rejection sampling avoids modulo bias).
pub fn new_code() -> Result<String> {
    const LIMIT: u32 = u32::MAX - (u32::MAX % 1_000_000);
    loop {
        let n = getrandom::u32().map_err(|e| Error::Crypto(e.to_string()))?;
        if n < LIMIT {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

/// Random identifier for a pairing attempt / window.
pub fn new_id() -> Result<u64> {
    getrandom::u64().map_err(|e| Error::Crypto(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_digits_and_vary() {
        let codes: std::collections::HashSet<_> = (0..50).map(|_| new_code().unwrap()).collect();
        assert!(codes.iter().all(|c| c.len() == 6 && c.chars().all(|d| d.is_ascii_digit())));
        assert!(codes.len() > 40);
        assert_ne!(new_qr_secret().unwrap(), new_qr_secret().unwrap());
        assert_ne!(new_id().unwrap(), new_id().unwrap());
    }
}
