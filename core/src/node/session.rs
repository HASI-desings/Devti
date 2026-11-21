//! Connections and sessions.
//!
//! * One QUIC endpoint per node both accepts and dials.
//! * Every connection is served by [`serve_connection`], which reads the
//!   [`StreamOpen`] header of each incoming stream and routes it (pairing or control).
//! * A **session** is an authenticated control stream with a paired device. There is at
//!   most one per peer; if both devices dial each other at once, both deterministically
//!   keep the connection initiated by the lower device ID.
//! * [`reconnect_loop`] redials offline peers with capped exponential backoff, using
//!   fresh mDNS addresses first and the last known address second.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{Connection, RecvStream, SendStream, VarInt};
use rustls::pki_types::CertificateDer;
use tokio::sync::mpsc;

use super::{Inner, NodeEvent, ceremony, trust::TrustView};
use crate::discovery::DiscoveryEvent;
use crate::error::{Error, Result};
use crate::identity::{Identity, fingerprint_of};
use crate::model::{DeviceId, DeviceInfo, DeviceStatus, Fingerprint};
use crate::net::now_ms;
use crate::protocol::{
    CONTROL_FRAME_LIMIT, ControlMsg, GoodbyeReason, PAIR_FRAME_LIMIT, PROTOCOL_VERSION, StreamOpen, close_code,
    read_message, write_message,
};
use crate::tls::{self, SERVER_NAME, ServerExpectation};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const DIAL_TIMEOUT: Duration = Duration::from_secs(5);
const FIRST_STREAM_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// An authenticated control channel with one peer.
pub(crate) struct Session {
    pub id: u64,
    pub conn: Connection,
    pub tx: mpsc::UnboundedSender<ControlMsg>,
    pub initiator: DeviceId,
    pub connected_at: i64,
}

fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut t = quinn::TransportConfig::default();
    t.keep_alive_interval(Some(Duration::from_secs(4)));
    t.max_idle_timeout(Some(
        quinn::IdleTimeout::try_from(Duration::from_secs(15)).expect("15s is a valid idle timeout"),
    ));
    Arc::new(t)
}

/// Binds the node's UDP endpoint, falling back to a random port if the default is taken.
pub(crate) fn bind_endpoint(
    identity: &Identity,
    trust: Arc<TrustView>,
    ip: IpAddr,
    port: u16,
) -> Result<quinn::Endpoint> {
    let tls = tls::server_config(identity, trust)?;
    let crypto = QuicServerConfig::try_from(tls).map_err(|e| Error::Tls(e.to_string()))?;
    let mut server = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    server.transport_config(transport_config());
    match quinn::Endpoint::server(server.clone(), SocketAddr::new(ip, port)) {
        Ok(e) => Ok(e),
        Err(e) if port != 0 && e.kind() == std::io::ErrorKind::AddrInUse => {
            tracing::warn!(port, "default port in use, binding a random port");
            Ok(quinn::Endpoint::server(server, SocketAddr::new(ip, 0))?)
        }
        Err(e) => Err(e.into()),
    }
}

fn client_config(identity: &Identity, expect: ServerExpectation) -> Result<quinn::ClientConfig> {
    let tls = tls::client_config(identity, expect)?;
    let crypto = QuicClientConfig::try_from(tls).map_err(|e| Error::Tls(e.to_string()))?;
    let mut cfg = quinn::ClientConfig::new(Arc::new(crypto));
    cfg.transport_config(transport_config());
    Ok(cfg)
}

