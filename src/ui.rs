use std::io::{stdout, Stdout};
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use bytesize::ByteSize;
use colored::Colorize;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

use crate::scanner::FoundFile;

type Tui = Terminal<CrosstermBackend<Stdout>>;

// ── TUI lifecycle ─────────────────────────────────────────────────────────────

fn init_tui() -> Result<Tui> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout()))?)
}

fn restore_tui(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Run a closure with a TUI terminal, restoring the terminal on exit or error.
fn with_tui<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&mut Tui) -> Result<T>,
{
    let mut terminal = init_tui()?;
    let result = f(&mut terminal);
    let _ = restore_tui(&mut terminal);
    result
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Show an interactive file list. Returns the files the user chose to remove.
/// Returns an empty vec if the user quits without confirming.
pub fn select_files(app_name: &str, files: &[FoundFile]) -> Result<Vec<FoundFile>> {
    with_tui(|t| run_file_selector(t, app_name, files))
}

/// Show a yes/no confirmation dialog. Defaults to No for safety.
pub fn confirm_deletion(files: &[FoundFile]) -> Result<bool> {
    let total: u64 = files.iter().map(|f| f.size).sum();
    with_tui(|t| run_confirm(t, files.len(), total))
}

/// Show a scrollable list and return the index the user selected, or None if
/// they quit. Used for restore session selection.
pub fn select_from_list(prompt: &str, items: &[String]) -> Result<Option<usize>> {
    with_tui(|t| run_list_selector(t, prompt, items))
}

/// Print a dry-run summary to stdout (no TUI needed — nothing is interactive).
pub fn show_dry_run(files: &[FoundFile]) {
    println!("\n{}", "[dry run] Would delete:".yellow().bold());
    for f in files {
        println!("  {}  {}", shorten_path(&f.path), ByteSize(f.size).to_string().yellow());
    }
    let total: u64 = files.iter().map(|f| f.size).sum();
    println!("\n{} {}", "Total:".bold(), ByteSize(total).to_string().bold());
}

// ── File selector ─────────────────────────────────────────────────────────────

fn run_file_selector(terminal: &mut Tui, app_name: &str, files: &[FoundFile]) -> Result<Vec<FoundFile>> {
    let mut selected = vec![true; files.len()];
    let mut cursor = 0usize;

    loop {
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(area);

            // Reserve space for checkbox + size + tag columns
            let path_width = (area.width as usize).saturating_sub(22);

            let items: Vec<ListItem> = files
                .iter()
                .enumerate()
                .map(|(i, file)| {
                    let checkbox = if selected[i] { "[✓]" } else { "[ ]" };
                    let path = shorten_path(&file.path);
                    let path_display = truncate_left(&path, path_width);
                    let size = format!("{:>9}", ByteSize(file.size).to_string());
                    let tag = if file.is_bundle { " [app]" } else { "      " };

                    let style = if i == cursor {
                        Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
                    } else if selected[i] {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!(" {checkbox} {path_display:<path_width$}  {size}{tag}"),
                            style,
                        ),
                    ]))
                })
                .collect();

            let selected_count = selected.iter().filter(|&&s| s).count();
            let selected_bytes: u64 = files
                .iter()
                .enumerate()
                .filter(|(i, _)| selected[*i])
                .map(|(_, f)| f.size)
                .sum();

            let title = format!(
                " {} — {}/{} selected  ({}) ",
                app_name,
                selected_count,
                files.len(),
                ByteSize(selected_bytes),
            );

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title));
            let mut state = ListState::default().with_selected(Some(cursor));
            f.render_stateful_widget(list, chunks[0], &mut state);

            let help = Paragraph::new(
                " ↑/k ↓/j  Navigate    Space  Toggle    a  Toggle all    Enter  Confirm    q  Quit",
            )
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::DarkGray));
            f.render_widget(help, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if cursor + 1 < files.len() {
                            cursor += 1;
                        }
                    }
                    KeyCode::Char(' ') => {
                        selected[cursor] = !selected[cursor];
                    }
                    KeyCode::Char('a') => {
                        let all = selected.iter().all(|&s| s);
                        selected.iter_mut().for_each(|s| *s = !all);
                    }
                    KeyCode::Enter => break,
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(Vec::new()),
                    _ => {}
                }
            }
        }
    }

    Ok(files
        .iter()
        .enumerate()
        .filter(|(i, _)| selected[*i])
        .map(|(_, f)| f.clone())
        .collect())
}

// ── Confirm dialog ────────────────────────────────────────────────────────────

fn run_confirm(terminal: &mut Tui, count: usize, total_bytes: u64) -> Result<bool> {
    // Default to No — forces the user to actively choose Yes
    let mut confirm = false;

    loop {
        terminal.draw(|f| {
            let area = f.area();

            // Centre a dialog box in the terminal
            let vchunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(35),
                    Constraint::Length(9),
                    Constraint::Min(0),
                ])
                .split(area);

            let hchunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Percentage(60),
                    Constraint::Percentage(20),
                ])
                .split(vchunks[1]);

            let dialog_area = hchunks[1];
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Confirm deletion ");
            let inner = block.inner(dialog_area);
            f.render_widget(block, dialog_area);

            let inner_chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            f.render_widget(
                Paragraph::new(format!(
                    "Will free {} across {} item(s)",
                    ByteSize(total_bytes),
                    count,
                ))
                .alignment(Alignment::Center),
                inner_chunks[1],
            );

            let yes_style = if confirm {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let no_style = if !confirm {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("  Yes, delete  ", yes_style),
                    Span::raw("    "),
                    Span::styled("  No, cancel  ", no_style),
                ]))
                .alignment(Alignment::Center),
                inner_chunks[3],
            );

            f.render_widget(
                Paragraph::new("← →  Switch    y  Yes    n/Esc  No")
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                inner_chunks[4],
            );
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                        confirm = !confirm;
                    }
                    KeyCode::Char('y') => return Ok(true),
                    KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => return Ok(false),
                    KeyCode::Enter => return Ok(confirm),
                    _ => {}
                }
            }
        }
    }
}

// ── Generic list selector ─────────────────────────────────────────────────────

fn run_list_selector(terminal: &mut Tui, prompt: &str, items: &[String]) -> Result<Option<usize>> {
    let mut cursor = 0usize;

    loop {
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(area);

            let list_items: Vec<ListItem> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let style = if i == cursor {
                        Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Span::styled(format!(" {item} "), style))
                })
                .collect();

            let list = List::new(list_items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {prompt} ")),
            );
            let mut state = ListState::default().with_selected(Some(cursor));
            f.render_stateful_widget(list, chunks[0], &mut state);

            let help = Paragraph::new(" ↑/k ↓/j  Navigate    Enter  Select    q  Quit")
                .block(Block::default().borders(Borders::ALL))
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(help, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if cursor + 1 < items.len() {
                            cursor += 1;
                        }
                    }
                    KeyCode::Enter => return Ok(Some(cursor)),
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                    _ => {}
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn shorten_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}

/// Truncate a string from the left, adding a `…` prefix if it was clipped.
fn truncate_left(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.len() <= max {
        return s.to_string();
    }
    format!("…{}", &s[s.len() - max.saturating_sub(1)..])
}
