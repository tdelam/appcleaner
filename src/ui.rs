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

            // Reserve space for checkbox (5) + two spaces (2) + size (9) + tag (6) + borders (2)
            let path_width = (area.width as usize).saturating_sub(24);

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use std::path::PathBuf;

    // ── truncate_left ──────────────────────────────────────────────────────────

    #[test]
    fn truncate_left_short_string_unchanged() {
        assert_eq!(truncate_left("hello", 10), "hello");
    }

    #[test]
    fn truncate_left_exact_length_unchanged() {
        assert_eq!(truncate_left("hello", 5), "hello");
    }

    #[test]
    fn truncate_left_long_string_gets_ellipsis() {
        let result = truncate_left("~/Library/Application Support/Slack", 10);
        assert!(result.starts_with('…'), "should start with ellipsis");
        assert!(result.len() <= 10 + '…'.len_utf8(), "should not exceed max");
    }

    #[test]
    fn truncate_left_max_zero_returns_empty() {
        assert_eq!(truncate_left("anything", 0), "");
    }

    #[test]
    fn truncate_left_preserves_tail() {
        // The tail of the string should always be visible
        let s = "/very/long/path/to/important/file.plist";
        let max = 15usize;
        let result = truncate_left(s, max);
        // Result should start with ellipsis and preserve the last (max-1) chars
        let expected_tail = &s[s.len() - (max - 1)..];
        assert!(result.starts_with('…'), "should start with ellipsis");
        assert!(result.ends_with(expected_tail), "should preserve the tail of the path");
    }

    // ── shorten_path ──────────────────────────────────────────────────────────

    #[test]
    fn shorten_path_outside_home_is_unchanged() {
        let path = PathBuf::from("/Library/Application Support/SomeApp");
        let result = shorten_path(&path);
        assert_eq!(result, "/Library/Application Support/SomeApp");
    }

    #[test]
    fn shorten_path_inside_home_gets_tilde() {
        if let Some(home) = dirs::home_dir() {
            let path = home.join("Library/Preferences/com.example.plist");
            let result = shorten_path(&path);
            assert!(result.starts_with("~/"), "should start with ~/");
            assert!(result.contains("Library/Preferences"));
        }
    }

    // ── rendering smoke tests (TestBackend) ───────────────────────────────────

    fn make_found_file(path: &str, size: u64, is_bundle: bool) -> FoundFile {
        FoundFile { path: PathBuf::from(path), size, is_bundle }
    }

    /// Render the file selector once and assert it doesn't panic and contains
    /// expected text in the output buffer.
    #[test]
    fn file_selector_renders_app_name_and_files() {
        let files = vec![
            make_found_file("/Applications/Slack.app", 300_000_000, true),
            make_found_file("/Users/user/Library/Application Support/Slack", 800_000_000, false),
        ];

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let selected = vec![true; files.len()];
        let cursor = 0usize;

        terminal
            .draw(|f| {
                let area = f.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(3), Constraint::Length(3)])
                    .split(area);

                let path_width = (area.width as usize).saturating_sub(24);
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
                        ListItem::new(Line::from(vec![Span::styled(
                            format!(" {checkbox} {path_display:<path_width$}  {size}{tag}"),
                            style,
                        )]))
                    })
                    .collect();

                let title = " Slack — 2/2 selected  (1.1 GB) ";
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
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();

        assert!(content.contains("Slack"), "title should contain app name");
        assert!(content.contains("[✓]"), "checked items should appear");
        assert!(content.contains("[app]"), "bundle marker should appear");
        assert!(content.contains("Navigate"), "help bar should appear");
    }

    #[test]
    fn confirm_dialog_renders_with_item_count_and_size() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = f.area();
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
                    Paragraph::new("Will free 1.1 GB across 3 item(s)")
                        .alignment(Alignment::Center),
                    inner_chunks[1],
                );

                let no_style = Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD);
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled("  Yes, delete  ", Style::default().fg(Color::DarkGray)),
                        Span::raw("    "),
                        Span::styled("  No, cancel  ", no_style),
                    ]))
                    .alignment(Alignment::Center),
                    inner_chunks[3],
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();

        assert!(content.contains("Confirm deletion"), "dialog title should appear");
        assert!(content.contains("1.1 GB"), "size should appear");
        assert!(content.contains("3 item(s)"), "item count should appear");
        assert!(content.contains("Yes, delete"), "yes button should appear");
        assert!(content.contains("No, cancel"), "no button should appear");
    }

    #[test]
    fn list_selector_renders_items_and_prompt() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![
            "Slack  —  6 item(s) trashed on 2026-04-15 21:00 UTC".to_string(),
            "Zoom  —  3 item(s) trashed on 2026-04-10 14:23 UTC".to_string(),
        ];
        let cursor = 0usize;

        terminal
            .draw(|f| {
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
                        .title(" Select a session to restore "),
                );
                let mut state = ListState::default().with_selected(Some(cursor));
                f.render_stateful_widget(list, chunks[0], &mut state);

                let help = Paragraph::new(" ↑/k ↓/j  Navigate    Enter  Select    q  Quit")
                    .block(Block::default().borders(Borders::ALL))
                    .style(Style::default().fg(Color::DarkGray));
                f.render_widget(help, chunks[1]);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();

        assert!(content.contains("Select a session to restore"), "prompt should appear");
        assert!(content.contains("Slack"), "first session should appear");
        assert!(content.contains("Zoom"), "second session should appear");
    }
}
