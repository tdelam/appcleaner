use std::path::Path;

use anyhow::Result;
use bytesize::ByteSize;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Input};

use crate::scanner::FoundFile;

/// Print the found files, then let the user optionally exclude items by number.
/// Returns the files that should be deleted.
pub fn select_files(app_name: &str, files: &[FoundFile]) -> Result<Vec<FoundFile>> {
    println!(
        "\n{} {}\n",
        "Associated files for".bold(),
        app_name.cyan().bold()
    );

    for (i, f) in files.iter().enumerate() {
        println!("  {:>2}.  {}", i + 1, format_entry(f));
    }

    println!();

    let exclude = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Exclude any items? (no = delete all)")
        .default(false)
        .interact()?;

    if !exclude {
        return Ok(files.to_vec());
    }

    let input: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Item numbers to exclude (e.g. 2,3)")
        .interact_text()?;

    let excluded: Vec<usize> = input
        .split(',')
        .filter_map(|s| s.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1 && n <= files.len())
        .map(|n| n - 1)
        .collect();

    Ok(files
        .iter()
        .enumerate()
        .filter(|(i, _)| !excluded.contains(i))
        .map(|(_, f)| f.clone())
        .collect())
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
        "{}  {}{}",
        shorten_path(&f.path),
        ByteSize(f.size).to_string().yellow(),
        tag
    )
}

fn shorten_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}
