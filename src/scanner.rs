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
        FoundFile { path, size, is_bundle }
    }
}

pub struct Scanner {
    home_dir: PathBuf,
}

impl Scanner {
    pub fn new() -> Result<Self> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        Ok(Scanner { home_dir })
    }

    /// Scan for all files and directories associated with `bundle`.
    /// The bundle itself is always the first entry in the returned list.
    pub fn scan(&self, bundle: &AppBundle) -> Result<Vec<FoundFile>> {
        let mut found = vec![FoundFile::new(bundle.path.clone())];

        // Pre-compute once per scan rather than once per directory entry.
        let terms = MatchTerms::from_bundle(bundle);

        for dir in self.search_dirs() {
            if !dir.exists() {
                continue;
            }
            self.scan_dir(&dir, &terms, &mut found);
        }

        // Sort by size descending so the largest items appear first (skip index 0 — the bundle)
        found[1..].sort_by(|a, b| b.size.cmp(&a.size));
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

    fn scan_dir(&self, dir: &Path, terms: &MatchTerms, found: &mut Vec<FoundFile>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return, // skip unreadable directories (e.g. permission denied)
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            if is_match(&file_name, terms) {
                found.push(FoundFile::new(path));
            }
        }
    }
}

/// Lowercased bundle terms computed once per scan and reused for every entry.
struct MatchTerms {
    bundle_id: String,
    app_name: String,
}

impl MatchTerms {
    fn from_bundle(bundle: &AppBundle) -> Self {
        MatchTerms {
            bundle_id: bundle.bundle_id.to_lowercase(),
            app_name: bundle.name.to_lowercase(),
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
    if name_lower.starts_with(&format!("{}.", terms.bundle_id))
        || name_lower.starts_with(&format!("{} ", terms.bundle_id))
    {
        return true;
    }

    // App name with extension: "Slack.savedState"
    if name_lower.starts_with(&format!("{}.", terms.app_name)) {
        return true;
    }

    false
}

fn compute_size(path: &Path) -> u64 {
    if path.is_symlink() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
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

    // Known limitation: an app with a very short bundle ID (e.g. "com.example") will
    // produce false-positive matches against files belonging to "com.example.other".
    // In practice this is not an issue because real bundle IDs are long and unique.
    // A future improvement could add boundary checks on the suffix.
}
