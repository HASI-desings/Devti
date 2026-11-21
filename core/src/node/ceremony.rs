//! The pairing ceremony over the network.
//!
//! ```text
//! Joiner                                         Hub (window open)
//!   |-- QUIC + TLS 1.3 (pins hub fp from QR) -------->|  (admits stranger: window open)
//!   |-- StreamOpen::Pair ---------------------------->|
//!   |-- Start{proto, method, spakeB, info} ---------->|  checks info.id == cert id
//!   |<----------------------- Reply{spakeA, info} ----|
//!   |   both: K = SPAKE2(secret); bind to TLS exporter + both fingerprints
//!   |-- Confirm{macJ} ------------------------------->|  wrong secret → Abort(BadSecret)
//!   |<------------------------------ Confirm{macH} ---|
//!   |   both screens show the same 6-digit code; users confirm
//!   |-- Decision{accept} <--------------> Decision{accept}
//!   |<------------------------------------- Done -----|  hub stored joiner as trusted
//!   |   joiner stores hub, then opens StreamOpen::Control on the same connection
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quinn::{Connection, RecvStream, SendStream, VarInt};
use serde_bytes::ByteBuf;
use tokio::sync::oneshot;

use super::session::{self, peer_fingerprint};
use super::{Inner, MAX_PAIRING_FAILURES, NodeEvent, PairingRole, PeerSnapshot, PeerState};
use crate::error::Result;
use crate::model::{DeviceInfo, Fingerprint, Permissions, TrustRole};
use crate::net::now_ms;
use crate::pairing::pake::{EXPORTER_LABEL, password_for_code, password_for_qr};
use crate::pairing::{Binding, PakeState, Side, format_code};
use crate::protocol::{
    PAIR_FRAME_LIMIT, PROTOCOL_VERSION, PairAbortReason, PairMethod, PairMsg, StreamOpen, close_code, read_message,
    write_message,
};
use crate::store::PeerRecord;
use crate::tls::ServerExpectation;

pub(crate) use crate::pairing::new_id;

const STEP_TIMEOUT: Duration = Duration::from_secs(30);
/// How long users have to compare the codes.
const DECISION_TIMEOUT: Duration = Duration::from_secs(120);

type Fail = (PairAbortReason, String);

fn fail(reason: PairAbortReason) -> Fail {
    (reason, reason.describe().to_string())
}

fn other(e: impl std::fmt::Display) -> Fail {
    (PairAbortReason::Other, e.to_string())
}

/// Where and how the joiner connects.
pub(crate) struct JoinTarget {
    pub addrs: Vec<SocketAddr>,
    pub expect: ServerExpectation,
    pub method: PairMethod,
    pub password: Vec<u8>,
}

// ------------------------------------------------------------------- joiner

pub(crate) async fn run_joiner(inner: Arc<Inner>, pairing_id: u64, target: JoinTarget) {
    inner.emit(NodeEvent::PairingStarted { pairing_id, role: PairingRole::Joiner, peer: None });
    let conn = match session::connect_any(&inner, &target.addrs, target.expect).await {
        Ok(c) => c,
        Err(e) => {
            let msg = format!(
                "Couldn't reach the other device ({e}). Make sure both devices are on the same network and Devti is open."
            );
            inner.emit(NodeEvent::PairingFailed { pairing_id, reason: PairAbortReason::Other, message: msg });
            return;
        }
    };
    match joiner_flow(&inner, pairing_id, &conn, &target).await {
        Ok(peer) => {
            inner.emit(NodeEvent::PairingCompleted { pairing_id, peer: peer.clone() });
            let id = peer.info.id;
            if let Err(e) = session::establish_outgoing(&inner, conn.clone(), id).await {
                tracing::debug!(error = %e, "post-pairing session failed; reconnect loop will retry");
                conn.close(VarInt::from_u32(close_code::PAIRING_FINISHED), b"paired");
            }
        }
        Err((reason, message)) => {
            inner.decisions.lock().remove(&pairing_id);
            inner.emit(NodeEvent::PairingFailed { pairing_id, reason, message });
            tokio::time::sleep(Duration::from_millis(100)).await;
            conn.close(VarInt::from_u32(close_code::PAIRING_FAILED), b"pairing failed");
        }
    }
}

