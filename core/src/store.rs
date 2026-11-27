//! Persistent state in one SQLite file: trusted devices, settings and the activity log.
//!
//! Schema changes go through numbered migrations keyed on `PRAGMA user_version`, so an
//! older database is always upgraded in place and never silently reset.

use std::net::SocketAddr;
use std::path::Path;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{DeviceId, DeviceInfo, DeviceStatus, Fingerprint, Permissions, TrustRole};

const MIGRATIONS: &[&str] = &[
    // v1 — Phase 1 foundation.
    "CREATE TABLE peers (
        id               TEXT PRIMARY KEY,
        fingerprint      BLOB NOT NULL,
        info_json        TEXT NOT NULL,
        role             TEXT NOT NULL,
        permissions_json TEXT NOT NULL,
        paired_at        INTEGER NOT NULL,
        last_seen        INTEGER,
        last_addrs_json  TEXT NOT NULL DEFAULT '[]',
        status_json      TEXT NOT NULL DEFAULT '{}'
     );
     CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
     CREATE TABLE activity (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        ts          INTEGER NOT NULL,
        device_id   TEXT,
        device_name TEXT,
        action      TEXT NOT NULL,
        detail      TEXT NOT NULL DEFAULT ''
     );
     CREATE INDEX activity_ts ON activity(ts DESC);
     CREATE INDEX activity_device ON activity(device_id, ts DESC);",
    // v2 — Phase 2 continuity.
    "CREATE TABLE clipboard (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        ts          INTEGER NOT NULL,
        device_id   TEXT,
        device_name TEXT,
        text        TEXT NOT NULL
     );
     CREATE TABLE battery (
        device_id TEXT NOT NULL,
        ts        INTEGER NOT NULL,
        percent   INTEGER NOT NULL,
        charging  INTEGER NOT NULL
     );
     CREATE INDEX battery_device ON battery(device_id, ts);
     CREATE TABLE transfers (
        id          INTEGER PRIMARY KEY,
        device_id   TEXT NOT NULL,
        device_name TEXT NOT NULL,
        incoming    INTEGER NOT NULL,
        name        TEXT NOT NULL,
        path        TEXT NOT NULL,
        size        INTEGER NOT NULL,
        done        INTEGER NOT NULL DEFAULT 0,
        state       TEXT NOT NULL,
        ts          INTEGER NOT NULL
     );",
];

/// Clipboard entries kept in history.
pub const CLIPBOARD_HISTORY: u32 = 50;

/// A trusted (paired) device as remembered on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRecord {
    pub info: DeviceInfo,
    pub fingerprint: Fingerprint,
    pub role: TrustRole,
    pub permissions: Permissions,
    /// Unix milliseconds.
    pub paired_at: i64,
    pub last_seen: Option<i64>,
    pub last_addrs: Vec<SocketAddr>,
    pub last_status: DeviceStatus,
}

/// One line of the activity log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub id: i64,
    pub ts: i64,
    pub device_id: Option<DeviceId>,
    pub device_name: Option<String>,
    pub action: String,
    pub detail: String,
}

