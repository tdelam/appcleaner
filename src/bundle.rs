use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct AppBundle {
    pub path: PathBuf,
    /// Display name (`CFBundleName` or stem of the .app filename)
    pub name: String,
    /// Reverse-DNS identifier, e.g. "com.tinyspeck.slackmacgap"
    pub bundle_id: String,
}

#[derive(Debug, Deserialize)]
struct InfoPlist {
    #[serde(rename = "CFBundleIdentifier")]
    bundle_identifier: String,
    #[serde(rename = "CFBundleName")]
    bundle_name: Option<String>,
}

impl AppBundle {
    /// Parse an `.app` bundle at `path` and extract its name and bundle ID.
    ///
    /// # Errors
    /// Returns an error if the path is not a `.app` bundle, if `Info.plist` is
    /// missing, or if the plist cannot be parsed.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| path.as_ref().to_path_buf());

        if path.extension().and_then(|e| e.to_str()) != Some("app") {
            return Err(anyhow!("not a .app bundle: {}", path.display()));
        }

        let plist_path = path.join("Contents/Info.plist");
        if !plist_path.exists() {
            return Err(anyhow!("missing Contents/Info.plist in {}", path.display()));
        }

        let info: InfoPlist = plist::from_file(&plist_path)
            .with_context(|| format!("failed to parse {}", plist_path.display()))?;

        let name = info.bundle_name.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string()
        });

        Ok(Self {
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