async fn joiner_flow(
    inner: &Arc<Inner>,
    pairing_id: u64,
    conn: &Connection,
    target: &JoinTarget,
) -> Result<PeerSnapshot, Fail> {
    let hub_fp = peer_fingerprint(conn).map_err(other)?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(other)?;
    write_message(&mut send, &StreamOpen::Pair).await.map_err(other)?;

    let pake = PakeState::start(Side::Joiner, &target.password);
    let start = PairMsg::Start {
        proto: PROTOCOL_VERSION,
        method: target.method,
        spake: ByteBuf::from(pake.message().to_vec()),
        info: inner.info(),
    };
    write_message(&mut send, &start).await.map_err(other)?;

    let PairMsg::Reply { spake, info: hub_info } = read_pair(&mut recv).await? else {
        return Err(fail(PairAbortReason::Other));
    };
    if hub_info.id != hub_fp.device_id() {
        let _ = write_message(&mut send, &PairMsg::Abort { reason: PairAbortReason::IdentityMismatch }).await;
        return Err(fail(PairAbortReason::IdentityMismatch));
    }
    let hub_info = hub_info.sanitized();
    let binding = Binding { exporter: exporter(conn)?, hub_fp, joiner_fp: inner.identity.fingerprint() };
    let keys = pake.finish(&spake, &binding).map_err(|_| fail(PairAbortReason::BadSecret))?;

    write_message(&mut send, &PairMsg::Confirm { mac: ByteBuf::from(keys.my_mac()) }).await.map_err(other)?;
    let PairMsg::Confirm { mac } = read_pair(&mut recv).await? else {
        return Err(fail(PairAbortReason::Other));
    };
    if !keys.verify_peer_mac(&mac) {
        let _ = write_message(&mut send, &PairMsg::Abort { reason: PairAbortReason::BadSecret }).await;
        return Err(fail(PairAbortReason::BadSecret));
    }

    let (tx, rx) = oneshot::channel();
    inner.decisions.lock().insert(pairing_id, tx);
    inner.emit(NodeEvent::PairingVerify {
        pairing_id,
        role: PairingRole::Joiner,
        peer: hub_info.clone(),
        sas: format_code(keys.sas()),
    });
    exchange_decisions(&mut send, &mut recv, rx).await?;

    match read_pair(&mut recv).await? {
        PairMsg::Done => {}
        _ => return Err(fail(PairAbortReason::Other)),
    }
    let snap = inner.add_trusted_peer(hub_info, hub_fp, conn.remote_address()).map_err(other)?;
    let _ = send.finish();
    Ok(snap)
}

// ---------------------------------------------------------------------- hub

pub(crate) async fn run_hub(
    inner: Arc<Inner>,
    conn: Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    joiner_fp: Fingerprint,
) {
    let reserved = {
        let mut w = inner.window.lock();
        match w.as_mut() {
            Some(w) if w.busy => Err(PairAbortReason::Busy),
            Some(w) if w.expires > Instant::now() => {
                w.busy = true;
                Ok((w.id, w.qr_secret, w.code.clone()))
            }
            _ => Err(PairAbortReason::NotAccepting),
        }
    };
    let (window_id, qr_secret, code) = match reserved {
        Ok(r) => r,
        Err(reason) => {
            let _ = write_message(&mut send, &PairMsg::Abort { reason }).await;
            let _ = send.finish();
            close_if_stranger(&inner, &conn, &joiner_fp).await;
            return;
        }
    };

    let pairing_id = new_id().unwrap_or_default();
    let secrets = HubSecrets { qr: password_for_qr(&qr_secret), code: password_for_code(&code) };
    let mut started = false;
    let result = hub_flow(&inner, pairing_id, &conn, &mut send, &mut recv, joiner_fp, &secrets, &mut started).await;

    match result {
        Ok(peer) => {
            inner.close_pairing_window("paired");
            inner.emit(NodeEvent::PairingCompleted { pairing_id, peer });
        }
        Err((reason, message)) => {
            inner.decisions.lock().remove(&pairing_id);
            let close_window = {
                let mut w = inner.window.lock();
                match w.as_mut() {
                    Some(win) if win.id == window_id => {
                        win.busy = false;
                        if reason == PairAbortReason::BadSecret {
                            win.failures += 1;
                        }
                        win.failures >= MAX_PAIRING_FAILURES || win.expires <= Instant::now()
                    }
                    _ => false,
                }
            };
            if close_window {
                inner.close_pairing_window("too many attempts or expired");
            }
            if started {
                inner.emit(NodeEvent::PairingFailed { pairing_id, reason, message });
            }
            close_if_stranger(&inner, &conn, &joiner_fp).await;
        }
    }
}

