//! Folder layout of a local backup.
//!
//! ```text
//! SignalBackups/                         (name may differ)
//! ├── files/00 … ff/<media name>         shared, encrypted media
//! └── signal-backup-YYYY-MM-DD-HH-mm-ss/ one snapshot
//!     ├── main                           encrypted message backup
//!     ├── metadata                       LocalBackup.proto Metadata
//!     └── files                          media names used by this snapshot
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

pub const ROOT_DIR_NAME: &str = "SignalBackups";
pub const SNAPSHOT_PREFIX: &str = "signal-backup-";
pub const MAIN_FILE: &str = "main";
pub const METADATA_FILE: &str = "metadata";
pub const FILES_LIST: &str = "files";
pub const FILES_DIR: &str = "files";

/// A located snapshot plus the shared media directory.
#[derive(Debug, Clone)]
pub struct BackupLocation {
    /// Directory that holds `files/` and the snapshot directories.
    pub root: PathBuf,
    /// The snapshot directory that will be read.
    pub snapshot: PathBuf,
    /// All complete snapshots found, newest first (directory names).
    pub snapshots: Vec<String>,
}

impl BackupLocation {
    pub fn main_path(&self) -> PathBuf {
        self.snapshot.join(MAIN_FILE)
    }

    pub fn metadata_path(&self) -> PathBuf {
        self.snapshot.join(METADATA_FILE)
    }

    pub fn files_list_path(&self) -> PathBuf {
        self.snapshot.join(FILES_LIST)
    }

    /// Path of an encrypted media file, sharded by the first two hex chars.
    pub fn media_path(&self, media_name: &str) -> PathBuf {
        let shard = media_name.get(..2).unwrap_or("00");
        self.root.join(FILES_DIR).join(shard).join(media_name)
    }

    pub fn snapshot_name(&self) -> String {
        self.snapshot
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Finds the backup from a user-selected path: the backup root, its
/// parent folder, or a single snapshot directory. The root is detected
/// by structure, not by name, because users may rename it.
pub fn locate(path: &Path) -> Result<BackupLocation> {
    if is_snapshot(path) {
        let root = path.parent().unwrap_or(path).to_path_buf();
        let snapshots = list_snapshots(&root)?;
        return Ok(BackupLocation {
            root,
            snapshot: path.to_path_buf(),
            snapshots,
        });
    }
    let snapshots = list_snapshots(path)?;
    if let Some(newest) = snapshots.first() {
        return Ok(BackupLocation {
            root: path.to_path_buf(),
            snapshot: path.join(newest),
            snapshots,
        });
    }
    let nested = path.join(ROOT_DIR_NAME);
    if nested.is_dir() {
        return locate(&nested);
    }
    Err(Error::NotABackup(path.to_path_buf()))
}

fn is_snapshot(dir: &Path) -> bool {
    dir.join(MAIN_FILE).is_file() && dir.join(METADATA_FILE).is_file()
}

/// Complete snapshot directories, newest first. In-progress snapshots
/// (temporary suffix, missing files) are skipped.
pub fn list_snapshots(root: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::NotABackup(root.to_path_buf()));
        }
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(SNAPSHOT_PREFIX) && !name.ends_with("tmp") && is_snapshot(&entry.path())
        {
            names.push(name);
        }
    }
    // Names embed a UTC timestamp (yyyy-MM-dd-HH-mm-ss), so they sort by time.
    names.sort_unstable_by(|a, b| b.cmp(a));
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(root: &Path, name: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MAIN_FILE), b"x").unwrap();
        fs::write(dir.join(METADATA_FILE), b"x").unwrap();
    }

    #[test]
    fn picks_newest_complete_snapshot_from_any_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("RenamedBackups");
        fs::create_dir_all(root.join(FILES_DIR)).unwrap();
        snapshot(&root, "signal-backup-2026-01-01-10-00-00");
        snapshot(&root, "signal-backup-2026-02-01-10-00-00");
        snapshot(&root, "signal-backup-2026-03-01-10-00-00-tmp");
        fs::create_dir_all(root.join("signal-backup-2026-04-01-10-00-00")).unwrap();

        let loc = locate(&root).unwrap();
        assert_eq!(loc.snapshot_name(), "signal-backup-2026-02-01-10-00-00");
        assert_eq!(loc.snapshots.len(), 2);

        let loc = locate(&root.join("signal-backup-2026-01-01-10-00-00")).unwrap();
        assert_eq!(loc.root, root);
        assert_eq!(loc.snapshot_name(), "signal-backup-2026-01-01-10-00-00");
    }

    #[test]
    fn finds_nested_default_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(ROOT_DIR_NAME);
        snapshot(&root, "signal-backup-2026-01-01-10-00-00");
        assert_eq!(locate(tmp.path()).unwrap().root, root);
    }

    #[test]
    fn media_path_is_sharded() {
        let loc = BackupLocation {
            root: PathBuf::from("r"),
            snapshot: PathBuf::from("r/s"),
            snapshots: vec![],
        };
        assert_eq!(loc.media_path("ab12"), Path::new("r/files/ab/ab12"));
    }

    #[test]
    fn rejects_unrelated_folder() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(locate(tmp.path()), Err(Error::NotABackup(_))));
    }
}
