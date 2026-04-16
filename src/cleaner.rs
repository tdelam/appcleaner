use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use crate::scanner::FoundFile;

/// Delete all given files/directories, showing a progress bar.
/// Returns Ok even if some deletions fail — errors are printed and counted.
pub fn delete_files(files: &[FoundFile]) -> Result<()> {
    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")?
            .progress_chars("=>-"),
    );

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
