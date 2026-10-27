//! Repositories browsing, filter, and detail view.

use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render_repositories(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let repos = state.filtered_repositories();

    // 1. Repository List (Left panel)
    let items: Vec<ListItem> = repos
        .iter()
        .enumerate()
        .map(|(i, repo)| {
            let is_selected = i == state.selected_index;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let flags = if repo.is_archived { " [ARCHIVED]" } else { "" };
            let line = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(&repo.name, style),
                Span::styled(flags, Style::default().fg(Color::DarkGray)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list_title = format!(" Repositories ({}) ", repos.len());
    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(list_title)
        .style(Style::default().fg(Color::Cyan));

    let list_widget = List::new(items).block(list_block);
    frame.render_widget(list_widget, chunks[0]);

    // 2. Repository Details (Right panel)
    let details_block = Block::default()
        .borders(Borders::ALL)
        .title(" Repository Metadata & Status ")
        .style(Style::default().fg(Color::Blue));

    let details_content = if let Some(selected_repo) = repos.get(state.selected_index) {
        let desc = selected_repo
            .description
            .as_deref()
            .unwrap_or("No description provided.");
        let lang = selected_repo
            .primary_language
            .as_ref()
            .map(|l| l.name.as_str())
            .unwrap_or("Unknown");
        let branch = selected_repo
            .default_branch_ref
            .as_ref()
            .map(|b| b.name.as_str())
            .unwrap_or("main");
        let commit = selected_repo
            .default_branch_ref
            .as_ref()
            .map(|b| b.target.oid.as_str())
            .unwrap_or("none");
        let sync_status = state
            .sync_statuses
            .get(&selected_repo.name)
            .map(|s| s.as_str())
            .unwrap_or("Current (Clean)");

        format!(
            "Name:           {}\n\
             URL:            {}\n\
             Language:       {}\n\
             Default Branch: {} (HEAD: {})\n\
             Sync Status:    {}\n\
             Archived:       {}\n\
             Private:        {}\n\n\
             Description:\n  {}\n\n\
             Actions:\n  [s] Safe Sync (Fast-forward)\n  [t] Toggle Tracking Flags",
            selected_repo.name,
            selected_repo.url,
            lang,
            branch,
            commit,
            sync_status,
            if selected_repo.is_archived {
                "Yes"
            } else {
                "No"
            },
            if selected_repo.is_private {
                "Yes"
            } else {
                "No"
            },
            desc
        )
    } else {
        "No repository selected.".to_string()
    };

    let details_paragraph = Paragraph::new(details_content)
        .block(details_block)
        .wrap(Wrap { trim: true });

    frame.render_widget(details_paragraph, chunks[1]);
}
