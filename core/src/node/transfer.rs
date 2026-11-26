//! File transfer on dedicated QUIC streams (one stream per file).
//!
//! ```text
//! Sender                                    Receiver
//!   |-- StreamOpen::File ------------------->|  needs the `files` permission
//!   |-- Offer{id, name, size, mime} -------->|  keeps `.devti-<id>.part` across retries
//!   |<---------------- Answer{accept, offset}|  offset = bytes already received
//!   |-- raw bytes [offset..size) ----------->|
//!   |-- Done{sha256 of whole file} --------->|  verifies, renames to a unique name
//!   |<------------------------------- Saved -|
//! ```
//!
//! If the connection drops, the sender marks the transfer *paused* and resumes it
//! automatically when the device reconnects; the receiver keeps the partial file, so
//! only the missing bytes are sent. Cancelling resets the stream with [`CANCELLED`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::link::{Link, LinkReadError, LinkRecv as RecvStream, LinkSend as SendStream};
use parking_lot::{Mutex, RwLock};
use ring::digest::{Context, SHA256};
use serde_bytes::ByteBuf;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use super::{Inner, NodeEvent};
use crate::error::{Error, Result};
use crate::model::DeviceId;
use crate::net::now_ms;
use crate::protocol::{FileMsg, PAIR_FRAME_LIMIT, StreamOpen, read_message, write_message};
use crate::store::TransferRecord;

pub(crate) const SETTING_DOWNLOADS: &str = "downloads_dir";
/// Stream reset code meaning "the sender cancelled on purpose".
pub const CANCELLED: u32 = 1;
const CHUNK: usize = 256 * 1024;
const PROGRESS_EVERY: Duration = Duration::from_millis(200);
/// Interrupted attempts in a row (without progress) before giving up.
const MAX_RETRIES: u32 = 5;

pub(crate) struct State {
    downloads: RwLock<PathBuf>,
    /// Running transfers → cancel flag.
    running: Mutex<HashMap<u64, Arc<std::sync::atomic::AtomicBool>>>,
    /// Consecutive interrupted attempts per outgoing transfer.
    retries: Mutex<HashMap<u64, u32>>,
}

impl State {
    pub fn new(downloads: PathBuf) -> Self {
        Self {
            downloads: RwLock::new(downloads),
            running: Mutex::new(HashMap::new()),
            retries: Mutex::new(HashMap::new()),
        }
    }
}

/// Makes a received file name safe: no directories, no reserved or hidden names.
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .chars()
        .take(200)
        .collect();
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4);
    if cleaned.is_empty() || reserved { format!("file-{}", now_ms()) } else { cleaned }
}

/// `dir/name`, or `dir/name (2).ext` … if it already exists.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..10_000).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(candidate)
}

fn emit(inner: &Inner, rec: &TransferRecord, rate_bps: u64) {
    inner.emit(NodeEvent::Transfer { transfer: rec.clone(), rate_bps });
}

fn finish(inner: &Inner, rec: &mut TransferRecord, state: &str) {
    rec.state = state.into();
    let _ = inner.store.upsert_transfer(rec);
    inner.transfers.running.lock().remove(&rec.id);
    emit(inner, rec, 0);
}

// -------------------------------------------------------------------- sending

/// Starts (or resumes) sending a queued/paused transfer if the peer is online.
fn start_send(inner: &Arc<Inner>, mut rec: TransferRecord) {
    if inner.transfers.running.lock().contains_key(&rec.id) {
        return;
    }
    let conn = {
        let peers = inner.peers.read();
        peers.get(&rec.device_id).and_then(|p| p.session.as_ref().map(|s| s.conn.clone()))
    };
    let Some(conn) = conn else {
        rec.state = "queued".into();
        let _ = inner.store.upsert_transfer(&rec);
        emit(inner, &rec, 0);
        return;
    };
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    inner.transfers.running.lock().insert(rec.id, cancel.clone());
    let inner2 = inner.clone();
    inner.rt.spawn(async move {
        let outcome = send_on(&inner2, &conn, &mut rec, &cancel).await;
        match outcome {
            Ok(()) => finish(&inner2, &mut rec, "done"),
            Err(_) if cancel.load(std::sync::atomic::Ordering::SeqCst) => finish(&inner2, &mut rec, "cancelled"),
            Err(Error::Protocol(reason)) => {
                tracing::info!(id = rec.id, %reason, "transfer declined");
                finish(&inner2, &mut rec, "failed");
            }
            Err(e) => {
                tracing::debug!(id = rec.id, error = %e, "transfer interrupted; will resume");
                finish(&inner2, &mut rec, "paused");
                schedule_retry(&inner2, rec);
            }
        }
    });
}

