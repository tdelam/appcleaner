use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use crate::bundle::AppBundle;

#[derive(Debug, Clone)]
pub struct FoundFile {
    pub path: PathBuf,
    /// Size in bytes (recursive for directories)
    pub size: u64,
    /// Whether this entry is the .app bundle itself
    pub is_bundle: bool,
}

impl FoundFile {
    fn new(path: PathBuf) -> Self {
        let is_bundle = path.extension().and_then(|e| e.to_str()) == Some("app");
        let size = compute_size(&path);
        Self { path, size, is_bundle }
    }
}

pub struct Scanner {
    home_dir: PathBuf,
}

impl Scanner {
    /// Create a scanner rooted at the current user's home directory.
    ///
    /// # Errors
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        Ok(Self { home_dir })
    }

    /// Scan for all files and directories associated with `bundle`.
    ///
    /// The bundle itself is always the first entry in the returned list.
    /// Remaining entries are sorted by size descending.
    ///
    /// # Errors
    /// Returns an error if the home directory is unavailable (propagated from
    /// construction — in practice infallible once `Scanner::new` succeeds).
    pub fn scan(&self, bundle: &AppBundle) -> Result<Vec<FoundFile>> {
        let mut found = vec![FoundFile::new(bundle.path.clone())];

        // Pre-compute once per scan rather than once per directory entry.
        let terms = MatchTerms::from_bundle(bundle);

        for dir in self.search_dirs() {
            if !dir.exists() {
                continue;
            }
            scan_dir(&dir, &terms, &mut found);
        }

        // Sort by size descending so the largest items appear first (skip index 0 — the bundle)
        found[1..].sort_by_key(|f| std::cmp::Reverse(f.size));
        Ok(found)
    }

    fn search_dirs(&self) -> Vec<PathBuf> {
        let lib = self.home_dir.join("Library");
        vec![
            lib.join("Application Support"),
            lib.join("Caches"),
            lib.join("Preferences"),
            lib.join("Logs"),
            lib.join("Containers"),
            lib.join("Group Containers"),
            lib.join("Cookies"),
            lib.join("Saved Application State"),
            lib.join("WebKit"),
            lib.join("HTTPStorages"),
            PathBuf::from("/Library/Application Support"),
            PathBuf::from("/Library/Caches"),
            PathBuf::from("/Library/Preferences"),
            PathBuf::from("/Library/Logs"),
        ]
    }
}

/// Lowercased bundle terms — and their common prefixes — computed once per
/// scan and reused for every directory entry to avoid per-entry allocations.
struct MatchTerms {
    bundle_id: String,
    app_name: String,
    bundle_id_dot: String,   // e.g. "com.tinyspeck.slackmacgap."
    bundle_id_space: String, // e.g. "com.tinyspeck.slackmacgap "
    app_name_dot: String,    // e.g. "slack."
}

impl MatchTerms {
    fn from_bundle(bundle: &AppBundle) -> Self {
        let bundle_id = bundle.bundle_id.to_lowercase();
        let app_name = bundle.name.to_lowercase();
        let bundle_id_dot = format!("{bundle_id}.");
        let bundle_id_space = format!("{bundle_id} ");
        let app_name_dot = format!("{app_name}.");
        Self { bundle_id, app_name, bundle_id_dot, bundle_id_space, app_name_dot }
    }
}

fn scan_dir(dir: &Path, terms: &MatchTerms, found: &mut Vec<FoundFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return; // skip unreadable directories (e.g. permission denied)
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if is_match(file_name, terms) {
            found.push(FoundFile::new(path));
        }
    }
}

fn is_match(name: &str, terms: &MatchTerms) -> bool {
    let name_lower = name.to_lowercase();

    // Exact match on bundle ID or app name
    if name_lower == terms.bundle_id || name_lower == terms.app_name {
        return true;
    }

    // Preference / cache files: "com.example.app.plist" or "com.example.app "
    if name_lower.starts_with(&terms.bundle_id_dot)
        || name_lower.starts_with(&terms.bundle_id_space)
    {
        return true;
    }

    // App name with extension: "Slack.savedState"
    name_lower.starts_with(&terms.app_name_dot)
}

/// Compute total byte size of `path`.
///
/// Symlinks count as zero and are not traversed: `WalkDir` does not follow
/// links by default, and with `follow_links(false)` its `DirEntry::metadata`
/// uses `symlink_metadata`, so symlinked files are skipped by the `is_file`
/// filter. This avoids double-counting shared storage and prevents traversal
/// cycles on malformed trees.
fn compute_size(path: &Path) -> u64 {
    if path.is_symlink() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bundle(name: &str, bundle_id: &str) -> AppBundle {
        AppBundle {
            path: PathBuf::from(format!("/Applications/{name}.app")),
            name: name.to_string(),
            bundle_id: bundle_id.to_string(),
        }
    }

    fn terms(bundle: &AppBundle) -> MatchTerms {
        MatchTerms::from_bundle(bundle)
    }

    #[test]
    fn matches_exact_bundle_id() {
        let bundle = make_bundle("Slack", "com.tinyspeck.slackmacgap");
        assert!(is_match("com.tinyspeck.slackmacgap", &terms(&bundle)));
    }

    #[test]
    fn matches_plist_with_suffix() {
        let bundle = make_bundle("Slack", "com.tinyspeck.slackmacgap");
        assert!(is_match("com.tinyspeck.slackmacgap.plist", &terms(&bundle)));
    }

    #[test]
    fn matches_app_name_with_extension() {
        let bundle = make_bundle("Slack", "com.tinyspeck.slackmacgap");
        assert!(is_match("Slack.savedState", &terms(&bundle)));
    }

    #[test]
    fn does_not_match_unrelated() {
        let bundle = make_bundle("Slack", "com.tinyspeck.slackmacgap");
        assert!(!is_match("com.apple.Safari", &terms(&bundle)));
    }

    #[test]
    fn requires_boundary_after_bundle_id_prefix() {
        // Matching must require a dot or space boundary after the bundle ID —
        // a bare character-level prefix is not enough. If this regresses, short
        // bundle IDs would start grabbing unrelated apps' files.
        let bundle = make_bundle("Ex", "com.ex");
        assert!(!is_match("com.example", &terms(&bundle)));
        assert!(!is_match("com.example.plist", &terms(&bundle)));
        // Sanity: the intended matches still work with a dot boundary.
        assert!(is_match("com.ex", &terms(&bundle)));
        assert!(is_match("com.ex.plist", &terms(&bundle)));
    }

    // Known limitation: an app with a very short bundle ID (e.g. "com.example") will
    // produce false-positive matches against files belonging to "com.example.other".
    // In practice this is not an issue because real bundle IDs are long and unique.
    // A future improvement could add boundary checks on the suffix.
}
