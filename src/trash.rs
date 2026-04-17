use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::macros::format_description;

use crate::scanner::FoundFile;
use crate::styled_progress_bar;

const TRASH_DIR_NAME: &str = ".appclean/trash";
const MANIFEST_FILE: &str = "manifest.json";

// ── public types ─────────────────────────────────────────────────────────────

/// A record of one removal session stored inside the trash directory.
#[derive(Debug, Serialize, Deserialize)]
pub struct TrashEntry {
    /// Display name of the app that was removed
    pub app_name: String,
    /// Unix timestamp of when it was trashed
    pub timestamp: u64,
    /// Original path → path inside the trash session directory
    pub items: Vec<TrashItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrashItem {
    /// Where the file/directory originally lived
    pub original_path: PathBuf,
    /// Where it now lives under the trash session directory
    pub trash_path: PathBuf,
}

impl TrashEntry {
    /// Human-readable label used in the restore selection list.
    #[must_use]
    pub fn label(&self) -> String {
        let fmt = format_description!("[year]-[month]-[day] [hour]:[minute] UTC");
        let date = i64::try_from(self.timestamp)
            .ok()
            .and_then(|ts| OffsetDateTime::from_unix_timestamp(ts).ok())
            .and_then(|dt| dt.format(fmt).ok())
            .unwrap_or_else(|| self.timestamp.to_string());

        format!(
            "{}  —  {} item(s) trashed on {}",
            self.app_name,
            self.items.len(),
            date,
        )
    }
}

// ── TrashStore ────────────────────────────────────────────────────────────────

/// Manages the on-disk trash directory.
/// Keeping the root path in a struct makes the logic testable with a temp dir.
pub struct TrashStore {
    root: PathBuf,
}

impl TrashStore {
    /// Create a store rooted at `~/.appclean/trash/`.
    ///
    /// # Errors
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        Ok(Self { root: home.join(TRASH_DIR_NAME) })
    }

    /// Move `files` into the trash and return the resulting [`TrashEntry`].
    ///
    /// # Errors
    /// Returns an error if the trash session directory cannot be created or if
    /// the manifest cannot be written. Individual file move errors are printed
    /// as warnings but do not abort the operation.
    pub fn move_to_trash(&self, files: &[FoundFile], app_name: &str) -> Result<TrashEntry> {
        let session_dir = self.create_session_dir(app_name)?;

        let pb = styled_progress_bar(files.len() as u64);

        let mut items = Vec::new();
        let mut errors: Vec<anyhow::Error> = Vec::new();

        for file in files {
            pb.set_message(
                file.path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string(),
            );

            // Rebuild the path without the root component so it can be joined
            // under session_dir. `Path::join` treats absolute paths as replacements,
            // so we must strip the leading root (e.g. `/` on unix) explicitly.
            let relative: PathBuf = file
                .path
                .components()
                .filter(|c| !matches!(c, Component::RootDir | Component::Prefix(_)))
                .collect();
            let dest = session_dir.join(&relative);

            match move_path(&file.path, &dest) {
                Ok(()) => items.push(TrashItem {
                    original_path: file.path.clone(),
                    trash_path: dest,
                }),
                Err(e) => errors.push(e),
            }

            pb.inc(1);
        }

        pb.finish_and_clear();

        for e in &errors {
            eprintln!("  warning: {e}");
        }

        let timestamp = now_secs();
        let entry = TrashEntry { app_name: app_name.to_string(), timestamp, items };

        let manifest_path = session_dir.join(MANIFEST_FILE);
        if let Err(e) = write_manifest(&entry, &manifest_path) {
            // Files have already been moved into the session dir. Without a
            // manifest, `apc restore` can't see them. Print a recovery trail
            // so the user can manually put them back.
            if !entry.items.is_empty() {
                eprintln!(
                    "error: manifest write failed — {} file(s) are in the trash but untracked.",
                    entry.items.len()
                );
                eprintln!("  Recover by moving these back manually:");
                for item in &entry.items {
                    eprintln!(
                        "    {} → {}",
                        item.trash_path.display(),
                        item.original_path.display(),
                    );
                }
            }
            return Err(e);
        }

        if errors.is_empty() {
            println!("Moved {} item(s) to trash.", entry.items.len());
        } else {
            println!(
                "Moved {}/{} item(s) to trash; {} could not be moved (see warnings above).",
                entry.items.len(),
                files.len(),
                errors.len(),
            );
        }
        println!("  Restore with: apc restore");

        Ok(entry)
    }

