use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};

use appclean::{bundle, cleaner, scanner, ui};

#[derive(Parser, Debug)]
#[command(
    name = "appclean",
    version,
    about = "Remove a macOS app and all its associated files"
)]
struct Cli {
    /// Path to the .app bundle (e.g. /Applications/Slack.app)
    app: PathBuf,

    /// Show what would be deleted without deleting anything
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Skip the confirmation prompt and delete immediately
    #[arg(short, long)]
    yes: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // 1. Parse the .app bundle
    let bundle = bundle::AppBundle::from_path(&cli.app)?;
    println!("Scanning for files associated with {}…", bundle.name);

    // 2. Scan with a spinner so the terminal doesn't feel frozen on large libraries
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg}")?,
    );
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner.set_message("searching…");

    let scanner = scanner::Scanner::new()?;
    let found = scanner.scan(&bundle)?;

    spinner.finish_and_clear();

    if found.is_empty() {
        println!("No associated files found for {}.", bundle.name);
        return Ok(());
    }

    // 3. Let the user choose which files to remove
    let selected = ui::select_files(&bundle.name, &found)?;

    if selected.is_empty() {
        println!("Nothing selected, exiting.");
        return Ok(());
    }

    // 4. Dry-run: just show what would happen
    if cli.dry_run {
        ui::show_dry_run(&selected);
        return Ok(());
    }

    // 5. Confirm unless --yes was passed
    if !cli.yes && !ui::confirm_deletion(&selected)? {
        println!("Aborted.");
        return Ok(());
    }

    // 6. Delete
    cleaner::delete_files(&selected)?;

    Ok(())
}