/// After an interruption, try again shortly. The device may already be back (in which
/// case its reconnect happened while this attempt was still unwinding, and the resume
/// trigger would otherwise be lost). Gives up after [`MAX_RETRIES`] tries without progress.
fn schedule_retry(inner: &Arc<Inner>, rec: TransferRecord) {
    let attempts = {
        let mut r = inner.transfers.retries.lock();
        let n = r.entry(rec.id).or_insert(0);
        *n += 1;
        *n
    };
    if attempts > MAX_RETRIES {
        inner.transfers.retries.lock().remove(&rec.id);
        let mut rec = rec;
        finish(inner, &mut rec, "failed");
        return;
    }
    let inner2 = inner.clone();
    inner.rt.spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let still_paused = inner2.store.transfer(rec.id).ok().flatten().is_some_and(|t| t.state == "paused");
        let online = inner2.peers.read().get(&rec.device_id).is_some_and(|p| p.session.is_some());
        if still_paused && online {
            start_send(&inner2, rec);
        }
    });
}

async fn send_on(
    inner: &Arc<Inner>,
    conn: &Link,
    rec: &mut TransferRecord,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    let mut file = tokio::fs::File::open(&rec.path).await?;
    let size = file.metadata().await?.len();
    if size != rec.size {
        return Err(Error::Protocol("the file changed since it was queued".into()));
    }
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(&mut send, &StreamOpen::File).await?;
    let mime = guess_mime(&rec.name).map(str::to_string);
    write_message(&mut send, &FileMsg::Offer { id: rec.id, name: rec.name.clone(), size, mime }).await?;
    let offset = match read_message::<_, FileMsg>(&mut recv, PAIR_FRAME_LIMIT).await? {
        Some(FileMsg::Answer { accept: true, offset, .. }) if offset <= size => offset,
        Some(FileMsg::Answer { reason, .. }) => {
            return Err(Error::Protocol(reason.unwrap_or_else(|| "declined".into())));
        }
        _ => return Err(Error::Protocol("unexpected answer".into())),
    };

    rec.state = "active".into();
    rec.done = offset;
    let _ = inner.store.upsert_transfer(rec);
    emit(inner, rec, 0);
    inner.transfers.retries.lock().remove(&rec.id);

    let mut hasher = Context::new(&SHA256);
    let mut buf = vec![0u8; CHUNK];
    let mut pos = 0u64;
    let (mut last, mut last_done) = (Instant::now(), offset);
    loop {
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = send.reset(CANCELLED);
            return Err(Error::Connection("cancelled".into()));
        }
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        hasher.update(chunk);
        let end = pos + n as u64;
        if end > offset {
            let skip = offset.saturating_sub(pos) as usize;
            send.write_all(&chunk[skip..]).await?;
            rec.done = end;
            if last.elapsed() >= PROGRESS_EVERY {
                let rate = ((rec.done - last_done) as f64 / last.elapsed().as_secs_f64()) as u64;
                emit(inner, rec, rate);
                let _ = inner.store.upsert_transfer(rec);
                (last, last_done) = (Instant::now(), rec.done);
            }
        }
        pos = end;
    }
    let digest = hasher.finish();
    write_message(&mut send, &FileMsg::Done { sha256: ByteBuf::from(digest.as_ref().to_vec()) }).await?;
    match read_message::<_, FileMsg>(&mut recv, PAIR_FRAME_LIMIT).await? {
        Some(FileMsg::Saved) => {
            let _ = send.finish();
            Ok(())
        }
        _ => Err(Error::Protocol("the other device couldn't verify the file".into())),
    }
}

/// Resumes queued or paused outgoing transfers when `peer` comes online.
pub(crate) fn resume_for(inner: &Arc<Inner>, peer: DeviceId) {
    let pending: Vec<TransferRecord> = inner
        .store
        .transfers(500)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.incoming && t.device_id == peer && (t.state == "queued" || t.state == "paused"))
        .collect();
    for t in pending {
        start_send(inner, t);
    }
}

// ------------------------------------------------------------------ receiving