/// Fingerprint of the certificate the peer authenticated with.
pub(crate) fn peer_fingerprint(conn: &Connection) -> Result<Fingerprint> {
    let certs = conn
        .peer_identity()
        .and_then(|id| id.downcast::<Vec<CertificateDer<'static>>>().ok())
        .ok_or_else(|| Error::Tls("peer presented no certificate".into()))?;
    let first = certs.first().ok_or_else(|| Error::Tls("peer presented no certificate".into()))?;
    fingerprint_of(first)
}

/// Dials the first reachable address.
pub(crate) async fn connect_any(
    inner: &Arc<Inner>,
    addrs: &[SocketAddr],
    expect: ServerExpectation,
) -> Result<Connection> {
    let cfg = client_config(&inner.identity, expect)?;
    let mut last_err = Error::Connection("no address to dial".into());
    for addr in addrs {
        if addr.is_ipv4() != inner.local_addr.is_ipv4() {
            continue; // the endpoint is bound to one address family
        }
        let attempt = async {
            let connecting = inner.endpoint.connect_with(cfg.clone(), *addr, SERVER_NAME)?;
            Ok::<_, Error>(connecting.await?)
        };
        match tokio::time::timeout(DIAL_TIMEOUT, attempt).await {
            Ok(Ok(conn)) => return Ok(conn),
            Ok(Err(e)) => last_err = e,
            Err(_) => last_err = Error::Timeout(format!("no answer from {addr}")),
        }
    }
    Err(last_err)
}

// ------------------------------------------------------------------ inbound

pub(crate) async fn accept_loop(inner: Arc<Inner>) {
    let mut shutdown = inner.shutdown.subscribe();
    loop {
        let incoming = tokio::select! {
            i = inner.endpoint.accept() => i,
            _ = shutdown.changed() => break,
        };
        let Some(incoming) = incoming else { break };
        if inner.paused.load(Ordering::SeqCst) {
            incoming.refuse();
            continue;
        }
        let rt = inner.rt.clone();
        let inner = inner.clone();
        rt.spawn(async move {
            let conn = match incoming.accept() {
                Ok(connecting) => match connecting.await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::debug!(error = %e, "inbound handshake failed");
                        return;
                    }
                },
                Err(e) => {
                    tracing::debug!(error = %e, "inbound connection rejected");
                    return;
                }
            };
            match peer_fingerprint(&conn) {
                Ok(fp) => serve_connection(inner, conn, fp).await,
                Err(_) => conn.close(VarInt::from_u32(close_code::UNTRUSTED), b"no certificate"),
            }
        });
    }
}

/// Routes every bi-directional stream the peer opens on `conn`.
pub(crate) async fn serve_connection(inner: Arc<Inner>, conn: Connection, peer_fp: Fingerprint) {
    let peer_id = peer_fp.device_id();
    let mut shutdown = inner.shutdown.subscribe();
    let mut first = true;
    loop {
        let wait_first = first && !inner.trust.is_trusted(&peer_fp);
        let accept = async {
            if wait_first {
                tokio::time::timeout(FIRST_STREAM_TIMEOUT, conn.accept_bi()).await.ok()
            } else {
                Some(conn.accept_bi().await)
            }
        };
        let accepted = tokio::select! {
            a = accept => a,
            _ = shutdown.changed() => return,
        };
        let (send, mut recv) = match accepted {
            Some(Ok(s)) => s,
            Some(Err(_)) => return, // connection closed
            None => {
                conn.close(VarInt::from_u32(close_code::UNTRUSTED), b"idle stranger");
                return;
            }
        };
        first = false;
        let open =
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, read_message::<_, StreamOpen>(&mut recv, PAIR_FRAME_LIMIT))
                .await
            {
                Ok(Ok(Some(o))) => o,
                _ => {
                    conn.close(VarInt::from_u32(close_code::PROTOCOL), b"bad stream header");
                    return;
                }
            };
        match open {
            StreamOpen::Pair => {
                inner.rt.spawn(ceremony::run_hub(inner.clone(), conn.clone(), send, recv, peer_fp));
            }
            StreamOpen::Control => {
                if !inner.trust.is_trusted(&peer_fp) {
                    conn.close(VarInt::from_u32(close_code::UNTRUSTED), b"not paired");
                    return;
                }
                inner.rt.spawn(accept_control(inner.clone(), conn.clone(), send, recv, peer_id));
            }
            StreamOpen::Unknown => {
                let mut send = send;
                let _ = send.reset(VarInt::from_u32(0));
                let _ = recv.stop(VarInt::from_u32(0));
            }
        }
    }
}

