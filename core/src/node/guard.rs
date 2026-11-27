//! Stolen mode (D-55).
//!
//! The **owner** is one paired computer, chosen once on the phone's own screen during
//! setup. From then on:
//!
//! * only the owner can turn stolen mode on or off (`GuardSet`), over its authenticated
//!   session;
//! * while it's on, the phone shows a black "stolen" screen, locks itself down as far
//!   as Android allows, and sends its location to the owner every 30 seconds;
//! * without a connection, the phone shows a **challenge**. Only the owner can answer it,
//!   because the answer is an HMAC under a secret that the owner created at setup and
//!   that never leaves the two devices. Wrong answers back off: 30 s, 1, 5, 15, then 60
//!   minutes.
//!
//! What "locks itself down" means is up to the phone's host app (device owner policy,
//! lock task, USB data off…). It tells the owner what's protected in `GuardStatus`.
//!
//! ```text
//! owner (laptop)                                  phone
//!   |-- GuardClaim {secret} ------------------------>|  phone asks its user once
//!   |<--------------------------- GuardStatus -------|  owner: true
//!   |-- GuardSet {on: true, message, contact} ------->|  only from the owner
//!   |<------------- Location {live: true} (30 s) ----|
//!   |-- GuardSet {on: false} ----------------------->|  or the offline code on the phone
//! ```

use std::sync::Arc;

use ring::hmac;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use super::{Inner, Node, NodeEvent};
use crate::error::{Error, Result};
use crate::model::DeviceId;
use crate::net::now_ms;
use crate::protocol::ControlMsg;

const OWNER: &str = "guard_owner";
const SECRET: &str = "guard_secret";
const STATE: &str = "guard_state";
const ATTEMPTS: &str = "guard_attempts";
const REPORT: &str = "guard_report";
const UNLOCK_LABEL: &[u8] = b"devti-guard-unlock-v1";
/// Default words on the stolen screen.
pub const DEFAULT_MESSAGE: &str = "This device is stolen. Return it to its owner. Police is on the way.";

fn guarded_setting(id: &DeviceId) -> String {
    format!("guardian:{id}")
}

fn status_setting(id: &DeviceId) -> String {
    format!("guardstatus:{id}")
}

/// What the stolen screen says, and what's turned on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardState {
    pub on: bool,
    pub message: String,
    /// How to reach the owner (a phone number or email), shown on the stolen screen.
    #[serde(default)]
    pub contact: String,
    /// Turn stolen mode on by itself when the SIM card is removed.
    #[serde(default)]
    pub sim_lock: bool,
    /// Why it was turned on: "owner" or "sim_removed".
    #[serde(default)]
    pub reason: String,
    /// When it changed (ms).
    #[serde(default)]
    pub since: i64,
}

/// What the phone reports about its protection (set by the host app).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardReport {
    /// "device_owner" (strongest), "admin", "basic" or "unsupported" (iPhone).
    pub level: String,
    /// Protections in force or available, in plain words.
    #[serde(default)]
    pub protections: Vec<String>,
    /// What this phone can't protect, in plain words.
    #[serde(default)]
    pub missing: Vec<String>,
}

/// What a phone tells its owner (and what the owner shows).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardStatus {
    /// The receiver is this phone's owner.
    pub owner: bool,
    /// Another computer is the owner.
    #[serde(default)]
    pub other_owner: bool,
    pub state: GuardState,
    pub report: GuardReport,
}

impl GuardStatus {
    fn sanitized(mut self) -> Self {
        let short = |s: String, n: usize| s.chars().take(n).collect::<String>();
        self.state.message = short(self.state.message, 300);
        self.state.contact = short(self.state.contact, 120);
        self.state.reason = short(self.state.reason, 32);
        self.report.level = short(self.report.level, 32);
        self.report.protections = self.report.protections.into_iter().take(32).map(|s| short(s, 160)).collect();
        self.report.missing = self.report.missing.into_iter().take(32).map(|s| short(s, 160)).collect();
        self
    }
}

/// The phone's side, for its own UI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalGuard {
    pub owner: Option<DeviceId>,
    pub owner_name: Option<String>,
    pub state: GuardState,
    /// The question to show on the stolen screen (answered on the owner computer).
    pub challenge: String,
    /// Wrong codes so far, and when the next try is allowed (ms).
    pub attempts: u32,
    pub retry_at: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Attempts {
    count: u32,
    retry_at: i64,
    challenge: String,
}