/// Handles an incoming `file` stream.
pub(crate) async fn receive(inner: Arc<Inner>, peer: DeviceId, mut send: SendStream, mut recv: RecvStream) {
    let allowed = inner.permissions_of(&peer).is_some_and(|p| p.files);
    let offer = match read_message::<_, FileMsg>(&mut recv, PAIR_FRAME_LIMIT).await {
        Ok(Some(FileMsg::Offer { id, name, size, .. })) => (id, name, size),
        _ => return,
    };
    let (id, name, size) = offer;
    if !allowed {
        let reason = Some("This device isn't allowed to send files here.".to_string());
        let _ = write_message(&mut send, &FileMsg::Answer { accept: false, offset: 0, reason }).await;
        let _ = send.finish();
        return;
    }
    let dir = inner.transfers.downloads.read().clone();
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        let reason = Some(format!("Couldn't create the downloads folder: {e}"));
        let _ = write_message(&mut send, &FileMsg::Answer { accept: false, offset: 0, reason }).await;
        return;
    }
    let name = safe_file_name(&name);
    let part = dir.join(format!(".devti-{id}.part"));
    let device_name = inner.peers.read().get(&peer).map(|p| p.record.info.name.clone()).unwrap_or_default();

    // Resume only a partial file that belongs to this very transfer.
    let prior = inner.store.transfer(id).ok().flatten();
    let resumable = prior.as_ref().is_some_and(|t| t.incoming && t.device_id == peer && t.size == size);
    let existing = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    let offset = if resumable && existing <= size { existing } else { 0 };

    let mut rec = TransferRecord {
        id,
        device_id: peer,
        device_name,
        incoming: true,
        name: name.clone(),
        path: part.to_string_lossy().into(),
        size,
        done: offset,
        state: "active".into(),
        ts: prior.map(|p| p.ts).unwrap_or_else(now_ms),
    };
    let _ = inner.store.upsert_transfer(&rec);
    emit(&inner, &rec, 0);

    match receive_into(&inner, &mut send, &mut recv, &part, &dir, &mut rec, offset).await {
        Ok(final_path) => {
            rec.path = final_path.to_string_lossy().into();
            rec.done = size;
            inner.log(Some((&peer, &rec.device_name)), "file_received", &rec.name);
            finish(&inner, &mut rec, "done");
        }
        Err(Error::Connection(reason)) if reason.contains("cancel") => {
            let _ = tokio::fs::remove_file(&part).await;
            finish(&inner, &mut rec, "cancelled");
        }
        Err(Error::Protocol(reason)) => {
            tracing::warn!(id, %reason, "incoming file rejected");
            let _ = tokio::fs::remove_file(&part).await;
            finish(&inner, &mut rec, "failed");
        }
        Err(_) => finish(&inner, &mut rec, "paused"),
    }
}