async fn accept_control(
    inner: Arc<Inner>,
    conn: Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    peer_id: DeviceId,
) {
    match exchange_hello(&inner, &mut send, &mut recv, peer_id, false).await {
        Ok((info, status)) => {
            if let Err(e) = register_session(&inner, peer_id, conn.clone(), peer_id, send, recv, info, status) {
                tracing::debug!(error = %e, "session rejected");
                conn.close(VarInt::from_u32(close_code::UNTRUSTED), b"not paired");
            }
        }
        Err(e) => {
            tracing::debug!(error = %e, "hello failed");
            conn.close(VarInt::from_u32(close_code::PROTOCOL), b"hello failed");
        }
    }
}

// ----------------------------------------------------------------- outbound

/// Opens the control stream on a freshly dialled (or freshly paired) connection.
pub(crate) async fn establish_outgoing(inner: &Arc<Inner>, conn: Connection, peer_id: DeviceId) -> Result<()> {
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(&mut send, &StreamOpen::Control).await?;
    let (info, status) = exchange_hello(inner, &mut send, &mut recv, peer_id, true).await?;
    let fp = peer_fingerprint(&conn)?;
    register_session(inner, peer_id, conn.clone(), inner.identity.device_id(), send, recv, info, status)?;
    inner.rt.spawn(serve_connection(inner.clone(), conn, fp));
    Ok(())
}

async fn exchange_hello(
    inner: &Arc<Inner>,
    send: &mut SendStream,
    recv: &mut RecvStream,
    peer_id: DeviceId,
    speak_first: bool,
) -> Result<(DeviceInfo, DeviceStatus)> {
    let hello = ControlMsg::Hello { proto: PROTOCOL_VERSION, info: inner.info(), status: inner.status.read().clone() };
    if speak_first {
        write_message(send, &hello).await?;
    }
    let reply = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_message::<_, ControlMsg>(recv, CONTROL_FRAME_LIMIT))
        .await
        .map_err(|_| Error::Timeout("hello".into()))??;
    let (info, status) = match reply {
        Some(ControlMsg::Hello { proto, info, status }) => {
            if proto != PROTOCOL_VERSION {
                return Err(Error::Protocol(format!("peer speaks protocol v{proto}, we speak v{PROTOCOL_VERSION}")));
            }
            if info.id != peer_id {
                return Err(Error::Protocol("hello identity does not match certificate".into()));
            }
            (info.sanitized(), status.sanitized())
        }
        _ => return Err(Error::Protocol("expected hello".into())),
    };
    if !speak_first {
        write_message(send, &hello).await?;
    }
    Ok((info, status))
}

#[allow(clippy::too_many_arguments)]
fn register_session(
    inner: &Arc<Inner>,
    peer_id: DeviceId,
    conn: Connection,
    initiator: DeviceId,
    send: SendStream,
    recv: RecvStream,
    info: DeviceInfo,
    status: DeviceStatus,
) -> Result<()> {
    let preferred = inner.identity.device_id().min(peer_id);
    let session_id = inner.session_seq.fetch_add(1, Ordering::SeqCst);
    let now = now_ms();
    let addr = conn.remote_address();
    let (tx, rx) = mpsc::unbounded_channel();

    let (snap, replaced) = {
        let mut peers = inner.peers.write();
        let p = peers.get_mut(&peer_id).ok_or_else(|| Error::UnknownPeer(peer_id.to_string()))?;
        let mut replaced = None;
        if let Some(existing) = &p.session
            && existing.conn.close_reason().is_none()
        {
            if existing.initiator == preferred && initiator != preferred {
                drop(peers);
                conn.close(VarInt::from_u32(close_code::DUPLICATE), b"duplicate");
                return Ok(());
            }
            replaced = Some(existing.conn.clone());
        }
        p.record.info = info;
        p.record.last_status = status;
        p.record.last_seen = Some(now);
        p.record.last_addrs = vec![addr];
        p.attempts = 0;
        p.session = Some(Session { id: session_id, conn: conn.clone(), tx, initiator, connected_at: now });
        (inner.snapshot(p), replaced)
    };
    if let Some(old) = replaced {
        old.close(VarInt::from_u32(close_code::DUPLICATE), b"duplicate");
    }
    let _ = inner.store.update_peer_info(&snap.info);
    let _ = inner.store.update_peer_status(&peer_id, &snap.status);
    let _ = inner.store.update_peer_seen(&peer_id, now, &[addr]);

    inner.rt.spawn(write_loop(send, rx));
    inner.rt.spawn(read_loop(inner.clone(), peer_id, recv));
    let watcher_inner = inner.clone();
    inner.rt.spawn(async move {
        let reason = conn.closed().await;
        on_session_closed(&watcher_inner, peer_id, session_id, reason);
    });

    inner.log(Some((&peer_id, &snap.info.name)), "connected", &addr.to_string());
    inner.emit(NodeEvent::PeerChanged { peer: snap });
    Ok(())
}