    /// List all trash sessions found under this store's root, newest first.
    ///
    /// # Errors
    /// Returns an error if the trash directory cannot be read or a manifest
    /// cannot be parsed.
    pub fn list_entries(&self) -> Result<Vec<(PathBuf, TrashEntry)>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();

        for dir_entry in std::fs::read_dir(&self.root)
            .with_context(|| format!("failed to read trash dir {}", self.root.display()))?
            .filter_map(Result::ok)
        {
            let manifest = dir_entry.path().join(MANIFEST_FILE);
            if !manifest.exists() {
                continue;
            }

            let json = std::fs::read_to_string(&manifest)
                .with_context(|| format!("failed to read {}", manifest.display()))?;
            let entry: TrashEntry = serde_json::from_str(&json)
                .with_context(|| format!("failed to parse {}", manifest.display()))?;

            entries.push((dir_entry.path(), entry));
        }

        entries.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
        Ok(entries)
    }

    /// Permanently delete trash sessions, optionally filtering to those older
    /// than `older_than_days` days. Returns the number of sessions removed.
    ///
    /// # Errors
    /// Returns an error if any session directory cannot be removed.
    pub fn empty_trash(&self, older_than_days: Option<u64>) -> Result<usize> {
        let entries = self.list_entries()?;

        if entries.is_empty() {
            return Ok(0);
        }

        let cutoff_secs = older_than_days.map(|d| now_secs().saturating_sub(d * 86_400));

        let to_delete: Vec<_> = entries
            .into_iter()
            .filter(|(_, entry)| cutoff_secs.is_none_or(|cutoff| entry.timestamp <= cutoff))
            .collect();

        if to_delete.is_empty() {
            return Ok(0);
        }

        for (session_path, _) in &to_delete {
            std::fs::remove_dir_all(session_path)
                .with_context(|| format!("failed to remove {}", session_path.display()))?;
        }

        Ok(to_delete.len())
    }

    /// Create a fresh, unique session directory under `self.root`.
    ///
    /// Two `apc` processes trashing the same app within a one-second window
    /// would otherwise generate identical `{timestamp}-{name}` paths and
    /// silently merge into the same session (since `create_dir_all` is
    /// idempotent). We use non-recursive `create_dir` and retry with an
    /// incrementing suffix so concurrent invocations each get their own dir.
    fn create_session_dir(&self, app_name: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create trash root {}", self.root.display()))?;

        let safe_name = app_name.replace(['/', ' ', '.'], "_");
        let base = format!("{}-{safe_name}", now_secs());

        for suffix in 0u32..1000 {
            let name = if suffix == 0 {
                base.clone()
            } else {
                format!("{base}-{suffix}")
            };
            let candidate = self.root.join(&name);
            match std::fs::create_dir(&candidate) {
                Ok(()) => return Ok(candidate),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(e).with_context(|| {
                        format!("failed to create trash dir {}", candidate.display())
                    });
                }
            }
        }

        anyhow::bail!(
            "could not find a unique trash session name under {} after 1000 attempts",
            self.root.display()
        )
    }
}

