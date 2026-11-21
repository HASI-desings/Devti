//! TLS admission policy, consulted *during* the handshake.
//!
//! A client certificate is admitted when the ecosystem isn't paused and either the
//! fingerprint is trusted, or a pairing window is open (strangers may then only use
//! the pairing stream — enforced again at the application layer).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;

use crate::model::Fingerprint;
use crate::tls::ClientPolicy;

#[derive(Debug, Default)]
pub(crate) struct TrustView {
    trusted: RwLock<HashSet<Fingerprint>>,
    pairing_open: AtomicBool,
    paused: AtomicBool,
}

impl TrustView {
    pub fn new(fps: impl IntoIterator<Item = Fingerprint>) -> Self {
        Self { trusted: RwLock::new(fps.into_iter().collect()), ..Default::default() }
    }

    pub fn insert(&self, fp: Fingerprint) {
        self.trusted.write().insert(fp);
    }

    pub fn remove(&self, fp: &Fingerprint) {
        self.trusted.write().remove(fp);
    }

    pub fn is_trusted(&self, fp: &Fingerprint) -> bool {
        self.trusted.read().contains(fp)
    }

    pub fn set_pairing_open(&self, open: bool) {
        self.pairing_open.store(open, Ordering::SeqCst);
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }
}

impl ClientPolicy for TrustView {
    fn allow(&self, fp: &Fingerprint) -> bool {
        !self.paused.load(Ordering::SeqCst) && (self.is_trusted(fp) || self.pairing_open.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_matrix() {
        let a = Fingerprint([1; 32]);
        let b = Fingerprint([2; 32]);
        let t = TrustView::new([a]);
        assert!(t.allow(&a));
        assert!(!t.allow(&b));
        t.set_pairing_open(true);
        assert!(t.allow(&b));
        t.set_paused(true);
        assert!(!t.allow(&a));
        assert!(!t.allow(&b));
        t.set_paused(false);
        t.set_pairing_open(false);
        t.insert(b);
        assert!(t.allow(&b));
        t.remove(&b);
        assert!(!t.allow(&b));
    }
}
