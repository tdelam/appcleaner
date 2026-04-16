use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

use crate::scanner::FoundFile;

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
    pub fn label(&self) -> String {
        let secs = self.timestamp;
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        format!(
            "{}  —  {} item(s) trashed at {:02}:{:02} UTC (unix: {})",
            self.app_name,
            self.items.len(),
            hours,
            mins,
            self.timestamp,
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
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        Ok(TrashStore { root: home.join(TRASH_DIR_NAME) })
    }

    /// Move `files` into the trash and return the resulting [`TrashEntry`].
    pub fn move_to_trash(&self, files: &[FoundFile], app_name: &str) -> Result<TrashEntry> {
        let session_dir = self.session_dir(app_name);
        std::fs::create_dir_all(&session_dir)
            .with_context(|| format!("failed to create trash dir {}", session_dir.display()))?;

        let pb = ProgressBar::new(files.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")?
                .progress_chars("=>-"),
        );

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

            // Strip the leading `/` so the path can be joined under session_dir.
            let relative = file.path.strip_prefix("/").unwrap_or(&file.path).to_path_buf();
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
        let json = serde_json::to_string_pretty(&entry)
            .context("failed to serialise trash manifest")?;
        std::fs::write(&manifest_path, json)
            .with_context(|| format!("failed to write manifest to {}", manifest_path.display()))?;

        println!("Moved {} item(s) to trash.", entry.items.len());
        println!("  Restore with: appclean restore");

        Ok(entry)
    }

    /// List all trash sessions found under this store's root, newest first.
    pub fn list_entries(&self) -> Result<Vec<(PathBuf, TrashEntry)>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();

        for dir_entry in std::fs::read_dir(&self.root)
            .with_context(|| format!("failed to read trash dir {}", self.root.display()))?
            .filter_map(|e| e.ok())
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

    fn session_dir(&self, app_name: &str) -> PathBuf {
        let safe_name = app_name.replace(['/', ' ', '.'], "_");
        self.root.join(format!("{}-{safe_name}", now_secs()))
    }
}

/// Move all items in `entry` back to their original locations.
pub fn restore(session_path: &Path, entry: &TrashEntry) -> Result<()> {
    let pb = ProgressBar::new(entry.items.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")?
            .progress_chars("=>-"),
    );

    let mut errors: Vec<anyhow::Error> = Vec::new();

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
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    if errors.is_empty() {
        let _ = std::fs::remove_dir_all(session_path);
        println!("Restored {} item(s).", entry.items.len());
    } else {
        eprintln!("{} error(s) during restore:", errors.len());
        for e in &errors {
            eprintln!("  {e}");
        }
    }

    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
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
}