/// Thread-safe handle to the database.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Opens (creating if needed) the database at `path` and runs migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    /// Opens a private in-memory database (tests, ephemeral nodes).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version as usize > MIGRATIONS.len() {
            return Err(Error::Storage(format!(
                "database schema v{version} is newer than this Devti build supports — update Devti"
            )));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;", i + 1))?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    // ---------------------------------------------------------------- peers

    pub fn upsert_peer(&self, p: &PeerRecord) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO peers (id, fingerprint, info_json, role, permissions_json, paired_at,
                                last_seen, last_addrs_json, status_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                fingerprint = excluded.fingerprint, info_json = excluded.info_json,
                role = excluded.role, permissions_json = excluded.permissions_json,
                paired_at = excluded.paired_at, last_seen = excluded.last_seen,
                last_addrs_json = excluded.last_addrs_json, status_json = excluded.status_json",
            params![
                p.info.id.to_string(),
                p.fingerprint.0.to_vec(),
                serde_json::to_string(&p.info)?,
                p.role.as_str(),
                serde_json::to_string(&p.permissions)?,
                p.paired_at,
                p.last_seen,
                serde_json::to_string(&p.last_addrs)?,
                serde_json::to_string(&p.last_status)?,
            ],
        )?;
        Ok(())
    }

    pub fn peers(&self) -> Result<Vec<PeerRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT fingerprint, info_json, role, permissions_json, paired_at, last_seen,
                    last_addrs_json, status_json FROM peers ORDER BY paired_at",
        )?;
        let rows = stmt.query_map([], row_to_raw)?;
        let mut out = Vec::new();
        for raw in rows {
            match raw?.into_record() {
                Ok(rec) => out.push(rec),
                Err(e) => tracing::warn!(error = %e, "skipping unreadable peer row"),
            }
        }
        Ok(out)
    }

    pub fn peer(&self, id: &DeviceId) -> Result<Option<PeerRecord>> {
        let conn = self.conn.lock();
        let raw = conn
            .query_row(
                "SELECT fingerprint, info_json, role, permissions_json, paired_at, last_seen,
                        last_addrs_json, status_json FROM peers WHERE id = ?1",
                [id.to_string()],
                row_to_raw,
            )
            .optional()?;
        raw.map(RawPeer::into_record).transpose()
    }

    /// Removes a peer; returns whether it existed.
    pub fn remove_peer(&self, id: &DeviceId) -> Result<bool> {
        Ok(self.conn.lock().execute("DELETE FROM peers WHERE id = ?1", [id.to_string()])? > 0)
    }

    pub fn update_peer_seen(&self, id: &DeviceId, ts: i64, addrs: &[SocketAddr]) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE peers SET last_seen = ?2, last_addrs_json = ?3 WHERE id = ?1",
            params![id.to_string(), ts, serde_json::to_string(addrs)?],
        )?;
        Ok(())
    }

    pub fn update_peer_info(&self, info: &DeviceInfo) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE peers SET info_json = ?2 WHERE id = ?1",
            params![info.id.to_string(), serde_json::to_string(info)?],
        )?;
        Ok(())
    }

    pub fn update_peer_status(&self, id: &DeviceId, status: &DeviceStatus) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE peers SET status_json = ?2 WHERE id = ?1",
            params![id.to_string(), serde_json::to_string(status)?],
        )?;
        Ok(())
    }

    pub fn set_peer_access(&self, id: &DeviceId, role: TrustRole, perms: &Permissions) -> Result<bool> {
        let n = self.conn.lock().execute(
            "UPDATE peers SET role = ?2, permissions_json = ?3 WHERE id = ?1",
            params![id.to_string(), role.as_str(), serde_json::to_string(perms)?],
        )?;
        Ok(n > 0)
    }

    // ------------------------------------------------------------- settings

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.lock().query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Every setting as `(key, value)` pairs.
    pub fn settings(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ------------------------------------------------------------- activity

    pub fn log_activity(&self, ts: i64, device: Option<(&DeviceId, &str)>, action: &str, detail: &str) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO activity (ts, device_id, device_name, action, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![ts, device.map(|d| d.0.to_string()), device.map(|d| d.1.to_string()), action, detail],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Newest-first page of the activity log, optionally for one device.
    pub fn activity(
        &self,
        device: Option<&DeviceId>,
        limit: u32,
        before_id: Option<i64>,
    ) -> Result<Vec<ActivityEntry>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, ts, device_id, device_name, action, detail FROM activity
             WHERE (?1 IS NULL OR device_id = ?1) AND (?2 IS NULL OR id < ?2)
             ORDER BY id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![device.map(|d| d.to_string()), before_id, limit.min(1000)], |r| {
            let device_id: Option<String> = r.get(2)?;
            Ok(ActivityEntry {
                id: r.get(0)?,
                ts: r.get(1)?,
                device_id: device_id.and_then(|s| s.parse().ok()),
                device_name: r.get(3)?,
                action: r.get(4)?,
                detail: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

/// One clipboard history entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub id: i64,
    pub ts: i64,
    /// `None` = copied on this device.
    pub device_id: Option<DeviceId>,
    pub device_name: Option<String>,
    pub text: String,
}

/// One battery sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatterySample {
    pub ts: i64,
    pub percent: u8,
    pub charging: bool,
}

/// A file transfer as remembered on disk (history and resume).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRecord {
    pub id: u64,
    pub device_id: DeviceId,
    pub device_name: String,
    pub incoming: bool,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub done: u64,
    /// `queued`, `active`, `paused`, `done`, `failed`, `cancelled`.
    pub state: String,
    pub ts: i64,
}

impl Store {
    // ------------------------------------------------------------ clipboard

    pub fn add_clipboard(&self, ts: i64, device: Option<(&DeviceId, &str)>, text: &str) -> Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO clipboard (ts, device_id, device_name, text) VALUES (?1, ?2, ?3, ?4)",
            params![ts, device.map(|d| d.0.to_string()), device.map(|d| d.1.to_string()), text],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "DELETE FROM clipboard WHERE id NOT IN (SELECT id FROM clipboard ORDER BY id DESC LIMIT ?1)",
            [CLIPBOARD_HISTORY],
        )?;
        Ok(id)
    }

    pub fn clipboard_history(&self) -> Result<Vec<ClipboardEntry>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT id, ts, device_id, device_name, text FROM clipboard ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt.query_map([CLIPBOARD_HISTORY], |r| {
            let device_id: Option<String> = r.get(2)?;
            Ok(ClipboardEntry {
                id: r.get(0)?,
                ts: r.get(1)?,
                device_id: device_id.and_then(|s| s.parse().ok()),
                device_name: r.get(3)?,
                text: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn clear_clipboard(&self) -> Result<()> {
        self.conn.lock().execute("DELETE FROM clipboard", [])?;
        Ok(())
    }

    // -------------------------------------------------------------- battery

    /// Stores a sample unless it repeats the previous one within 10 minutes.
    pub fn add_battery(&self, device: &DeviceId, s: BatterySample) -> Result<bool> {
        self.add_battery_key(&device.to_string(), s)
    }

    /// Battery history under any key (paired devices use their id, accessories `bt:<address>`).
    pub fn add_battery_key(&self, key: &str, s: BatterySample) -> Result<bool> {
        let device = key;
        let conn = self.conn.lock();
        let last: Option<(i64, i64, bool)> = conn
            .query_row(
                "SELECT ts, percent, charging FROM battery WHERE device_id = ?1 ORDER BY ts DESC LIMIT 1",
                [device],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((ts, pct, ch)) = last
            && pct == s.percent as i64
            && ch == s.charging
            && s.ts - ts < 10 * 60 * 1000
        {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO battery (device_id, ts, percent, charging) VALUES (?1, ?2, ?3, ?4)",
            params![device.to_string(), s.ts, s.percent, s.charging],
        )?;
        // Keep 30 days.
        conn.execute("DELETE FROM battery WHERE ts < ?1", [s.ts - 30 * 24 * 3600 * 1000])?;
        Ok(true)
    }

    pub fn battery_history(&self, device: &DeviceId, since: i64) -> Result<Vec<BatterySample>> {
        self.battery_history_key(&device.to_string(), since)
    }

    pub fn battery_history_key(&self, device: &str, since: i64) -> Result<Vec<BatterySample>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT ts, percent, charging FROM battery WHERE device_id = ?1 AND ts >= ?2 ORDER BY ts")?;
        let rows = stmt.query_map(params![device, since], |r| {
            Ok(BatterySample { ts: r.get(0)?, percent: r.get::<_, i64>(1)?.clamp(0, 100) as u8, charging: r.get(2)? })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ------------------------------------------------------------ transfers

    pub fn upsert_transfer(&self, t: &TransferRecord) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO transfers (id, device_id, device_name, incoming, name, path, size, done, state, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET done = excluded.done, state = excluded.state, path = excluded.path",
            params![
                t.id as i64,
                t.device_id.to_string(),
                t.device_name,
                t.incoming,
                t.name,
                t.path,
                t.size as i64,
                t.done as i64,
                t.state,
                t.ts
            ],
        )?;
        Ok(())
    }

    pub fn transfer(&self, id: u64) -> Result<Option<TransferRecord>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT id, device_id, device_name, incoming, name, path, size, done, state, ts
                 FROM transfers WHERE id = ?1",
                [id as i64],
                row_to_transfer,
            )
            .optional()?
            .flatten())
    }

    pub fn transfers(&self, limit: u32) -> Result<Vec<TransferRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, device_id, device_name, incoming, name, path, size, done, state, ts
             FROM transfers ORDER BY ts DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit.min(500)], row_to_transfer)?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    pub fn clear_finished_transfers(&self) -> Result<()> {
        self.conn.lock().execute("DELETE FROM transfers WHERE state IN ('done', 'failed', 'cancelled')", [])?;
        Ok(())
    }
}

fn row_to_transfer(r: &rusqlite::Row<'_>) -> rusqlite::Result<Option<TransferRecord>> {
    let device: String = r.get(1)?;
    let Ok(device_id) = device.parse() else { return Ok(None) };
    Ok(Some(TransferRecord {
        id: r.get::<_, i64>(0)? as u64,
        device_id,
        device_name: r.get(2)?,
        incoming: r.get(3)?,
        name: r.get(4)?,
        path: r.get(5)?,
        size: r.get::<_, i64>(6)? as u64,
        done: r.get::<_, i64>(7)? as u64,
        state: r.get(8)?,
        ts: r.get(9)?,
    }))
}

struct RawPeer {
    fingerprint: Vec<u8>,
    info: String,
    role: String,
    perms: String,
    paired_at: i64,
    last_seen: Option<i64>,
    addrs: String,
    status: String,
}

fn row_to_raw(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawPeer> {
    Ok(RawPeer {
        fingerprint: r.get(0)?,
        info: r.get(1)?,
        role: r.get(2)?,
        perms: r.get(3)?,
        paired_at: r.get(4)?,
        last_seen: r.get(5)?,
        addrs: r.get(6)?,
        status: r.get(7)?,
    })
}

impl RawPeer {
    fn into_record(self) -> Result<PeerRecord> {
        let fp: [u8; 32] = self.fingerprint.try_into().map_err(|_| Error::Storage("corrupt fingerprint".into()))?;
        Ok(PeerRecord {
            info: serde_json::from_str(&self.info)?,
            fingerprint: Fingerprint(fp),
            role: TrustRole::parse(&self.role),
            permissions: serde_json::from_str(&self.perms)?,
            paired_at: self.paired_at,
            last_seen: self.last_seen,
            last_addrs: serde_json::from_str(&self.addrs).unwrap_or_default(),
            last_status: serde_json::from_str(&self.status).unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BatteryState, DeviceKind, Platform};

    fn record(seed: u8) -> PeerRecord {
        let fp = Fingerprint([seed; 32]);
        PeerRecord {
            info: DeviceInfo {
                id: fp.device_id(),
                name: format!("Device {seed}"),
                kind: DeviceKind::Phone,
                platform: Platform::Android,
                os_version: "Android 15".into(),
                model: Some("Pixel".into()),
                app_version: "0.1.0".into(),
                capabilities: vec![],
            },
            fingerprint: fp,
            role: TrustRole::Trusted,
            permissions: Permissions::for_role(TrustRole::Trusted),
            paired_at: seed as i64,
            last_seen: None,
            last_addrs: vec![],
            last_status: DeviceStatus::default(),
        }
    }

    #[test]
    fn peers_crud() {
        let s = Store::open_in_memory().unwrap();
        let a = record(1);
        let b = record(2);
        s.upsert_peer(&a).unwrap();
        s.upsert_peer(&b).unwrap();
        assert_eq!(s.peers().unwrap(), vec![a.clone(), b.clone()]);
        assert_eq!(s.peer(&a.info.id).unwrap().unwrap(), a);

        let addr: SocketAddr = "10.0.0.2:47810".parse().unwrap();
        s.update_peer_seen(&a.info.id, 99, &[addr]).unwrap();
        let mut info = a.info.clone();
        info.name = "Renamed".into();
        s.update_peer_info(&info).unwrap();
        let status = DeviceStatus { battery: Some(BatteryState::new(42, false)), ..Default::default() };
        s.update_peer_status(&a.info.id, &status).unwrap();
        let perms = Permissions::for_role(TrustRole::Restricted);
        assert!(s.set_peer_access(&a.info.id, TrustRole::Restricted, &perms).unwrap());
        let got = s.peer(&a.info.id).unwrap().unwrap();
        assert_eq!(got.last_seen, Some(99));
        assert_eq!(got.last_addrs, vec![addr]);
        assert_eq!(got.info.name, "Renamed");
        assert_eq!(got.last_status, status);
        assert_eq!(got.role, TrustRole::Restricted);
        assert_eq!(got.permissions, perms);

        assert!(s.remove_peer(&a.info.id).unwrap());
        assert!(!s.remove_peer(&a.info.id).unwrap());
        assert!(s.peer(&a.info.id).unwrap().is_none());
        assert!(!s.set_peer_access(&a.info.id, TrustRole::Trusted, &perms).unwrap());
    }

    #[test]
    fn clipboard_history_is_capped() {
        let s = Store::open_in_memory().unwrap();
        let id = record(1).info.id;
        for i in 0..(CLIPBOARD_HISTORY + 5) {
            s.add_clipboard(i as i64, if i % 2 == 0 { Some((&id, "Phone")) } else { None }, &format!("t{i}")).unwrap();
        }
        let h = s.clipboard_history().unwrap();
        assert_eq!(h.len(), CLIPBOARD_HISTORY as usize);
        assert_eq!(h[0].text, format!("t{}", CLIPBOARD_HISTORY + 4));
        s.clear_clipboard().unwrap();
        assert!(s.clipboard_history().unwrap().is_empty());
    }

    #[test]
    fn battery_history_dedupes_and_ranges() {
        let s = Store::open_in_memory().unwrap();
        let id = record(1).info.id;
        let at = |ts, percent| BatterySample { ts, percent, charging: false };
        assert!(s.add_battery(&id, at(1_000, 80)).unwrap());
        assert!(!s.add_battery(&id, at(2_000, 80)).unwrap(), "same value soon after is skipped");
        assert!(s.add_battery(&id, at(3_000, 79)).unwrap());
        assert!(s.add_battery(&id, at(3_000 + 11 * 60 * 1000, 79)).unwrap(), "heartbeat after 10 min");
        assert_eq!(s.battery_history(&id, 0).unwrap().len(), 3);
        assert_eq!(s.battery_history(&id, 2_500).unwrap().len(), 2);
    }

    #[test]
    fn transfers_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        let id = record(1).info.id;
        let mut t = TransferRecord {
            id: 42,
            device_id: id,
            device_name: "Phone".into(),
            incoming: true,
            name: "a.txt".into(),
            path: "/tmp/a.txt".into(),
            size: 10,
            done: 0,
            state: "active".into(),
            ts: 5,
        };
        s.upsert_transfer(&t).unwrap();
        t.done = 10;
        t.state = "done".into();
        s.upsert_transfer(&t).unwrap();
        assert_eq!(s.transfer(42).unwrap().unwrap(), t);
        assert_eq!(s.transfers(10).unwrap().len(), 1);
        assert!(s.transfer(7).unwrap().is_none());
        s.clear_finished_transfers().unwrap();
        assert!(s.transfers(10).unwrap().is_empty());
    }

    #[test]
    fn settings_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.setting("theme").unwrap(), None);
        s.set_setting("theme", "dark").unwrap();
        s.set_setting("theme", "light").unwrap();
        s.set_setting("autostart", "true").unwrap();
        assert_eq!(s.setting("theme").unwrap().as_deref(), Some("light"));
        assert_eq!(s.settings().unwrap().len(), 2);
    }

    #[test]
    fn activity_log_pages_and_filters() {
        let s = Store::open_in_memory().unwrap();
        let a = record(1).info.id;
        for i in 0..5 {
            s.log_activity(i, Some((&a, "Phone")), "connected", "").unwrap();
        }
        s.log_activity(10, None, "paused", "kill switch").unwrap();
        let all = s.activity(None, 100, None).unwrap();
        assert_eq!(all.len(), 6);
        assert_eq!(all[0].action, "paused");
        assert_eq!(all[0].device_id, None);
        let mine = s.activity(Some(&a), 2, None).unwrap();
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].device_name.as_deref(), Some("Phone"));
        let next = s.activity(Some(&a), 10, Some(mine[1].id)).unwrap();
        assert_eq!(next.len(), 3);
    }

    #[test]
    fn database_persists_and_migrates_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/devti.db");
        {
            let s = Store::open(&path).unwrap();
            s.upsert_peer(&record(3)).unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.peers().unwrap().len(), 1);
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devti.db");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", 999).unwrap();
        }
        let err = Store::open(&path).err().unwrap();
        assert!(err.to_string().contains("newer"));
    }

    #[test]
    fn corrupt_rows_are_skipped_not_fatal() {
        let s = Store::open_in_memory().unwrap();
        s.upsert_peer(&record(1)).unwrap();
        s.conn
            .lock()
            .execute("INSERT INTO peers (id, fingerprint, info_json, role, permissions_json, paired_at) VALUES ('X', x'00', '{}', 'trusted', '{}', 5)", [])
            .unwrap();
        assert_eq!(s.peers().unwrap().len(), 1);
    }
}
