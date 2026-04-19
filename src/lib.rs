//! appclean — macOS application cleaner.
//!
//! Removes an `.app` bundle and all its associated files (preferences, caches,
//! containers, logs, etc.) from the standard macOS library locations.
//! Removed files are moved to a recoverable trash by default.

pub mod bundle;
pub mod cleaner;
pub mod scanner;
pub mod trash;
pub mod ui;

// Re-export the most commonly used types so downstream code can write
// `appclean::AppBundle` instead of `appclean::bundle::AppBundle`.
pub use bundle::AppBundle;
pub use scanner::{FoundFile, Scanner};
pub use trash::TrashStore;

use indicatif::{ProgressBar, ProgressStyle};

const PROGRESS_TEMPLATE: &str = "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}";

/// Build a consistently-styled progress bar for file operations.
pub(crate) fn styled_progress_bar(len: u64) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(PROGRESS_TEMPLATE)
            .expect("hardcoded progress template is valid")
            .progress_chars("=>-"),
    );
    pb
}