/// Move all items in `entry` back to their original locations.
///
/// On partial failure, the session manifest is rewritten to contain only the
/// items that still need restoring, so a subsequent `apc restore` retries just
/// those. Successfully restored items are not rolled back.
///
/// # Errors
/// Returns an error if any item cannot be moved back.
pub fn restore(session_path: &Path, entry: &TrashEntry) -> Result<()> {
    let pb = styled_progress_bar(entry.items.len() as u64);

    let mut errors: Vec<anyhow::Error> = Vec::new();
    let mut still_pending: Vec<TrashItem> = Vec::new();

    for item in &entry.items {
        pb.set_message(
            item.original_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string(),
        );

        if let Err(e) = move_path(&item.trash_path, &item.original_path) {
            errors.push(e);
            still_pending.push(TrashItem {
                original_path: item.original_path.clone(),
                trash_path: item.trash_path.clone(),
            });
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    if errors.is_empty() {
        if let Err(e) = std::fs::remove_dir_all(session_path) {
            eprintln!("warning: could not remove trash session directory: {e}");
        }
        println!("Restored {} item(s).", entry.items.len());
        Ok(())
    } else {
        for e in &errors {
            eprintln!("  error: {e}");
        }

        // Rewrite the manifest to list only the items still in the trash so
        // that a retry doesn't try to re-move files that were already restored.
        let remaining = TrashEntry {
            app_name: entry.app_name.clone(),
            timestamp: entry.timestamp,
            items: still_pending,
        };
        let manifest_path = session_path.join(MANIFEST_FILE);
        if let Err(e) = write_manifest(&remaining, &manifest_path) {
            eprintln!("warning: {e}");
        }

        anyhow::bail!(
            "{} of {} item(s) could not be restored (retry with: apc restore)",
            errors.len(),
            entry.items.len()
        )
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_secs()
}

fn move_path(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::rename(src, dest)
        .with_context(|| format!("failed to move {} to {}", src.display(), dest.display()))
}

fn write_manifest(entry: &TrashEntry, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(entry)
        .context("failed to serialise trash manifest")?;
    std::fs::write(path, json)
        .with_context(|| format!("failed to write manifest to {}", path.display()))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::FoundFile;
    use tempfile::tempdir;

    fn make_store(root: PathBuf) -> TrashStore {
        TrashStore { root }
    }

    fn make_found_file(path: PathBuf) -> FoundFile {
        FoundFile { size: 0, is_bundle: false, path }
    }

    #[test]
    fn move_to_trash_removes_original_and_writes_manifest() {
        let src_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();

        let file = src_dir.path().join("test.plist");
        std::fs::write(&file, "data").unwrap();
        assert!(file.exists());

        let store = make_store(trash_dir.path().to_path_buf());
        let entry = store
            .move_to_trash(&[make_found_file(file.clone())], "TestApp")
            .unwrap();

        assert!(!file.exists(), "original file should have been moved");
        assert_eq!(entry.items.len(), 1);
        assert_eq!(entry.app_name, "TestApp");

        // Manifest should exist in the session directory
        let (session_path, _) = store.list_entries().unwrap().into_iter().next().unwrap();
        assert!(session_path.join(MANIFEST_FILE).exists());
    }

    #[test]
    fn list_entries_returns_sessions_newest_first() {
        let src_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let store = make_store(trash_dir.path().to_path_buf());

        // Trash two separate files so we get two sessions
        for name in ["alpha.plist", "beta.plist"] {
            let file = src_dir.path().join(name);
            std::fs::write(&file, "x").unwrap();
            store.move_to_trash(&[make_found_file(file)], name).unwrap();
        }

        let entries = store.list_entries().unwrap();
        assert_eq!(entries.len(), 2);
        // Newest first — timestamps should be non-increasing
        assert!(entries[0].1.timestamp >= entries[1].1.timestamp);
    }

    #[test]
    fn restore_moves_files_back_and_removes_session() {
        let src_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();

        let file = src_dir.path().join("prefs.plist");
        std::fs::write(&file, "data").unwrap();

        let store = make_store(trash_dir.path().to_path_buf());
        store
            .move_to_trash(&[make_found_file(file.clone())], "TestApp")
            .unwrap();
        assert!(!file.exists());

        let entries = store.list_entries().unwrap();
        let (session_path, entry) = &entries[0];
        restore(session_path, entry).unwrap();

        assert!(file.exists(), "file should be restored to original location");
        assert!(!session_path.exists(), "session dir should be cleaned up after restore");
    }

    #[test]
    fn list_entries_empty_when_no_trash_dir() {
        let store = make_store(PathBuf::from("/tmp/appclean-nonexistent-test-dir"));
        assert!(store.list_entries().unwrap().is_empty());
    }

    #[test]
    fn empty_trash_removes_all_sessions() {
        let src_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let store = make_store(trash_dir.path().to_path_buf());

        for name in ["app1.plist", "app2.plist"] {
            let file = src_dir.path().join(name);
            std::fs::write(&file, "x").unwrap();
            store.move_to_trash(&[make_found_file(file)], name).unwrap();
        }

        assert_eq!(store.list_entries().unwrap().len(), 2);
        let removed = store.empty_trash(None).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(store.list_entries().unwrap().len(), 0);
    }

    #[test]
    fn empty_trash_older_than_keeps_recent_sessions() {
        let src_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let store = make_store(trash_dir.path().to_path_buf());

        // Manually insert a session with an old timestamp
        let old_session = trash_dir.path().join("old_session");
        std::fs::create_dir_all(&old_session).unwrap();
        let old_entry = TrashEntry {
            app_name: "OldApp".to_string(),
            timestamp: 1_000_000, // very old unix timestamp
            items: vec![],
        };
        std::fs::write(
            old_session.join(MANIFEST_FILE),
            serde_json::to_string(&old_entry).unwrap(),
        ).unwrap();

        // Add a recent session via the normal flow
        let file = src_dir.path().join("recent.plist");
        std::fs::write(&file, "x").unwrap();
        store.move_to_trash(&[make_found_file(file)], "RecentApp").unwrap();

        assert_eq!(store.list_entries().unwrap().len(), 2);

        // Empty sessions older than 30 days — should only remove the old one
        let removed = store.empty_trash(Some(30)).unwrap();
        assert_eq!(removed, 1);

        let remaining = store.list_entries().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].1.app_name, "RecentApp");
    }

    #[test]
    fn empty_trash_on_empty_store_is_a_no_op() {
        let trash_dir = tempdir().unwrap();
        let store = make_store(trash_dir.path().to_path_buf());
        let removed = store.empty_trash(None).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn auto_purge_does_not_remove_recent_sessions() {
        let src_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let store = make_store(trash_dir.path().to_path_buf());

        // Recent session — should survive a 30-day purge
        let file = src_dir.path().join("recent.plist");
        std::fs::write(&file, "x").unwrap();
        store.move_to_trash(&[make_found_file(file)], "RecentApp").unwrap();

        let removed = store.empty_trash(Some(30)).unwrap();
        assert_eq!(removed, 0, "recent session should not be auto-purged");
        assert_eq!(store.list_entries().unwrap().len(), 1);
    }

    #[test]
    fn label_contains_app_name_and_item_count() {
        let entry = TrashEntry {
            app_name: "Slack".to_string(),
            timestamp: 1_000_000,
            items: vec![
                TrashItem {
                    original_path: PathBuf::from("/tmp/a"),
                    trash_path: PathBuf::from("/tmp/trash/a"),
                },
            ],
        };
        let label = entry.label();
        assert!(label.contains("Slack"));
        assert!(label.contains("1 item(s)"));
    }

    #[test]
    fn restore_partial_failure_rewrites_manifest_to_pending_only() {
        // Two files are trashed. Before restoring, we pre-create one of the
        // original paths as a directory so that `rename` fails for that item
        // — simulating a partial restore. The manifest should then list only
        // the still-pending item, letting a retry handle it cleanly.
        let src_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let store = make_store(trash_dir.path().to_path_buf());

        let good = src_dir.path().join("good.plist");
        let bad = src_dir.path().join("bad.plist");
        std::fs::write(&good, "g").unwrap();
        std::fs::write(&bad, "b").unwrap();

        store
            .move_to_trash(
                &[make_found_file(good.clone()), make_found_file(bad.clone())],
                "TestApp",
            )
            .unwrap();

        // Block `bad`'s restore: create a non-empty directory at its original
        // path so `fs::rename(trash_path, original_path)` fails with ENOTEMPTY.
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("blocker"), "x").unwrap();

        let (session_path, entry) = store.list_entries().unwrap().into_iter().next().unwrap();
        let err = restore(&session_path, &entry).unwrap_err();
        assert!(err.to_string().contains("1 of 2"));

        // `good` should be restored; `bad` remains in trash
        assert!(good.exists() && good.is_file());
        assert!(session_path.exists(), "session dir kept on partial failure");

        // Manifest should now list only the 1 still-pending item
        let rewritten = store.list_entries().unwrap();
        assert_eq!(rewritten.len(), 1);
        assert_eq!(rewritten[0].1.items.len(), 1);
        assert_eq!(rewritten[0].1.items[0].original_path, bad);
    }

    #[test]
    fn create_session_dir_avoids_collision_within_same_second() {
        // Back-to-back calls for the same app within the same second must
        // produce distinct directories — the naive `{timestamp}-{name}` scheme
        // would collide and silently merge via `create_dir_all`.
        let trash_dir = tempdir().unwrap();
        let store = make_store(trash_dir.path().to_path_buf());

        let a = store.create_session_dir("TestApp").unwrap();
        let b = store.create_session_dir("TestApp").unwrap();
        let c = store.create_session_dir("TestApp").unwrap();

        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        assert!(a.is_dir() && b.is_dir() && c.is_dir());
    }

    #[test]
    fn write_manifest_returns_contextualised_error_when_parent_missing() {
        // Writing to a path whose parent directory doesn't exist must surface
        // a real error (not panic) so callers can react — in move_to_trash
        // this triggers the recovery-trail stderr dump.
        let entry = TrashEntry {
            app_name: "X".into(),
            timestamp: 0,
            items: vec![],
        };
        let bad_path = PathBuf::from("/nonexistent-dir-appclean-test/manifest.json");
        let err = write_manifest(&entry, &bad_path).unwrap_err();
        assert!(err.to_string().contains("failed to write manifest"));
    }
}
