//! Phase 7 shared folders: browse and download what another device chose to share.
//!
//! Nothing is shared until the owner adds a folder, and a peer also needs the `browse`
//! permission. Peers only ever see *virtual* paths — `/<folder name>/sub/file` — never the
//! real location. Each request is resolved against the shared folder, refused if it
//! names `..`, a drive or a separator trick, and then checked again after following
//! symbolic links, so nothing outside a shared folder can be listed or sent. Downloads
//! travel as ordinary, resumable file transfers.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{Inner, Node, NodeEvent};
use crate::model::DeviceId;
use crate::protocol::{ControlMsg, FsEntry};
use crate::{Error, Result};

const SETTING: &str = "shared_folders";
/// Entries in one listing.
pub const MAX_ENTRIES: usize = 2000;
/// Folders one device can share.
pub const MAX_FOLDERS: usize = 32;

/// A folder this device shares, under the name peers see.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedFolder {
    pub name: String,
    pub path: String,
}

fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

pub(crate) fn folders(inner: &Inner) -> Vec<SharedFolder> {
    inner
        .store
        .setting(SETTING)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<SharedFolder>>(&s).ok())
        .unwrap_or_default()
}

/// Splits a virtual path into the folder name and the parts below it, refusing anything
/// that could climb out (`..`, `.`, separators inside a part, drive letters, NUL).
fn split(vpath: &str) -> Result<(String, Vec<String>)> {
    if !vpath.starts_with('/') || vpath.len() > 4096 {
        return Err(Error::InvalidInput("paths start with /".into()));
    }
    let parts: Vec<&str> = vpath.split('/').filter(|p| !p.is_empty()).collect();
    let Some((first, rest)) = parts.split_first() else {
        return Err(Error::InvalidInput("that's the list of shared folders".into()));
    };
    for p in &parts {
        if *p == ".." || *p == "." || p.contains('\\') || p.contains(':') || p.contains('\0') {
            return Err(Error::InvalidInput("that path isn't allowed".into()));
        }
    }
    Ok((first.to_string(), rest.iter().map(|s| s.to_string()).collect()))
}

/// The real path for a virtual one, guaranteed to be inside a shared folder.
pub(crate) fn resolve(folders: &[SharedFolder], vpath: &str) -> Result<PathBuf> {
    let (name, rest) = split(vpath)?;
    let folder =
        folders.iter().find(|f| f.name == name).ok_or_else(|| Error::InvalidInput(format!("“{name}” isn't shared")))?;
    let root = std::fs::canonicalize(&folder.path)
        .map_err(|_| Error::InvalidInput(format!("the shared folder “{name}” is missing on this device")))?;
    let mut joined = root.clone();
    for part in &rest {
        let p = Path::new(part);
        // One normal component each, nothing else.
        if p.components().count() != 1 || !matches!(p.components().next(), Some(Component::Normal(_))) {
            return Err(Error::InvalidInput("that path isn't allowed".into()));
        }
        joined.push(p);
    }
    let real = std::fs::canonicalize(&joined).map_err(|_| Error::InvalidInput("that file or folder is gone".into()))?;
    if !real.starts_with(&root) {
        return Err(Error::InvalidInput("that path leads outside the shared folder".into()));
    }
    Ok(real)
}

