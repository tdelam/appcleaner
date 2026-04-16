use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

use crate::scanner::FoundFile;

const TRASH_DIR_NAME: &str = ".appclean/trash";
const MANIFEST_FILE: &str = "manifest.json";

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
    /// Human-readable label for display (e.g. "Slack  —  2026-04-15 21:00")
    pub fn label(&self) -> String {
        let secs = self.timestamp;
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        // Date components are approximate — good enough for a list label
        format!(
            "{}  —  {} item(s) trashed at {:02}:{:02} UTC (unix: {})",
            self.app_name,
            self.items.len(),
            hours,
            mins,
            self.timestamp
        )
    }
}

/// Move `files` into the trash and return the resulting `TrashEntry`.
pub fn move_to_trash(files: &[FoundFile], app_name: &str) -> Result<TrashEntry> {
    let session_dir = session_dir(app_name)?;
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
        let label = file
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        pb.set_message(label);

        // Reconstruct a relative path to preserve naming inside the session dir.
        // Strip the leading `/` so it can be joined under session_dir.
        let relative = file
            .path
            .strip_prefix("/")
            .unwrap_or(&file.path)
            .to_path_buf();
        let dest = session_dir.join(&relative);

        let result = move_path(&file.path, &dest);
        match result {
            Ok(()) => items.push(TrashItem {
                original_path: file.path.clone(),
                trash_path: dest,
            }),
            Err(e) => errors.push(e),
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("  warning: {e}");
        }
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let entry = TrashEntry {
        app_name: app_name.to_string(),
        timestamp,
        items,
    };

    // Write the manifest so we can restore later
    let manifest_path = session_dir.join(MANIFEST_FILE);
    let json = serde_json::to_string_pretty(&entry)
        .context("failed to serialise trash manifest")?;
    std::fs::write(&manifest_path, json)
        .with_context(|| format!("failed to write manifest to {}", manifest_path.display()))?;

    println!("Moved {} item(s) to trash.", entry.items.len());
    println!("  Restore with: appclean restore");

    Ok(entry)
}

/// List all trash session entries found under `~/.appclean/trash/`.
pub fn list_entries() -> Result<Vec<(PathBuf, TrashEntry)>> {
    let trash_root = trash_root()?;
    if !trash_root.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();

    for dir_entry in std::fs::read_dir(&trash_root)
        .with_context(|| format!("failed to read trash dir {}", trash_root.display()))?
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

    // Sort newest first
    entries.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
    Ok(entries)
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
        let label = item
            .original_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        pb.set_message(label);

        let result = move_path(&item.trash_path, &item.original_path);
        if let Err(e) = result {
            errors.push(e);
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    if errors.is_empty() {
        // Remove the now-empty session directory
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

// ── helpers ──────────────────────────────────────────────────────────────────

fn trash_root() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    Ok(home.join(TRASH_DIR_NAME))
}

fn session_dir(app_name: &str) -> Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let safe_name = app_name.replace(['/', ' ', '.'], "_");
    Ok(trash_root()?.join(format!("{ts}-{safe_name}")))
}

fn move_path(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::rename(src, dest).with_context(|| {
        format!("failed to move {} to {}", src.display(), dest.display())
    })
}