struct HubSecrets {
    qr: Vec<u8>,
    code: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
async fn hub_flow(
    inner: &Arc<Inner>,
    pairing_id: u64,
    conn: &Connection,
    send: &mut SendStream,
    recv: &mut RecvStream,
    joiner_fp: Fingerprint,
    secrets: &HubSecrets,
    started: &mut bool,
) -> Result<PeerSnapshot, Fail> {
    let PairMsg::Start { proto, method, spake, info } = read_pair(recv).await? else {
        return Err(fail(PairAbortReason::Other));
    };
    if proto != PROTOCOL_VERSION {
        let _ = write_message(send, &PairMsg::Abort { reason: PairAbortReason::Incompatible }).await;
        return Err(fail(PairAbortReason::Incompatible));
    }
    if info.id != joiner_fp.device_id() {
        let _ = write_message(send, &PairMsg::Abort { reason: PairAbortReason::IdentityMismatch }).await;
        return Err(fail(PairAbortReason::IdentityMismatch));
    }
    let info = info.sanitized();
    *started = true;
    inner.emit(NodeEvent::PairingStarted { pairing_id, role: PairingRole::Hub, peer: Some(info.clone()) });

    let password = match method {
        PairMethod::Qr => &secrets.qr,
        PairMethod::Code => &secrets.code,
    };
    let pake = PakeState::start(Side::Hub, password);
    let reply = PairMsg::Reply { spake: ByteBuf::from(pake.message().to_vec()), info: inner.info() };
    write_message(send, &reply).await.map_err(other)?;

    let binding = Binding { exporter: exporter(conn)?, hub_fp: inner.identity.fingerprint(), joiner_fp };
    let keys = match pake.finish(&spake, &binding) {
        Ok(k) => k,
        Err(_) => {
            let _ = write_message(send, &PairMsg::Abort { reason: PairAbortReason::BadSecret }).await;
            return Err(fail(PairAbortReason::BadSecret));
        }
    };

    let PairMsg::Confirm { mac } = read_pair(recv).await? else {
        return Err(fail(PairAbortReason::Other));
    };
    if !keys.verify_peer_mac(&mac) {
        let _ = write_message(send, &PairMsg::Abort { reason: PairAbortReason::BadSecret }).await;
        return Err(fail(PairAbortReason::BadSecret));
    }
    write_message(send, &PairMsg::Confirm { mac: ByteBuf::from(keys.my_mac()) }).await.map_err(other)?;

    let (tx, rx) = oneshot::channel();
    inner.decisions.lock().insert(pairing_id, tx);
    inner.emit(NodeEvent::PairingVerify {
        pairing_id,
        role: PairingRole::Hub,
        peer: info.clone(),
        sas: format_code(keys.sas()),
    });
    exchange_decisions(send, recv, rx).await?;

    let snap = inner.add_trusted_peer(info, joiner_fp, conn.remote_address()).map_err(other)?;
    write_message(send, &PairMsg::Done).await.map_err(other)?;
    let _ = send.finish();
    Ok(snap)
}

// ------------------------------------------------------------------ shared

async fn read_pair(recv: &mut RecvStream) -> Result<PairMsg, Fail> {
    match tokio::time::timeout(STEP_TIMEOUT, read_message::<_, PairMsg>(recv, PAIR_FRAME_LIMIT)).await {
        Err(_) => Err(fail(PairAbortReason::Timeout)),
        Ok(Err(e)) => Err(other(e)),
        Ok(Ok(None)) => Err((PairAbortReason::Other, "The other device closed the connection.".into())),
        Ok(Ok(Some(PairMsg::Abort { reason }))) => Err(fail(reason)),
        Ok(Ok(Some(m))) => Ok(m),
    }
}

/// Waits until both the local user and the remote device accepted.
async fn exchange_decisions(
    send: &mut SendStream,
    recv: &mut RecvStream,
    mut local: oneshot::Receiver<bool>,
) -> Result<(), Fail> {
    let remote = read_message::<_, PairMsg>(recv, PAIR_FRAME_LIMIT);
    tokio::pin!(remote);
    let deadline = tokio::time::sleep(DECISION_TIMEOUT);
    tokio::pin!(deadline);
    let (mut local_ok, mut remote_ok) = (false, false);
    loop {
        tokio::select! {
            d = &mut local, if !local_ok => {
                let accept = d.unwrap_or(false);
                write_message(send, &PairMsg::Decision { accept }).await.map_err(other)?;
                if !accept {
                    return Err(fail(PairAbortReason::Rejected));
                }
                local_ok = true;
            }
            m = &mut remote, if !remote_ok => match m {
                Ok(Some(PairMsg::Decision { accept: true })) => remote_ok = true,
                Ok(Some(PairMsg::Decision { accept: false })) => return Err(fail(PairAbortReason::Rejected)),
                Ok(Some(PairMsg::Abort { reason })) => return Err(fail(reason)),
                Ok(Some(_)) | Ok(None) => return Err((PairAbortReason::Other, "The other device stopped pairing.".into())),
                Err(e) => return Err(other(e)),
            },
            _ = &mut deadline => {
                let _ = write_message(send, &PairMsg::Abort { reason: PairAbortReason::Timeout }).await;
                return Err(fail(PairAbortReason::Timeout));
            }
        }
        if local_ok && remote_ok {
            return Ok(());
        }
    }
}

fn exporter(conn: &Connection) -> Result<[u8; 32], Fail> {
    let mut out = [0u8; 32];
    conn.export_keying_material(&mut out, EXPORTER_LABEL, b"devti").map_err(|_| other("TLS exporter unavailable"))?;
    Ok(out)
}

async fn close_if_stranger(inner: &Arc<Inner>, conn: &Connection, fp: &Fingerprint) {
    if !inner.trust.is_trusted(fp) {
        tokio::time::sleep(Duration::from_millis(150)).await;
        conn.close(VarInt::from_u32(close_code::PAIRING_FAILED), b"pairing failed");
    }
}

impl Inner {
    /// Persists a newly paired device and starts treating it as trusted.
    pub(crate) fn add_trusted_peer(&self, info: DeviceInfo, fp: Fingerprint, addr: SocketAddr) -> Result<PeerSnapshot> {
        let now = now_ms();
        let record = PeerRecord {
            info: info.clone(),
            fingerprint: fp,
            role: TrustRole::Trusted,
            permissions: Permissions::for_role(TrustRole::Trusted),
            paired_at: now,
            last_seen: Some(now),
            last_addrs: vec![addr],
            last_status: Default::default(),
        };
        self.store.upsert_peer(&record)?;
        self.trust.insert(fp);
        let snap = {
            let mut peers = self.peers.write();
            let entry = peers.entry(info.id).or_insert_with(|| PeerState {
                record: record.clone(),
                session: None,
                // Give the joiner a moment to open the control stream on the pairing
                // connection before the reconnect loop dials independently.
                next_attempt: Instant::now() + Duration::from_secs(3),
                attempts: 0,
                dialing: false,
            });
            entry.record = record;
            self.snapshot(entry)
        };
        self.log(Some((&info.id, &info.name)), "paired", "");
        self.emit(NodeEvent::PeerChanged { peer: snap.clone() });
        Ok(snap)
    }
}