fn modified_ms(m: &std::fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Lists a virtual path: "/" gives the shared folders, anything else that folder's
/// entries (folders first, then by name; hidden files left out).
pub(crate) fn list(folders: &[SharedFolder], vpath: &str) -> Result<(Vec<FsEntry>, bool)> {
    if vpath.trim_end_matches('/').is_empty() {
        let entries = folders
            .iter()
            .map(|f| {
                let modified = std::fs::metadata(&f.path).map(|m| modified_ms(&m)).unwrap_or(0);
                FsEntry { name: f.name.clone(), dir: true, size: 0, modified }
            })
            .collect();
        return Ok((entries, false));
    }
    let dir = resolve(folders, vpath)?;
    if !dir.is_dir() {
        return Err(Error::InvalidInput("that's a file, not a folder".into()));
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    for e in std::fs::read_dir(&dir).map_err(|e| Error::InvalidInput(format!("can't open that folder: {e}")))? {
        let Ok(e) = e else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = std::fs::metadata(e.path()) else { continue };
        if entries.len() == MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(FsEntry {
            name,
            dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: modified_ms(&meta),
        });
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok((entries, truncated))
}

/// Requests and answers from a peer.
pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, msg: ControlMsg) {
    let Some(perms) = inner.permissions_of(&peer) else { return };
    let name = inner.peer_name(&peer);
    match msg {
        ControlMsg::FsList { path } => {
            if !perms.browse {
                let error = Some("This device doesn't let you browse its files.".to_string());
                let _ =
                    inner.send_to(&peer, ControlMsg::FsListing { path, entries: Vec::new(), truncated: false, error });
                return;
            }
            let inner2 = inner.clone();
            inner.rt.spawn_blocking(move || {
                let reply = match list(&folders(&inner2), &path) {
                    Ok((entries, truncated)) => ControlMsg::FsListing { path, entries, truncated, error: None },
                    Err(e) => ControlMsg::FsListing {
                        path,
                        entries: Vec::new(),
                        truncated: false,
                        error: Some(e.to_string()),
                    },
                };
                let _ = inner2.send_to(&peer, reply);
            });
        }
        ControlMsg::FsGet { path } => {
            let result = if !perms.browse {
                Err(Error::InvalidInput("This device doesn't let you download its files.".into()))
            } else {
                resolve(&folders(inner), &path).and_then(|real| {
                    if !real.is_file() {
                        return Err(Error::InvalidInput("only files can be downloaded".into()));
                    }
                    inner.log(Some((&peer, &name)), "shared_file_sent", &path);
                    Node { inner: inner.clone() }.send_file(&peer, &real).map(|_| ())
                })
            };
            if let Err(e) = result {
                let _ = inner.send_to(&peer, ControlMsg::FsError { path, error: e.to_string() });
            }
        }
        ControlMsg::FsListing { path, entries, truncated, error } => {
            let entries: Vec<FsEntry> = entries
                .into_iter()
                .take(MAX_ENTRIES)
                .map(|e| FsEntry { name: clean_text(&e.name, 255), ..e })
                .filter(|e| !e.name.is_empty())
                .collect();
            inner.emit(NodeEvent::FsListing {
                from: peer,
                path: clean_text(&path, 4096),
                entries,
                truncated,
                error: error.map(|e| clean_text(&e, 300)),
            });
        }
        ControlMsg::FsError { path, error } => {
            inner.emit(NodeEvent::FsError {
                from: peer,
                path: clean_text(&path, 4096),
                error: clean_text(&error, 300),
            });
        }
        _ => {}
    }
}

impl Node {
    /// The folders this device shares.
    pub fn shared_folders(&self) -> Vec<SharedFolder> {
        folders(&self.inner)
    }

    /// Replaces the shared folders. Names must be unique and can't contain "/" or "\";
    /// paths must be existing folders.
    pub fn set_shared_folders(&self, list: Vec<SharedFolder>) -> Result<()> {
        if list.len() > MAX_FOLDERS {
            return Err(Error::InvalidInput(format!("at most {MAX_FOLDERS} shared folders")));
        }
        let mut seen = std::collections::HashSet::new();
        let mut clean = Vec::new();
        for f in list {
            let name = clean_text(f.name.trim(), 64);
            if name.is_empty()
                || name.contains('/')
                || name.contains('\\')
                || name == "."
                || name == ".."
                || name.contains(':')
            {
                return Err(Error::InvalidInput(format!("“{name}” can't be a shared folder's name")));
            }
            if !seen.insert(name.to_lowercase()) {
                return Err(Error::InvalidInput(format!("two shared folders are called “{name}”")));
            }
            if !Path::new(&f.path).is_dir() {
                return Err(Error::InvalidInput(format!("“{}” isn't a folder on this device", f.path)));
            }
            clean.push(SharedFolder { name, path: f.path });
        }
        self.inner.store.set_setting(SETTING, &serde_json::to_string(&clean)?)?;
        self.inner.log(None, "shared_folders_changed", &clean.len().to_string());
        Ok(())
    }

    /// Asks `peer` for a listing; the answer arrives as `NodeEvent::FsListing`.
    pub fn fs_list(&self, peer: &DeviceId, path: &str) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::FsList { path: path.to_string() })
    }

    /// Asks `peer` to send one of its shared files; it arrives as a file transfer (or a
    /// `NodeEvent::FsError`).
    pub fn fs_get(&self, peer: &DeviceId, path: &str) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::FsGet { path: path.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Vec<SharedFolder>) {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        std::fs::create_dir_all(docs.join("2026")).unwrap();
        std::fs::write(docs.join("plan.pdf"), b"plan").unwrap();
        std::fs::write(docs.join(".secret"), b"hidden").unwrap();
        std::fs::write(docs.join("2026/b.txt"), b"bb").unwrap();
        std::fs::write(dir.path().join("outside.txt"), b"nope").unwrap();
        let folders = vec![SharedFolder { name: "Documents".into(), path: docs.to_string_lossy().into() }];
        (dir, folders)
    }

    #[test]
    fn root_lists_shared_folders_and_folders_list_entries() {
        let (_d, f) = setup();
        let (root, _) = list(&f, "/").unwrap();
        assert_eq!(root.len(), 1);
        assert!(root[0].dir && root[0].name == "Documents");
        let (entries, truncated) = list(&f, "/Documents").unwrap();
        assert!(!truncated);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["2026", "plan.pdf"], "folders first, hidden files left out");
        assert_eq!(entries[1].size, 4);
        let (sub, _) = list(&f, "/Documents/2026/").unwrap();
        assert_eq!(sub[0].name, "b.txt");
    }

    #[test]
    fn nothing_outside_a_shared_folder_is_reachable() {
        let (dir, f) = setup();
        for bad in [
            "/Documents/../outside.txt",
            "/Documents/./plan.pdf",
            "/Documents/..\\outside.txt",
            "/Documents/C:/Windows",
            "Documents/plan.pdf",
            "/Music",
            "/Documents/missing.txt",
        ] {
            assert!(resolve(&f, bad).is_err(), "{bad}");
        }
        assert!(resolve(&f, "/Documents/plan.pdf").unwrap().ends_with("plan.pdf"));
        #[cfg(unix)]
        {
            // A link inside the shared folder that points out of it is refused.
            std::os::unix::fs::symlink(dir.path().join("outside.txt"), dir.path().join("docs/escape")).unwrap();
            assert!(resolve(&f, "/Documents/escape").is_err());
            std::os::unix::fs::symlink(dir.path(), dir.path().join("docs/up")).unwrap();
            assert!(list(&f, "/Documents/up").is_err());
        }
    }

    #[test]
    fn listings_are_capped() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_ENTRIES + 5) {
            std::fs::write(dir.path().join(format!("f{i}")), b"").unwrap();
        }
        let f = vec![SharedFolder { name: "Many".into(), path: dir.path().to_string_lossy().into() }];
        let (entries, truncated) = list(&f, "/Many").unwrap();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert!(truncated);
    }
}
