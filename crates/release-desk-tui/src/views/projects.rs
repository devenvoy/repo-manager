//! Projects management view.

use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render_projects(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let items: Vec<ListItem> = state
        .projects
        .iter()
        .enumerate()
        .map(|(i, proj)| {
            let is_selected = i == state.selected_index;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let line = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(proj.name.as_str(), style),
                Span::styled(
                    format!(" (id: {})", proj.id.as_u128()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Registered Projects ")
        .style(Style::default().fg(Color::Yellow));

    let list_widget = List::new(items).block(list_block);
    frame.render_widget(list_widget, chunks[0]);

    let details_block = Block::default()
        .borders(Borders::ALL)
        .title(" Project Configuration ")
        .style(Style::default().fg(Color::Blue));

    let details_text = if let Some(proj) = state.projects.get(state.selected_index) {
        let clone_dir = proj
            .clone_location
            .as_ref()
            .map(|l| l.display().to_string())
            .unwrap_or_else(|| "Not configured".into());
        let backup_file = proj
            .clone_location
            .as_ref()
            .map(|l| l.join(".release-desk/data.json").display().to_string())
            .unwrap_or_else(|| "None".into());

        format!(
            "Project ID:       {}\n\
             Project Name:     {}\n\
             Clone Location:   {}\n\
             Local Backup:     {}\n\n\
             Actions:\n\
               [Enter] Switch active project\n\
               [c]     Reconcile project backup snapshot",
            proj.id.as_u128(),
            proj.name.as_str(),
            clone_dir,
            backup_file
        )
    } else {
        "No projects registered. Create projects using `release-desk project create`.".to_string()
    };

    let details_paragraph = Paragraph::new(details_text)
        .block(details_block)
        .wrap(Wrap { trim: true });

    frame.render_widget(details_paragraph, chunks[1]);
}