async fn write_loop(mut send: SendStream, mut rx: mpsc::UnboundedReceiver<ControlMsg>) {
    while let Some(msg) = rx.recv().await {
        if write_message(&mut send, &msg).await.is_err() {
            break;
        }
    }
    let _ = send.finish();
}

async fn read_loop(inner: Arc<Inner>, peer_id: DeviceId, mut recv: RecvStream) {
    loop {
        match read_message::<_, ControlMsg>(&mut recv, CONTROL_FRAME_LIMIT).await {
            Ok(Some(msg)) => handle_control(&inner, peer_id, msg),
            Ok(None) => break,
            Err(e) => {
                tracing::debug!(error = %e, peer = %peer_id, "control stream ended");
                break;
            }
        }
    }
}

fn handle_control(inner: &Arc<Inner>, peer_id: DeviceId, msg: ControlMsg) {
    match msg {
        ControlMsg::Status { status } => {
            let status = status.sanitized();
            let snap = {
                let mut peers = inner.peers.write();
                let Some(p) = peers.get_mut(&peer_id) else { return };
                p.record.last_status = status.clone();
                inner.snapshot(p)
            };
            let _ = inner.store.update_peer_status(&peer_id, &status);
            inner.emit(NodeEvent::PeerChanged { peer: snap });
        }
        ControlMsg::Info { info } => {
            if info.id != peer_id {
                return;
            }
            let info = info.sanitized();
            let snap = {
                let mut peers = inner.peers.write();
                let Some(p) = peers.get_mut(&peer_id) else { return };
                p.record.info = info.clone();
                inner.snapshot(p)
            };
            let _ = inner.store.update_peer_info(&info);
            inner.emit(NodeEvent::PeerChanged { peer: snap });
        }
        ControlMsg::Ping { nonce } => {
            if let Some(s) = inner.peers.read().get(&peer_id).and_then(|p| p.session.as_ref()) {
                let _ = s.tx.send(ControlMsg::Pong { nonce });
            }
        }
        ControlMsg::Goodbye { reason: GoodbyeReason::Revoked } => {
            let _ = inner.remove_peer(&peer_id, super::RemovalReason::RevokedByPeer);
        }
        ControlMsg::Goodbye { .. } | ControlMsg::Pong { .. } | ControlMsg::Hello { .. } => {}
        ControlMsg::Unknown => tracing::trace!(peer = %peer_id, "ignoring unknown control message"),
    }
}

fn on_session_closed(inner: &Arc<Inner>, peer_id: DeviceId, session_id: u64, reason: quinn::ConnectionError) {
    let revoked_by_peer = matches!(
        &reason,
        quinn::ConnectionError::ApplicationClosed(c) if c.error_code == VarInt::from_u32(close_code::REVOKED)
    );
    let now = now_ms();
    let (snap, addrs) = {
        let mut peers = inner.peers.write();
        let Some(p) = peers.get_mut(&peer_id) else { return };
        if p.session.as_ref().map(|s| s.id) != Some(session_id) {
            return; // an older, replaced session
        }
        p.session = None;
        p.record.last_seen = Some(now);
        p.attempts = 0;
        p.next_attempt = Instant::now() + Duration::from_millis(500);
        (inner.snapshot(p), p.record.last_addrs.clone())
    };
    let _ = inner.store.update_peer_seen(&peer_id, now, &addrs);
    if revoked_by_peer {
        let _ = inner.remove_peer(&peer_id, super::RemovalReason::RevokedByPeer);
        return;
    }
    inner.log(Some((&peer_id, &snap.info.name)), "disconnected", &reason.to_string());
    inner.emit(NodeEvent::PeerChanged { peer: snap });
}

