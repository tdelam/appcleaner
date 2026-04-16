use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct AppBundle {
    pub path: PathBuf,
    /// Display name (CFBundleName or stem of the .app filename)
    pub name: String,
    /// Reverse-DNS identifier, e.g. "com.tinyspeck.slackmacgap"
    pub bundle_id: String,
}

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("not a .app bundle: {0}")]
    NotABundle(PathBuf),
    #[error("missing Contents/Info.plist in {0}")]
    MissingPlist(PathBuf),
}

#[derive(Debug, Deserialize)]
struct InfoPlist {
    #[serde(rename = "CFBundleIdentifier")]
    bundle_identifier: String,
    #[serde(rename = "CFBundleName")]
    bundle_name: Option<String>,
}

impl AppBundle {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| path.as_ref().to_path_buf());

        if path.extension().and_then(|e| e.to_str()) != Some("app") {
            return Err(BundleError::NotABundle(path).into());
        }

        let plist_path = path.join("Contents/Info.plist");
        if !plist_path.exists() {
            return Err(BundleError::MissingPlist(path).into());
        }

        let info: InfoPlist = plist::from_file(&plist_path)
            .with_context(|| format!("failed to parse {}", plist_path.display()))?;

        let name = info.bundle_name.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string()
        });

        Ok(AppBundle {
            path,
            name,
            bundle_id: info.bundle_identifier,
        })
    }
}

impl std::fmt::Display for AppBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.bundle_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_app_path() {
        // Use a path that is guaranteed not to be a .app bundle
        let err = AppBundle::from_path("/tmp/not-an-app.dmg").unwrap_err();
        assert!(err.to_string().contains("not a .app bundle"));
    }

    #[test]
    fn display_includes_name_and_bundle_id() {
        let bundle = AppBundle {
            path: "/Applications/Slack.app".into(),
            name: "Slack".to_string(),
            bundle_id: "com.tinyspeck.slackmacgap".to_string(),
        };
        assert_eq!(bundle.to_string(), "Slack (com.tinyspeck.slackmacgap)");
    }
}