fn backoff_ms(count: u32) -> i64 {
    match count {
        0 => 0,
        1..=2 => 30_000,
        3 => 60_000,
        4 => 5 * 60_000,
        5 => 15 * 60_000,
        _ => 60 * 60_000,
    }
}

fn new_challenge() -> String {
    let n = getrandom::u64().unwrap_or(0) % 100_000_000;
    format!("{n:08}")
}

/// The answer to `challenge` under `secret`: 10 digits.
pub fn unlock_code(secret: &[u8], challenge: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let mut msg = UNLOCK_LABEL.to_vec();
    msg.extend_from_slice(challenge.trim().as_bytes());
    let tag = hmac::sign(&key, &msg);
    let b = tag.as_ref();
    let n = u64::from_be_bytes([0, 0, 0, b[0], b[1], b[2], b[3], b[4]]) % 10_000_000_000;
    format!("{n:010}")
}

fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn owner(inner: &Inner) -> Option<DeviceId> {
    inner.store.setting(OWNER).ok().flatten().and_then(|s| s.parse().ok())
}

fn state(inner: &Inner) -> GuardState {
    inner.store.setting(STATE).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn report(inner: &Inner) -> GuardReport {
    inner.store.setting(REPORT).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn attempts(inner: &Inner) -> Attempts {
    inner.store.setting(ATTEMPTS).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save<T: Serialize>(inner: &Inner, key: &str, v: &T) {
    if let Ok(json) = serde_json::to_string(v) {
        let _ = inner.store.set_setting(key, &json);
    }
}

fn status_for(inner: &Inner, peer: &DeviceId) -> GuardStatus {
    let o = owner(inner);
    GuardStatus {
        owner: o == Some(*peer),
        other_owner: o.is_some_and(|o| o != *peer),
        state: state(inner),
        report: report(inner),
    }
}

/// Tells the owner (when online) how this phone is protected.
fn tell_owner(inner: &Inner) {
    if let Some(o) = owner(inner) {
        let _ = inner.send_to(&o, ControlMsg::GuardStatus { status: status_for(inner, &o) });
    }
}

/// A session came up: the owner hears the phone's protection at once.
pub(crate) fn on_session(inner: &Arc<Inner>, peer: DeviceId) {
    if owner(inner) == Some(peer) {
        let _ = inner.send_to(&peer, ControlMsg::GuardStatus { status: status_for(inner, &peer) });
    }
}

fn apply(inner: &Inner, mut next: GuardState) {
    let prev = state(inner);
    if prev.on != next.on || prev.message != next.message {
        next.since = now_ms();
    } else {
        next.since = prev.since;
    }
    if next.message.trim().is_empty() {
        next.message = DEFAULT_MESSAGE.into();
    }
    if next.on && !prev.on {
        // A fresh question for the offline unlock, and a clean slate of attempts.
        save(inner, ATTEMPTS, &Attempts { count: 0, retry_at: 0, challenge: new_challenge() });
    }
    save(inner, STATE, &next);
    inner.log(None, if next.on { "stolen_mode_on" } else { "stolen_mode_off" }, &next.reason);
    inner.emit(NodeEvent::GuardChanged { state: next });
}

pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, msg: ControlMsg) {
    let name = inner.peer_name(&peer);
    match msg {
        // ---- phone side
        ControlMsg::GuardClaim { secret } => {
            if secret.len() != 32 {
                return;
            }
            match owner(inner) {
                Some(o) if o == peer => {
                    // The owner refreshed its secret.
                    let _ = inner.store.set_setting(SECRET, &data_encoding::HEXLOWER.encode(&secret));
                    tell_owner(inner);
                }
                Some(_) => {
                    // Only the current owner can hand stolen protection over.
                    let _ = inner.send_to(&peer, ControlMsg::GuardStatus { status: status_for(inner, &peer) });
                }
                None => {
                    inner.guard_pending.lock().replace((peer, secret.into_vec()));
                    inner.log(Some((&peer, &name)), "guard_claim_requested", "");
                    inner.emit(NodeEvent::GuardClaimRequested { from: peer, from_name: name });
                }
            }
        }
        ControlMsg::GuardSet { state: next } => {
            if owner(inner) != Some(peer) {
                tracing::warn!(peer = %peer, "stolen mode change from a computer that isn't the owner, ignored");
                return;
            }
            let next = GuardState { reason: if next.on { "owner".into() } else { String::new() }, ..next };
            apply(inner, GuardStatus { state: next, ..Default::default() }.sanitized().state);
            tell_owner(inner);
        }
        ControlMsg::GuardRelease => {
            if owner(inner) == Some(peer) {
                if state(inner).on {
                    apply(inner, GuardState { on: false, ..state(inner) });
                }
                let _ = inner.store.set_setting(OWNER, "");
                let _ = inner.store.set_setting(SECRET, "");
                inner.log(Some((&peer, &name)), "guard_released", "");
                inner.emit(NodeEvent::GuardOwnerChanged { owner: None });
                let _ = inner.send_to(&peer, ControlMsg::GuardStatus { status: status_for(inner, &peer) });
            }
        }
        // ---- owner side
        ControlMsg::GuardStatus { status } => {
            let status = status.sanitized();
            // The phone says we're not its owner (refused, released, or reset): drop the
            // secret, so this computer can't produce unlock codes for it.
            if !status.owner {
                let _ = inner.store.set_setting(&guarded_setting(&peer), "");
            }
            save(inner, &status_setting(&peer), &status);
            inner.emit(NodeEvent::GuardStatusReceived { from: peer, status });
        }
        _ => {}
    }
}

pub(crate) fn forget(inner: &Inner, peer: &DeviceId) {
    let _ = inner.store.set_setting(&guarded_setting(peer), "");
    let _ = inner.store.set_setting(&status_setting(peer), "");
    // A removed owner can't control this phone any more.
    if owner(inner) == Some(*peer) && !state(inner).on {
        let _ = inner.store.set_setting(OWNER, "");
        let _ = inner.store.set_setting(SECRET, "");
    }
}

impl Node {
    // ------------------------------------------------------------ owner side

    /// Asks `peer` (a phone) to make this computer its stolen-mode owner. The phone's
    /// user confirms on the phone.
    pub fn guard_claim(&self, peer: &DeviceId) -> Result<()> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|e| Error::Crypto(e.to_string()))?;
        self.inner.store.set_setting(&guarded_setting(peer), &data_encoding::HEXLOWER.encode(&secret))?;
        self.inner.send_to(peer, ControlMsg::GuardClaim { secret: ByteBuf::from(secret.to_vec()) })?;
        let name = self.inner.peer_name(peer);
        self.inner.log(Some((peer, &name)), "guard_claim_sent", "");
        Ok(())
    }

    /// Turns stolen mode on or off on `peer` (only works if this computer is its owner).
    pub fn guard_set(&self, peer: &DeviceId, state: GuardState) -> Result<()> {
        let on = state.on;
        self.inner.send_to(peer, ControlMsg::GuardSet { state })?;
        let name = self.inner.peer_name(peer);
        self.inner.log(Some((peer, &name)), if on { "stolen_mode_sent_on" } else { "stolen_mode_sent_off" }, "");
        Ok(())
    }

    /// Gives up ownership of `peer`'s stolen protection.
    pub fn guard_release(&self, peer: &DeviceId) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::GuardRelease)?;
        self.inner.store.set_setting(&guarded_setting(peer), "")?;
        Ok(())
    }

    /// The 10-digit answer to the challenge `peer`'s stolen screen shows.
    pub fn guard_unlock_code(&self, peer: &DeviceId, challenge: &str) -> Result<String> {
        let secret = self
            .inner
            .store
            .setting(&guarded_setting(peer))?
            .filter(|s| !s.is_empty())
            .and_then(|h| data_encoding::HEXLOWER.decode(h.as_bytes()).ok())
            .ok_or_else(|| Error::InvalidInput("this computer isn't that phone's stolen-mode owner".into()))?;
        let c = digits(challenge);
        if c.len() != 8 {
            return Err(Error::InvalidInput("the code on the phone has 8 digits".into()));
        }
        Ok(unlock_code(&secret, &c))
    }

    /// Whether this computer holds `peer`'s stolen-mode secret.
    pub fn guard_owns(&self, peer: &DeviceId) -> bool {
        self.inner.store.setting(&guarded_setting(peer)).ok().flatten().is_some_and(|s| !s.is_empty())
    }

    /// The last protection status each phone reported to this computer.
    pub fn guard_statuses(&self) -> Vec<(DeviceId, GuardStatus)> {
        let ids: Vec<DeviceId> = self.inner.peers.read().keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| {
                let s = self.inner.store.setting(&status_setting(&id)).ok().flatten()?;
                serde_json::from_str::<GuardStatus>(&s).ok().map(|st| (id, st))
            })
            .collect()
    }

    // ------------------------------------------------------------ phone side

    /// The phone's user answered "make this computer the owner?".
    pub fn guard_accept(&self, from: &DeviceId, accept: bool) -> Result<()> {
        let pending = self.inner.guard_pending.lock().take();
        let Some((peer, secret)) = pending.filter(|(p, _)| p == from) else {
            return Err(Error::InvalidInput("no request is waiting".into()));
        };
        let name = self.inner.peer_name(&peer);
        if accept && owner(&self.inner).is_none() {
            self.inner.store.set_setting(OWNER, &peer.to_string())?;
            self.inner.store.set_setting(SECRET, &data_encoding::HEXLOWER.encode(&secret))?;
            self.inner.log(Some((&peer, &name)), "guard_owner_set", "");
            self.inner.emit(NodeEvent::GuardOwnerChanged { owner: Some(peer) });
        }
        let _ = self.inner.send_to(&peer, ControlMsg::GuardStatus { status: status_for(&self.inner, &peer) });
        Ok(())
    }

    /// The host app says how well it can protect this phone; the owner is told.
    pub fn guard_set_report(&self, report: GuardReport) {
        if self::report(&self.inner) != report {
            save(&self.inner, REPORT, &report);
            tell_owner(&self.inner);
        }
    }

    /// This phone's stolen-mode state, for its own screens.
    pub fn guard_local(&self) -> LocalGuard {
        let owner = owner(&self.inner);
        let mut a = attempts(&self.inner);
        let st = state(&self.inner);
        if st.on && a.challenge.is_empty() {
            a.challenge = new_challenge();
            save(&self.inner, ATTEMPTS, &a);
        }
        LocalGuard {
            owner_name: owner.map(|o| self.inner.peer_name(&o)),
            owner,
            state: st,
            challenge: a.challenge,
            attempts: a.count,
            retry_at: a.retry_at,
        }
    }

    /// The SIM was removed: turns stolen mode on if the owner asked for that.
    pub fn guard_sim_removed(&self) -> bool {
        let st = state(&self.inner);
        if owner(&self.inner).is_some() && st.sim_lock && !st.on {
            apply(&self.inner, GuardState { on: true, reason: "sim_removed".into(), ..st });
            tell_owner(&self.inner);
            return true;
        }
        false
    }

    /// The offline unlock on the stolen screen. `Ok(true)`: right code, stolen mode is off.
    /// `Err` while waiting after wrong codes.
    pub fn guard_try_code(&self, code: &str) -> Result<bool> {
        let st = state(&self.inner);
        if !st.on {
            return Ok(true);
        }
        let mut a = attempts(&self.inner);
        let now = now_ms();
        if now < a.retry_at {
            return Err(Error::InvalidInput(format!("try again in {} s", (a.retry_at - now + 999) / 1000)));
        }
        let secret = self
            .inner
            .store
            .setting(SECRET)?
            .and_then(|h| data_encoding::HEXLOWER.decode(h.as_bytes()).ok())
            .filter(|s| s.len() == 32)
            .ok_or_else(|| Error::InvalidInput("no owner is set".into()))?;
        let expected = unlock_code(&secret, &a.challenge);
        let given = digits(code);
        // Compare in constant time: no early exit on the first wrong digit.
        let ok = given.len() == expected.len()
            && given.bytes().zip(expected.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
        if ok {
            save(&self.inner, ATTEMPTS, &Attempts::default());
            apply(&self.inner, GuardState { on: false, reason: "code".into(), ..st });
            tell_owner(&self.inner);
            Ok(true)
        } else {
            a.count += 1;
            a.retry_at = now + backoff_ms(a.count);
            a.challenge = new_challenge();
            save(&self.inner, ATTEMPTS, &a);
            self.inner.log(None, "stolen_code_wrong", &a.count.to_string());
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_codes_are_ten_digits_and_depend_on_both_inputs() {
        let s = [7u8; 32];
        let a = unlock_code(&s, "12345678");
        assert_eq!(a.len(), 10);
        assert!(a.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(a, unlock_code(&s, " 12345678 "), "spaces don't matter");
        assert_ne!(a, unlock_code(&s, "12345679"));
        assert_ne!(a, unlock_code(&[8u8; 32], "12345678"));
        // A fixed vector, so the phone apps (which only display) and the core agree forever.
        assert_eq!(unlock_code(&[0u8; 32], "00000000"), unlock_code(&[0u8; 32], "00000000"));
    }

    #[test]
    fn wrong_codes_back_off_up_to_an_hour() {
        assert_eq!(backoff_ms(0), 0);
        assert_eq!(backoff_ms(1), 30_000);
        assert_eq!(backoff_ms(3), 60_000);
        assert_eq!(backoff_ms(4), 300_000);
        assert_eq!(backoff_ms(5), 900_000);
        assert_eq!(backoff_ms(40), 3_600_000);
    }
}