async fn receive_into(
    inner: &Arc<Inner>,
    send: &mut SendStream,
    recv: &mut RecvStream,
    part: &Path,
    dir: &Path,
    rec: &mut TransferRecord,
    offset: u64,
) -> Result<PathBuf> {
    let mut file = tokio::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(part).await?;
    file.set_len(offset).await?;
    // Hash what is already on disk so the final check covers the whole file.
    let mut hasher = Context::new(&SHA256);
    let mut buf = vec![0u8; CHUNK];
    file.seek(std::io::SeekFrom::Start(0)).await?;
    let mut left = offset;
    while left > 0 {
        let n = file.read(&mut buf[..(left.min(CHUNK as u64) as usize)]).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    write_message(send, &FileMsg::Answer { accept: true, offset, reason: None }).await?;

    let (mut last, mut last_done) = (Instant::now(), offset);
    let mut remaining = rec.size - offset;
    while remaining > 0 {
        let want = remaining.min(CHUNK as u64) as usize;
        let n = match recv.read(&mut buf[..want]).await {
            Ok(Some(n)) => n,
            Ok(None) => return Err(Error::Connection("stream ended early".into())),
            Err(LinkReadError::Reset(code)) if code == u64::from(CANCELLED) => {
                return Err(Error::Connection("cancelled by sender".into()));
            }
            Err(e) => return Err(Error::Connection(e.to_string())),
        };
        file.write_all(&buf[..n]).await?;
        hasher.update(&buf[..n]);
        remaining -= n as u64;
        rec.done += n as u64;
        if last.elapsed() >= PROGRESS_EVERY {
            file.flush().await?;
            let rate = ((rec.done - last_done) as f64 / last.elapsed().as_secs_f64()) as u64;
            emit(inner, rec, rate);
            let _ = inner.store.upsert_transfer(rec);
            (last, last_done) = (Instant::now(), rec.done);
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    let expected = match read_message::<_, FileMsg>(recv, PAIR_FRAME_LIMIT).await? {
        Some(FileMsg::Done { sha256 }) => sha256,
        _ => return Err(Error::Protocol("missing checksum".into())),
    };
    if hasher.finish().as_ref() != expected.as_ref() {
        return Err(Error::Protocol("checksum mismatch".into()));
    }
    let final_path = unique_path(dir, &rec.name);
    tokio::fs::rename(part, &final_path).await?;
    write_message(send, &FileMsg::Saved).await?;
    let _ = send.finish();
    Ok(final_path)
}

fn guess_mime(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        "zip" => "application/zip",
        _ => return None,
    })
}

// ------------------------------------------------------------------ public API

impl super::Node {
    /// Queues `path` to be sent to `peer`; it starts now if the peer is online and
    /// resumes automatically after disconnects. Returns the transfer id.
    pub fn send_file(&self, peer: &DeviceId, path: &Path) -> Result<u64> {
        let inner = &self.inner;
        let meta = std::fs::metadata(path).map_err(|e| Error::InvalidInput(format!("can't read that file: {e}")))?;
        if !meta.is_file() {
            return Err(Error::InvalidInput("folders can't be sent yet — zip them first".into()));
        }
        let device_name = inner
            .peers
            .read()
            .get(peer)
            .map(|p| p.record.info.name.clone())
            .ok_or_else(|| Error::UnknownPeer(peer.to_string()))?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
        let rec = TransferRecord {
            id: crate::pairing::new_id()?,
            device_id: *peer,
            device_name,
            incoming: false,
            name,
            path: path.to_string_lossy().into(),
            size: meta.len(),
            done: 0,
            state: "queued".into(),
            ts: now_ms(),
        };
        inner.store.upsert_transfer(&rec)?;
        emit(inner, &rec, 0);
        let id = rec.id;
        start_send(inner, rec);
        Ok(id)
    }

    /// Cancels a running or queued transfer.
    pub fn cancel_transfer(&self, id: u64) -> Result<()> {
        if let Some(flag) = self.inner.transfers.running.lock().get(&id) {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            return Ok(());
        }
        let mut rec = self.inner.store.transfer(id)?.ok_or_else(|| Error::InvalidInput("no such transfer".into()))?;
        if rec.incoming && rec.state != "done" {
            let _ = std::fs::remove_file(&rec.path);
        }
        finish(&self.inner, &mut rec, "cancelled");
        Ok(())
    }

    /// Retries a failed or paused outgoing transfer.
    pub fn retry_transfer(&self, id: u64) -> Result<()> {
        let mut rec = self.inner.store.transfer(id)?.ok_or_else(|| Error::InvalidInput("no such transfer".into()))?;
        if rec.incoming {
            return Err(Error::InvalidInput("only the sending device can retry".into()));
        }
        rec.state = "queued".into();
        start_send(&self.inner, rec);
        Ok(())
    }

    /// Recent transfers, newest first.
    pub fn transfers(&self) -> Result<Vec<TransferRecord>> {
        self.inner.store.transfers(200)
    }

    pub fn clear_finished_transfers(&self) -> Result<()> {
        self.inner.store.clear_finished_transfers()
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.inner.transfers.downloads.read().clone()
    }

    pub fn set_downloads_dir(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        *self.inner.transfers.downloads.write() = dir.to_path_buf();
        self.inner.store.set_setting(SETTING_DOWNLOADS, &dir.to_string_lossy())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\Users\\x\\photo.jpg"), "photo.jpg");
        assert_eq!(safe_file_name("a<b>c:d|e?.txt"), "abcde.txt");
        assert_eq!(safe_file_name("..hidden"), "hidden");
        assert!(safe_file_name("CON.txt").starts_with("file-"));
        assert!(safe_file_name("").starts_with("file-"));
        assert_eq!(safe_file_name(&"x".repeat(500)).len(), 200);
    }

    #[test]
    fn unique_paths_do_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unique_path(dir.path(), "a.txt"), dir.path().join("a.txt"));
        std::fs::write(dir.path().join("a.txt"), b"1").unwrap();
        assert_eq!(unique_path(dir.path(), "a.txt"), dir.path().join("a (2).txt"));
        std::fs::write(dir.path().join("noext"), b"1").unwrap();
        assert_eq!(unique_path(dir.path(), "noext"), dir.path().join("noext (2)"));
    }

    #[test]
    fn mime_guesses() {
        assert_eq!(guess_mime("a.JPG"), Some("image/jpeg"));
        assert_eq!(guess_mime("a.unknown"), None);
    }
}
