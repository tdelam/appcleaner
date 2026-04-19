use anyhow::{Context, Result};

use crate::scanner::FoundFile;
use crate::styled_progress_bar;

/// Permanently delete all given files/directories, showing a progress bar.
///
/// # Errors
/// Returns an error listing how many items could not be removed if any
/// deletion fails. Individual errors are printed to stderr before returning.
pub fn delete_files(files: &[FoundFile]) -> Result<()> {
    let pb = styled_progress_bar(files.len() as u64);

    let mut errors: Vec<anyhow::Error> = Vec::new();

    for file in files {
        let label = file
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        pb.set_message(label);

        let result = if file.path.is_dir() && !file.path.is_symlink() {
            std::fs::remove_dir_all(&file.path)
                .with_context(|| format!("failed to remove {}", file.path.display()))
        } else {
            std::fs::remove_file(&file.path)
                .with_context(|| format!("failed to remove {}", file.path.display()))
        };

        if let Err(e) = result {
            errors.push(e);
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    if errors.is_empty() {
        println!("Done. Removed {} item(s).", files.len());
        Ok(())
    } else {
        for e in &errors {
            eprintln!("  error: {e}");
        }
        anyhow::bail!(
            "{} of {} item(s) could not be removed",
            errors.len(),
            files.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn found(path: PathBuf) -> FoundFile {
        FoundFile { size: 0, is_bundle: false, path }
    }

    #[test]
    fn removes_files_and_directories() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("prefs.plist");
        let nested = dir.path().join("cache_dir");
        std::fs::write(&file, "x").unwrap();
        std::fs::create_dir_all(nested.join("inner")).unwrap();
        std::fs::write(nested.join("inner/f"), "y").unwrap();

        delete_files(&[found(file.clone()), found(nested.clone())]).unwrap();

        assert!(!file.exists());
        assert!(!nested.exists());
    }

    #[test]
    fn returns_error_when_any_target_is_missing() {
        let dir = tempdir().unwrap();
        let present = dir.path().join("real.plist");
        std::fs::write(&present, "x").unwrap();
        let missing = dir.path().join("does_not_exist.plist");

        let err = delete_files(&[found(present.clone()), found(missing)]).unwrap_err();
        assert!(err.to_string().contains("1 of 2"));
        assert!(!present.exists(), "successful removals still go through on partial failure");
    }

    #[test]
    fn empty_input_is_a_no_op() {
        delete_files(&[]).unwrap();
    }

    #[test]
    fn does_not_follow_symlinked_directory() {
        // A symlink to a directory should be removed as a symlink (remove_file),
        // not recursed into with remove_dir_all — otherwise we'd delete the target.
        let dir = tempdir().unwrap();
        let target = dir.path().join("keep_me");
        std::fs::create_dir_all(&target).unwrap();
        let sentinel = target.join("sentinel");
        std::fs::write(&sentinel, "important").unwrap();

        let link = dir.path().join("link_to_keep_me");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        delete_files(&[found(link.clone())]).unwrap();

        assert!(!link.exists(), "symlink should be removed");
        assert!(sentinel.exists(), "symlink target must not be touched");
    }
}
