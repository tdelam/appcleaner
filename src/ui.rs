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

/// RAII guard that owns the terminal and restores cooked mode / the main
/// screen on drop. This runs on both normal return AND stack unwinding, so
/// a panic inside the TUI closure can't leave the user stuck in raw mode.
struct TuiGuard {
    terminal: Tui,
}

impl TuiGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
        Ok(Self { terminal })
    }
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        // Best-effort — nothing useful to do if restoration fails, and we
        // must not panic during drop.
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// Run a closure with a TUI terminal. The terminal is restored on normal
/// return, error return, and panic unwinding via `TuiGuard`'s Drop impl.
fn with_tui<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&mut Tui) -> Result<T>,
{
    let mut guard = TuiGuard::new()?;
    f(&mut guard.terminal)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Show an interactive file list. Returns the files the user chose to remove.
/// Returns an empty vec if the user quits without confirming.
///
/// Takes `files` by value so that selected items move into the returned vec
/// without per-item clones — important for large scans.
///
/// # Errors
/// Returns an error if terminal initialisation or event polling fails.
pub fn select_files(app_name: &str, files: Vec<FoundFile>) -> Result<Vec<FoundFile>> {
    with_tui(|t| run_file_selector(t, app_name, files))
}

/// Show a yes/no confirmation dialog. Defaults to No for safety.
///
/// # Errors
/// Returns an error if terminal initialisation or event polling fails.
pub fn confirm_deletion(files: &[FoundFile]) -> Result<bool> {
    let total: u64 = files.iter().map(|f| f.size).sum();
    with_tui(|t| run_confirm(t, files.len(), total))
}

/// Show a scrollable list and return the index the user selected, or `None` if
/// they quit. Used for restore session selection.
///
/// # Errors
/// Returns an error if terminal initialisation or event polling fails.
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

#[allow(clippy::too_many_lines)]
fn run_file_selector(terminal: &mut Tui, app_name: &str, files: Vec<FoundFile>) -> Result<Vec<FoundFile>> {
    let mut selected = vec![true; files.len()];
    let mut cursor = 0usize;
    let max_size = files.iter().map(|f| f.size).max().unwrap_or(1);

    loop {
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(area);

            // columns: check(3) + path + bar_with_gaps(8) + size(9) + tag(6) + borders(2) = 28
            let path_width = (area.width as usize).saturating_sub(28);

            let items: Vec<ListItem> = files
                .iter()
                .enumerate()
                .map(|(i, file)| {
                    let is_sel = selected[i];

                    let check_span = Span::styled(
                        if is_sel { " ◉ " } else { " ○ " },
                        if is_sel {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        },
                    );

                    let path = shorten_path(&file.path);
                    let path_display = format!("{:<path_width$}", truncate_left(&path, path_width));
                    let path_color = if is_sel {
                        file_type_color(&file.path)
                    } else {
                        Color::DarkGray
                    };
                    let path_span = Span::styled(
                        path_display,
                        Style::default().fg(path_color),
                    );

                    let bar = size_bar(file.size, max_size, 6);
                    let bar_span = Span::styled(
                        format!(" {bar} "),
                        if is_sel {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        },
                    );

                    let size_span = Span::styled(
                        format!("{:>9}", ByteSize(file.size).to_string()),
                        if is_sel {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        },
                    );

                    let tag_span = if file.is_bundle {
                        Span::styled(" [app]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                    } else {
                        Span::raw("      ")
                    };

                    ListItem::new(Line::from(vec![
                        check_span, path_span, bar_span, size_span, tag_span,
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

            let title = Line::from(vec![
                Span::raw(" "),
                Span::styled(app_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" — {}/{} selected  ({}) ", selected_count, files.len(), ByteSize(selected_bytes)),
                    Style::default().fg(Color::White),
                ),
            ]);

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_style(
                    Style::default()
                        .bg(Color::Rgb(45, 45, 65))
                        .add_modifier(Modifier::BOLD),
                );
            let mut state = ListState::default().with_selected(Some(cursor));
            f.render_stateful_widget(list, chunks[0], &mut state);

            let help = Line::from(vec![
                Span::raw(" "),
                Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
                Span::raw(" Navigate    "),
                Span::styled("Space", Style::default().fg(Color::Cyan)),
                Span::raw(" Toggle    "),
                Span::styled("a", Style::default().fg(Color::Cyan)),
                Span::raw(" Toggle all    "),
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(" Confirm    "),
                Span::styled("q", Style::default().fg(Color::Red)),
                Span::raw(" Quit"),
            ]);
            f.render_widget(
                Paragraph::new(help).block(Block::default().borders(Borders::ALL)),
                chunks[1],
            );
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
                        selected.fill(!all);
                    }
                    KeyCode::Enter => break,
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(Vec::new()),
                    _ => {}
                }
            }
        }
    }

    Ok(files
        .into_iter()
        .zip(selected)
        .filter_map(|(f, is_selected)| is_selected.then_some(f))
        .collect())
}

// ── Confirm dialog ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
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
                    Constraint::Percentage(30),
                    Constraint::Length(11),
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
                .border_style(Style::default().fg(Color::Red))
                .title(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("⚠  Confirm deletion", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                ]));
            let inner = block.inner(dialog_area);
            f.render_widget(block, dialog_area);

            let inner_chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(1), // padding
                    Constraint::Length(1), // summary line
                    Constraint::Length(1), // restore hint
                    Constraint::Length(1), // padding
                    Constraint::Length(1), // buttons
                    Constraint::Length(1), // key hints
                ])
                .split(inner);

            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!("{count} item(s)"),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" will be moved to the appclean trash  ("),
                    Span::styled(
                        ByteSize(total_bytes).to_string(),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(")"),
                ]))
                .alignment(Alignment::Center),
                inner_chunks[1],
            );

            f.render_widget(
                Paragraph::new(
                    Span::styled(
                        "Restore any time with: apc restore",
                        Style::default().fg(Color::DarkGray),
                    ),
                )
                .alignment(Alignment::Center),
                inner_chunks[2],
            );

            let yes_style = if confirm {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let no_style = if confirm {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            };

            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("  Yes, delete  ", yes_style),
                    Span::raw("    "),
                    Span::styled("  No, cancel  ", no_style),
                ]))
                .alignment(Alignment::Center),
                inner_chunks[4],
            );

            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("← →", Style::default().fg(Color::Cyan)),
                    Span::raw(" Switch    "),
                    Span::styled("y", Style::default().fg(Color::Red)),
                    Span::raw(" Yes    "),
                    Span::styled("n/Esc", Style::default().fg(Color::Green)),
                    Span::raw(" No"),
                ]))
                .alignment(Alignment::Center),
                inner_chunks[5],
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
                    KeyCode::Char('n' | 'q') | KeyCode::Esc => return Ok(false),
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
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Span::styled(format!(" {item} "), style))
                })
                .collect();

            let title = Line::from(vec![
                Span::raw(" "),
                Span::styled(prompt, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
            ]);

            let list = List::new(list_items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_style(
                    Style::default()
                        .bg(Color::Rgb(45, 45, 65))
                        .add_modifier(Modifier::BOLD),
                );
            let mut state = ListState::default().with_selected(Some(cursor));
            f.render_stateful_widget(list, chunks[0], &mut state);

            let help = Line::from(vec![
                Span::raw(" "),
                Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
                Span::raw(" Navigate    "),
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(" Select    "),
                Span::styled("q", Style::default().fg(Color::Red)),
                Span::raw(" Quit"),
            ]);
            f.render_widget(
                Paragraph::new(help).block(Block::default().borders(Borders::ALL)),
                chunks[1],
            );
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

/// Truncate a string from the left to at most `max` characters, adding a `…`
/// prefix if it was clipped. Operates on chars — not bytes — so it cannot
/// panic on non-ASCII paths.
fn truncate_left(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let char_count = s.chars().count();
    if char_count <= max {
        return s.to_string();
    }
    // Keep the last (max - 1) chars and prefix with `…` for a total of `max`.
    let skip = char_count - max.saturating_sub(1);
    let tail: String = s.chars().skip(skip).collect();
    format!("…{tail}")
}

/// Returns a color hinting at where the file lives on disk.
fn file_type_color(path: &Path) -> Color {
    let s = path.to_string_lossy();
    if s.ends_with(".app") {
        Color::Red
    } else if s.contains("/Caches") {
        Color::Yellow
    } else if s.contains("/Preferences") {
        Color::Blue
    } else if s.contains("/Logs") {
        Color::Cyan
    } else if s.contains("/Containers") {
        Color::Magenta
    } else {
        Color::White
    }
}

/// Render a proportional block bar of `width` chars (e.g. `████░░`).
fn size_bar(size: u64, max: u64, width: usize) -> String {
    if width == 0 || max == 0 {
        return "░".repeat(width);
    }
    // Integer-only arithmetic — avoids float precision/truncation casts.
    // saturating_mul guards against overflow on pathologically large sizes.
    // try_from is infallible here: the value is bounded by .min(width as u64)
    // which already fits in a usize.
    let filled = usize::try_from(
        (size.saturating_mul(width as u64) / max).min(width as u64),
    )
    .unwrap_or(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
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
        let s = "/very/long/path/to/important/file.plist";
        let max = 15usize;
        let result = truncate_left(s, max);
        let expected_tail = &s[s.len() - (max - 1)..];
        assert!(result.starts_with('…'), "should start with ellipsis");
        assert!(result.ends_with(expected_tail), "should preserve the tail of the path");
    }

    #[test]
    fn truncate_left_does_not_panic_on_multibyte_chars() {
        // Byte-slicing would panic if the cut landed mid-codepoint. Char-based
        // truncation keeps the last `max - 1` chars regardless of byte width.
        let s = "~/Library/Application Support/微信/Data";
        let result = truncate_left(s, 12);
        assert_eq!(result.chars().count(), 12);
        assert!(result.starts_with('…'));
        assert!(result.ends_with("Data"));
    }

    #[test]
    fn truncate_left_fits_exactly_on_unicode_boundary() {
        let s = "αβγδε"; // 5 chars, 10 bytes
        assert_eq!(truncate_left(s, 5), s);
        assert_eq!(truncate_left(s, 3), "…δε");
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

    // ── size_bar ──────────────────────────────────────────────────────────────

    #[test]
    fn size_bar_full_when_max() {
        let bar = size_bar(100, 100, 6);
        assert_eq!(bar, "██████");
    }

    #[test]
    fn size_bar_empty_when_zero() {
        let bar = size_bar(0, 100, 6);
        assert_eq!(bar, "░░░░░░");
    }

    #[test]
    fn size_bar_correct_width() {
        let bar = size_bar(50, 100, 8);
        assert_eq!(bar.chars().count(), 8);
    }

    // ── file_type_color ───────────────────────────────────────────────────────

    #[test]
    fn file_type_color_bundle_is_red() {
        assert_eq!(file_type_color(Path::new("/Applications/Slack.app")), Color::Red);
    }

    #[test]
    fn file_type_color_caches_is_yellow() {
        assert_eq!(
            file_type_color(Path::new("/Users/user/Library/Caches/Slack")),
            Color::Yellow,
        );
    }

    // ── rendering smoke tests (TestBackend) ───────────────────────────────────

    fn make_found_file(path: &str, size: u64, is_bundle: bool) -> FoundFile {
        FoundFile { path: PathBuf::from(path), size, is_bundle }
    }

    #[test]
    fn file_selector_renders_app_name_and_files() {
        let files = [
            make_found_file("/Applications/Slack.app", 300_000_000, true),
            make_found_file("/Users/user/Library/Application Support/Slack", 800_000_000, false),
        ];
        let max_size = files.iter().map(|f| f.size).max().unwrap_or(1);

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

                // columns: check(3) + path + bar_with_gaps(8) + size(9) + tag(6) + borders(2) = 28
                let path_width = (area.width as usize).saturating_sub(28);

                let items: Vec<ListItem> = files
                    .iter()
                    .enumerate()
                    .map(|(i, file)| {
                        let is_sel = selected[i];
                        let check_span = Span::styled(
                            if is_sel { " ◉ " } else { " ○ " },
                            if is_sel {
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::DarkGray)
                            },
                        );
                        let path = shorten_path(&file.path);
                        let path_display = format!("{:<path_width$}", truncate_left(&path, path_width));
                        let path_span = Span::styled(
                            path_display,
                            Style::default().fg(file_type_color(&file.path)),
                        );
                        let bar = size_bar(file.size, max_size, 6);
                        let bar_span = Span::styled(
                            format!(" {bar} "),
                            Style::default().fg(Color::Cyan),
                        );
                        let size_span = Span::styled(
                            format!("{:>9}", ByteSize(file.size).to_string()),
                            Style::default().fg(Color::Yellow),
                        );
                        let tag_span = if file.is_bundle {
                            Span::styled(" [app]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                        } else {
                            Span::raw("      ")
                        };
                        ListItem::new(Line::from(vec![
                            check_span, path_span, bar_span, size_span, tag_span,
                        ]))
                        .style(if i == cursor {
                            Style::default().bg(Color::Rgb(45, 45, 65))
                        } else {
                            Style::default()
                        })
                    })
                    .collect();

                let title = Line::from(vec![
                    Span::raw(" "),
                    Span::styled("Slack", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(" — 2/2 selected  (1.1 GB) "),
                ]);
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title(title))
                    .highlight_style(Style::default().bg(Color::Rgb(45, 45, 65)));
                let mut state = ListState::default().with_selected(Some(cursor));
                f.render_stateful_widget(list, chunks[0], &mut state);

                let help = Line::from(vec![
                    Span::raw(" "),
                    Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
                    Span::raw(" Navigate    "),
                    Span::styled("Space", Style::default().fg(Color::Cyan)),
                    Span::raw(" Toggle    "),
                    Span::styled("a", Style::default().fg(Color::Cyan)),
                    Span::raw(" Toggle all    "),
                    Span::styled("Enter", Style::default().fg(Color::Green)),
                    Span::raw(" Confirm    "),
                    Span::styled("q", Style::default().fg(Color::Red)),
                    Span::raw(" Quit"),
                ]);
                f.render_widget(
                    Paragraph::new(help).block(Block::default().borders(Borders::ALL)),
                    chunks[1],
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();

        assert!(content.contains("Slack"), "title should contain app name");
        assert!(content.contains("◉"), "selected items should show filled circle");
        assert!(content.contains("[app]"), "bundle marker should appear");
        assert!(content.contains("Navigate"), "help bar should appear");
        assert!(content.contains("█"), "size bar should appear");
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
                        Constraint::Percentage(30),
                        Constraint::Length(11),
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
                    .border_style(Style::default().fg(Color::Red))
                    .title(Line::from(vec![
                        Span::raw(" "),
                        Span::styled(
                            "⚠  Confirm deletion",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                    ]));
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
                        Constraint::Length(1),
                    ])
                    .split(inner);

                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled("3 item(s)", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::raw(" will be moved to the appclean trash  ("),
                        Span::styled("1.1 GB", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                        Span::raw(")"),
                    ]))
                    .alignment(Alignment::Center),
                    inner_chunks[1],
                );

                let no_style = Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD);
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled("  Yes, delete  ", Style::default().fg(Color::DarkGray)),
                        Span::raw("    "),
                        Span::styled("  No, cancel  ", no_style),
                    ]))
                    .alignment(Alignment::Center),
                    inner_chunks[4],
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
        let items = [
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
                            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        ListItem::new(Span::styled(format!(" {item} "), style))
                    })
                    .collect();

                let title = Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        "Select a session to restore",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                ]);
                let list = List::new(list_items)
                    .block(Block::default().borders(Borders::ALL).title(title))
                    .highlight_style(Style::default().bg(Color::Rgb(45, 45, 65)));
                let mut state = ListState::default().with_selected(Some(cursor));
                f.render_stateful_widget(list, chunks[0], &mut state);

                let help = Line::from(vec![
                    Span::raw(" "),
                    Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
                    Span::raw(" Navigate    "),
                    Span::styled("Enter", Style::default().fg(Color::Green)),
                    Span::raw(" Select    "),
                    Span::styled("q", Style::default().fg(Color::Red)),
                    Span::raw(" Quit"),
                ]);
                f.render_widget(
                    Paragraph::new(help).block(Block::default().borders(Borders::ALL)),
                    chunks[1],
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();

        assert!(content.contains("Select a session to restore"), "prompt should appear");
        assert!(content.contains("Slack"), "first session should appear");
        assert!(content.contains("Zoom"), "second session should appear");
    }
}
