use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};
use indicatif::{ProgressBar, ProgressStyle};

use appclean::{bundle, cleaner, scanner, trash, ui};

#[derive(Parser, Debug)]
#[command(
    name = "appclean",
    version,
    about = "Remove a macOS app and all its associated files"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the .app bundle (e.g. /Applications/Slack.app)
    app: Option<PathBuf>,

    /// Show what would be deleted without deleting anything
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Skip the confirmation prompt
    #[arg(short, long)]
    yes: bool,

    /// Permanently delete files instead of moving them to the appclean trash
    #[arg(long)]
    permanent: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Restore files from a previous appclean trash session
    Restore,
    /// Permanently delete sessions from the appclean trash
    EmptyTrash {
        /// Only remove sessions older than this many days
        #[arg(long, value_name = "DAYS")]
        older_than: Option<u64>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Restore) => return cmd_restore(),
        Some(Command::EmptyTrash { older_than }) => return cmd_empty_trash(older_than),
        None => {}
    }

    let app_path = cli.app.ok_or_else(|| anyhow::anyhow!("a .app path is required\n\nUsage: appclean <APP>\n       appclean restore"))?;
    cmd_clean(app_path, cli.dry_run, cli.yes, cli.permanent)
}

fn cmd_clean(app_path: PathBuf, dry_run: bool, yes: bool, permanent: bool) -> Result<()> {
    // 1. Parse the .app bundle
    let bundle = bundle::AppBundle::from_path(&app_path)?;
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
    if dry_run {
        ui::show_dry_run(&selected);
        return Ok(());
    }

    // 5. Confirm unless --yes was passed
    if !yes && !ui::confirm_deletion(&selected)? {
        println!("Aborted.");
        return Ok(());
    }

    // 6. Move to trash (default) or permanently delete
    if permanent {
        cleaner::delete_files(&selected)?;
    } else {
        trash::TrashStore::new()?.move_to_trash(&selected, &bundle.name)?;
    }

    Ok(())
}

fn cmd_empty_trash(older_than: Option<u64>) -> Result<()> {
    let store = trash::TrashStore::new()?;

    if let Some(days) = older_than {
        println!("Permanently removing trash sessions older than {} day(s)…", days);
    } else {
        println!("Permanently removing all sessions from trash…");
    }

    store.empty_trash(older_than)?;
    Ok(())
}

fn cmd_restore() -> Result<()> {
    let entries = trash::TrashStore::new()?.list_entries()?;

    if entries.is_empty() {
        println!("No items in the appclean trash.");
        return Ok(());
    }

    let labels: Vec<String> = entries.iter().map(|(_, e)| e.label()).collect();

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select a session to restore")
        .items(&labels)
        .default(0)
        .interact()?;

    let (session_path, entry) = &entries[selection];

    println!(
        "\nRestoring {} item(s) for {}…\n",
        entry.items.len(),
        entry.app_name.cyan().bold()
    );

    trash::restore(session_path, entry)?;

    Ok(())
}
