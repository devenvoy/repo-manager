//! Multi-host Git provider authentication overview.

use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render_auth(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let providers = ["GitHub", "GitLab", "Bitbucket"];
    let items: Vec<ListItem> = providers
        .iter()
        .map(|prov| {
            let is_authed = state.auth_status.get(*prov).copied().unwrap_or(false);
            let (mark, style) = if is_authed {
                (
                    "✓ Connected",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("✗ Not configured", Style::default().fg(Color::DarkGray))
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<15} ", prov), Style::default().fg(Color::White)),
                Span::styled(mark, style),
            ]))
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Git Hosting Providers ")
        .style(Style::default().fg(Color::Cyan));

    let list_widget = List::new(items).block(list_block);
    frame.render_widget(list_widget, chunks[0]);

    let details_block = Block::default()
        .borders(Borders::ALL)
        .title(" Authentication Methods ")
        .style(Style::default().fg(Color::Blue));

    let details_text = "Release Desk supports multi-host Git authentication:\n\n\
                        1. Personal Access Token (PAT):\n\
                           Direct token stored in OS secure keychain.\n\n\
                        2. Browser OAuth:\n\
                           Opens default system browser with loopback callback.\n\n\
                        3. Custom Webview Window:\n\
                           If system browser is absent or fails, an embedded\n\
                           modal panel opens and automatically closes on completion.\n\n\
                        To configure or login via CLI:\n\
                          release-desk auth login --provider <github|gitlab|bitbucket> --token <PAT>";

    let details_paragraph = Paragraph::new(details_text)
        .block(details_block)
        .wrap(Wrap { trim: true });

    frame.render_widget(details_paragraph, chunks[1]);
}
