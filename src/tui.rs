use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Constraint,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io::stdout;

use crate::search::Clip;

#[derive(Debug)]
pub enum ClipChoice {
    Selected(Clip),
    Random,
    Quit,
}

pub fn select_clip(
    word: &str,
    clips: &[Clip],
    word_index: usize,
    total_words: usize,
) -> Result<ClipChoice> {
    if clips.is_empty() {
        eprintln!("No clips found for '{}', will use fallback", word);
        return Ok(ClipChoice::Random);
    }

    enable_raw_mode().context("Failed to enable raw mode")?;
    stdout().execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut state = ListState::default();
    state.select(Some(0));

    let result = run_selection_loop(&mut terminal, word, clips, &mut state, word_index, total_words);

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_selection_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    word: &str,
    clips: &[Clip],
    state: &mut ListState,
    word_index: usize,
    total_words: usize,
) -> Result<ClipChoice> {
    loop {
        terminal.draw(|frame| {
            let chunks = ratatui::layout::Layout::default()
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(frame.area());

            let header = Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" Word {}/{}: ", word_index + 1, total_words),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("\"{}\"", word),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
            .block(Block::default().borders(Borders::ALL).title("BadTV"));

            let items: Vec<ListItem> = clips
                .iter()
                .enumerate()
                .map(|(i, clip)| {
                    let date_end = 10.min(clip.date.len());
                    let content = format!(
                        " {}. {} ({}) \"{}\"",
                        i + 1,
                        clip.show,
                        &clip.date[..date_end],
                        highlight_word(&clip.snippet, word)
                    );
                    ListItem::new(content)
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  > ");

            let footer =
                Paragraph::new(" arrows: navigate  enter: select  r: random  q: quit")
                    .style(Style::default().fg(Color::DarkGray));

            frame.render_widget(header, chunks[0]);
            frame.render_stateful_widget(list, chunks[1], state);
            frame.render_widget(footer, chunks[2]);
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(ClipChoice::Quit),
                KeyCode::Char('r') => return Ok(ClipChoice::Random),
                KeyCode::Enter => {
                    let idx = state.selected().unwrap_or(0);
                    return Ok(ClipChoice::Selected(clips[idx].clone()));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some(if i == 0 { clips.len() - 1 } else { i - 1 }));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + 1) % clips.len()));
                }
                _ => {}
            }
        }
    }
}

fn highlight_word(snippet: &str, word: &str) -> String {
    let lower_snippet = snippet.to_lowercase();
    let lower_word = word.to_lowercase();
    if let Some(pos) = lower_snippet.find(&lower_word) {
        let before = &snippet[..pos];
        let matched = &snippet[pos..pos + word.len()];
        let after = &snippet[pos + word.len()..];
        format!("{}{}{}", before, matched.to_uppercase(), after)
    } else {
        snippet.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_word() {
        assert_eq!(
            highlight_word("we must act on this", "act"),
            "we must ACT on this"
        );
    }

    #[test]
    fn test_highlight_not_found() {
        assert_eq!(highlight_word("hello world", "missing"), "hello world");
    }
}
