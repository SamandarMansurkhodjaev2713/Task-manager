//! Scheduled SQLite hot-backup via `VACUUM INTO`.
//!
//! # Why `VACUUM INTO` rather than a file copy?
//!
//! A plain `cp data/app.db data/app.db.bak` produces a corrupt backup if the
//! WAL journal is active.  SQLite's `VACUUM INTO 'path'` writes an
//! atomically-consistent, fully-checkpointed copy of the database at `path`
//! without blocking normal reads or writes.  The resulting file is a valid
//! standalone SQLite database with no WAL journal.
//!
//! # File naming and rotation
//!
//! Backup files are named `app-YYYY-MM-DD-HH.db` (UTC timestamp at run time).
//! After each backup, the job scans the backup directory for files matching
//! `app-*.db` and deletes the oldest ones if the count exceeds
//! `max_backup_files`.  Files not matching the pattern are left untouched so
//! that manually placed backups survive rotation.
//!
//! # Failure policy
//!
//! Every error (filesystem, SQL, metadata) is logged as `error` / `warn` and
//! silently swallowed so a backup failure never kills the main process or
//! interrupts the UX.

use std::path::{Path, PathBuf};

use chrono::Utc;
use sqlx::SqlitePool;

/// Runs one backup cycle: execute `VACUUM INTO`, then rotate old files.
///
/// `backup_dir` must already exist (the job does not create it).
/// `max_files` is the inclusive upper bound on the number of `app-*.db`
/// files; if the count after the new backup exceeds it, the oldest are
/// deleted.
pub async fn run_backup_cycle(pool: &SqlitePool, backup_dir: &str, max_files: u32) {
    // Derive a timestamped path: data/backups/app-2026-05-05-14.db
    let timestamp = Utc::now().format("%Y-%m-%d-%H").to_string();
    let backup_path = PathBuf::from(backup_dir).join(format!("app-{timestamp}.db"));

    // Ensure the backup directory exists — create it if missing so operators
    // do not have to pre-create it before enabling the feature.
    if let Err(err) = std::fs::create_dir_all(backup_dir) {
        tracing::error!(
            backup_dir,
            error = %err,
            "sqlite_backup: cannot create backup directory; skipping this cycle"
        );
        return;
    }

    // Execute VACUUM INTO — this is the actual backup step.
    let path_str = backup_path.to_string_lossy().to_string();
    let result =
        sqlx::query("VACUUM INTO ?").bind(&path_str).execute(pool).await;

    match result {
        Ok(_) => {
            tracing::info!(path = path_str, "sqlite_backup: backup written successfully");
        }
        Err(err) => {
            tracing::error!(
                path = path_str,
                error = %err,
                "sqlite_backup: VACUUM INTO failed; backup may be incomplete or absent"
            );
            return; // Skip rotation — nothing new was written.
        }
    }

    // Rotation: collect app-*.db files sorted by name (lexicographic =
    // chronological because we use ISO-8601 timestamps).
    rotate_backups(backup_dir, max_files);
}

fn rotate_backups(backup_dir: &str, max_files: u32) {
    let dir = Path::new(backup_dir);
    let mut entries: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("app-") && n.ends_with(".db"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(err) => {
            tracing::warn!(
                backup_dir,
                error = %err,
                "sqlite_backup: cannot read backup directory for rotation; skipping"
            );
            return;
        }
    };

    entries.sort(); // lexicographic ≡ chronological for our filename pattern

    let excess = (entries.len() as i64) - (max_files as i64);
    if excess <= 0 {
        return; // Nothing to delete.
    }

    for old_path in entries.iter().take(excess as usize) {
        match std::fs::remove_file(old_path) {
            Ok(()) => {
                tracing::info!(
                    path = %old_path.display(),
                    "sqlite_backup: rotated (deleted) old backup file"
                );
            }
            Err(err) => {
                tracing::warn!(
                    path = %old_path.display(),
                    error = %err,
                    "sqlite_backup: could not delete old backup file; manual cleanup may be needed"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rotate_backups;
    use std::fs;
    use tempfile::TempDir;

    fn create_backup_files(dir: &TempDir, names: &[&str]) {
        for name in names {
            fs::write(dir.path().join(name), b"fake_sqlite_backup").unwrap();
        }
    }

    fn list_backup_files(dir: &TempDir) -> Vec<String> {
        let mut files: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| {
                e.ok().and_then(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("app-") && name.ends_with(".db") {
                        Some(name)
                    } else {
                        None
                    }
                })
            })
            .collect();
        files.sort();
        files
    }

    #[test]
    fn given_fewer_files_than_max_when_rotate_then_nothing_deleted() {
        let dir = TempDir::new().unwrap();
        create_backup_files(
            &dir,
            &[
                "app-2026-05-01-06.db",
                "app-2026-05-01-12.db",
                "app-2026-05-01-18.db",
            ],
        );

        rotate_backups(dir.path().to_str().unwrap(), 14);

        assert_eq!(list_backup_files(&dir).len(), 3);
    }

    #[test]
    fn given_files_exceeding_max_when_rotate_then_oldest_deleted() {
        let dir = TempDir::new().unwrap();
        create_backup_files(
            &dir,
            &[
                "app-2026-05-01-06.db",
                "app-2026-05-01-12.db",
                "app-2026-05-02-06.db",
                "app-2026-05-02-12.db",
                "app-2026-05-03-06.db",
            ],
        );

        // max = 3 → should keep the 3 newest and delete the 2 oldest
        rotate_backups(dir.path().to_str().unwrap(), 3);

        let remaining = list_backup_files(&dir);
        assert_eq!(remaining.len(), 3, "expected exactly 3 files, got {remaining:?}");
        assert!(
            !remaining.contains(&"app-2026-05-01-06.db".to_owned()),
            "oldest file must be deleted"
        );
        assert!(
            !remaining.contains(&"app-2026-05-01-12.db".to_owned()),
            "second oldest file must be deleted"
        );
        assert!(
            remaining.contains(&"app-2026-05-03-06.db".to_owned()),
            "newest file must be retained"
        );
    }

    #[test]
    fn given_non_matching_files_when_rotate_then_they_are_ignored() {
        let dir = TempDir::new().unwrap();
        // A manually-placed backup that doesn't match our pattern
        fs::write(dir.path().join("manual-backup.db"), b"manual").unwrap();
        create_backup_files(
            &dir,
            &[
                "app-2026-05-01-06.db",
                "app-2026-05-01-12.db",
                "app-2026-05-02-06.db",
            ],
        );

        // max = 2 → oldest auto-backup should go, manual file must survive
        rotate_backups(dir.path().to_str().unwrap(), 2);

        let remaining = list_backup_files(&dir);
        assert_eq!(remaining.len(), 2, "auto-backup files remaining: {remaining:?}");
        // Manual file is not in the auto-rotation list
        assert!(
            dir.path().join("manual-backup.db").exists(),
            "manually placed backup must not be deleted by rotation"
        );
    }
}
