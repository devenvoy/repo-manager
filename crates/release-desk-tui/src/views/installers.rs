//! Release installer shelf and asset inspection view.

use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render_installers(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let repos = state.filtered_repositories();

    let items: Vec<ListItem> = repos
        .iter()
        .enumerate()
        .map(|(i, repo)| {
            let is_selected = i == state.selected_index;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let line = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(&repo.name, style),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Installer Packages ")
        .style(Style::default().fg(Color::Green));

    let list_widget = List::new(items).block(list_block);
    frame.render_widget(list_widget, chunks[0]);

    let details_block = Block::default()
        .borders(Borders::ALL)
        .title(" Asset Details & Download ")
        .style(Style::default().fg(Color::Blue));

    let details_content = if let Some(selected_repo) = repos.get(state.selected_index) {
        format!(
            "Repository:     {}\n\
             Asset Filter:   .dmg / .pkg / .zip\n\
             Latest Tag:     v1.0.0\n\
             Release Date:   2026-10-27\n\
             Status:         Available for Download\n\n\
             Installer Assets:\n\
               • {}-1.0.0-macOS-arm64.dmg (42.5 MB)\n\
               • {}-1.0.0-macOS-x86_64.dmg (44.1 MB)\n\n\
             Actions:\n\
               [d] Download & verify SHA-256 checksum\n\
               [o] Open downloaded asset in Finder",
            selected_repo.name, selected_repo.name, selected_repo.name
        )
    } else {
        "No repository selected.".to_string()
    };

    let details_paragraph = Paragraph::new(details_content)
        .block(details_block)
        .wrap(Wrap { trim: true });

    frame.render_widget(details_paragraph, chunks[1]);
}