// ---------------------------------------------------------------- reconnect

/// Capped exponential backoff with ±20 % jitter: 1, 2, 4, 8, 16, 30, 30… seconds.
pub(crate) fn backoff(attempts: u32) -> Duration {
    let base = Duration::from_secs(1).saturating_mul(1u32 << attempts.min(5)).min(MAX_BACKOFF);
    let jitter = getrandom::u32().unwrap_or(0) % 400; // 0..400 per-mille
    base.mul_f64(0.8 + jitter as f64 / 1000.0)
}

pub(crate) async fn reconnect_loop(inner: Arc<Inner>) {
    let mut shutdown = inner.shutdown.subscribe();
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = shutdown.changed() => break,
        }
        if inner.paused.load(Ordering::SeqCst) {
            continue;
        }
        let now = Instant::now();
        let due: Vec<(DeviceId, Fingerprint, Vec<SocketAddr>)> = {
            // Lock order: discovered → peers.
            let discovered = inner.discovered.read();
            let mut peers = inner.peers.write();
            peers
                .iter_mut()
                .filter(|(_, p)| p.session.is_none() && !p.dialing && p.next_attempt <= now)
                .filter_map(|(id, p)| {
                    let mut addrs = discovered.get(id).map(|d| d.addrs.clone()).unwrap_or_default();
                    for a in &p.record.last_addrs {
                        if !addrs.contains(a) {
                            addrs.push(*a);
                        }
                    }
                    if addrs.is_empty() {
                        p.next_attempt = now + Duration::from_secs(5);
                        return None;
                    }
                    p.dialing = true;
                    Some((*id, p.record.fingerprint, addrs))
                })
                .collect()
        };
        for (id, fp, addrs) in due {
            inner.rt.spawn(dial_peer(inner.clone(), id, fp, addrs));
        }
    }
}

async fn dial_peer(inner: Arc<Inner>, id: DeviceId, fp: Fingerprint, addrs: Vec<SocketAddr>) {
    let result = async {
        let conn = connect_any(&inner, &addrs, ServerExpectation::Pinned(fp)).await?;
        if let Err(e) = establish_outgoing(&inner, conn.clone(), id).await {
            conn.close(VarInt::from_u32(close_code::PROTOCOL), b"session setup failed");
            return Err(e);
        }
        Ok::<_, Error>(())
    }
    .await;
    let mut peers = inner.peers.write();
    if let Some(p) = peers.get_mut(&id) {
        p.dialing = false;
        if let Err(e) = result {
            p.attempts = p.attempts.saturating_add(1);
            p.next_attempt = Instant::now() + backoff(p.attempts);
            tracing::debug!(peer = %id, error = %e, attempts = p.attempts, "reconnect failed");
        }
    }
}

pub(crate) async fn discovery_loop(inner: Arc<Inner>, mut rx: mpsc::UnboundedReceiver<DiscoveryEvent>) {
    let mut shutdown = inner.shutdown.subscribe();
    loop {
        let ev = tokio::select! {
            e = rx.recv() => e,
            _ = shutdown.changed() => break,
        };
        let Some(ev) = ev else { break };
        match ev {
            DiscoveryEvent::Found(dev) => {
                inner.discovered.write().insert(dev.id, dev.clone());
                if let Some(p) = inner.peers.write().get_mut(&dev.id)
                    && p.session.is_none()
                {
                    p.attempts = 0;
                    p.next_attempt = Instant::now();
                }
                inner.emit(NodeEvent::DeviceDiscovered { device: dev });
            }
            DiscoveryEvent::Lost(id) => {
                if inner.discovered.write().remove(&id).is_some() {
                    inner.emit(NodeEvent::DeviceLost { id });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let b0 = backoff(0);
        assert!(b0 >= Duration::from_millis(800) && b0 <= Duration::from_millis(1200));
        let b3 = backoff(3);
        assert!(b3 >= Duration::from_millis(6400) && b3 <= Duration::from_millis(9600));
        for n in [5, 10, 100, u32::MAX] {
            assert!(backoff(n) <= Duration::from_secs(36));
            assert!(backoff(n) >= Duration::from_secs(24));
        }
    }
}
