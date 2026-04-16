use anyhow::Result;
use bytesize::ByteSize;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, MultiSelect};

use crate::scanner::FoundFile;

/// Present an interactive multi-select list and return the chosen entries.
pub fn select_files(app_name: &str, files: &[FoundFile]) -> Result<Vec<FoundFile>> {
    println!(
        "\n{} {}\n",
        "Associated files for".bold(),
        app_name.cyan().bold()
    );

    let items: Vec<String> = files.iter().map(format_entry).collect();
    let defaults = vec![true; files.len()];

    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Space to toggle  ·  Enter to confirm")
        .items(&items)
        .defaults(&defaults)
        .interact()?;

    Ok(selections.into_iter().map(|i| files[i].clone()).collect())
}

/// Ask the user to confirm permanent deletion and return their answer.
pub fn confirm_deletion(files: &[FoundFile]) -> Result<bool> {
    let total: u64 = files.iter().map(|f| f.size).sum();
    println!(
        "\n{} {} across {} item(s)\n",
        "Will free:".bold(),
        ByteSize(total).to_string().red().bold(),
        files.len(),
    );

    Ok(Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Permanently delete these files?")
        .default(false)
        .interact()?)
}

/// Print what would be deleted without taking any action.
pub fn show_dry_run(files: &[FoundFile]) {
    println!("\n{}", "[dry run] Would delete:".yellow().bold());
    for f in files {
        println!("  {}", format_entry(f));
    }
    let total: u64 = files.iter().map(|f| f.size).sum();
    println!("\n{} {}", "Total:".bold(), ByteSize(total).to_string().bold());
}

fn format_entry(f: &FoundFile) -> String {
    let tag = if f.is_bundle {
        " [app]".dimmed().to_string()
    } else {
        String::new()
    };
    format!(
        "{:<60}{:>10}{}",
        f.path.display(),
        ByteSize(f.size).to_string().yellow(),
        tag
    )
}
