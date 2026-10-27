//! Pull requests inspection view.

use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render_pull_requests(frame: &mut Frame, area: Rect, _state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let pr_items = vec![
        ListItem::new(Line::from(vec![
            Span::styled(
                "▶ #42 ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("[OPEN] ", Style::default().fg(Color::Green)),
            Span::raw("Implement safe fast-forward sync guard"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  #41 ", Style::default().fg(Color::White)),
            Span::styled("[MERGED] ", Style::default().fg(Color::Magenta)),
            Span::raw("Support multi-host OAuth and token authentication"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("  #40 ", Style::default().fg(Color::White)),
            Span::styled("[CLOSED] ", Style::default().fg(Color::Red)),
            Span::raw("Deprecated monolithic storage script"),
        ])),
    ];

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Pull Requests ")
        .style(Style::default().fg(Color::Magenta));

    let list_widget = List::new(pr_items).block(list_block);
    frame.render_widget(list_widget, chunks[0]);

    let details_block = Block::default()
        .borders(Borders::ALL)
        .title(" PR Details ")
        .style(Style::default().fg(Color::Blue));

    let details_text = "PR Number:   #42\n\
                        Title:       Implement safe fast-forward sync guard\n\
                        State:       OPEN\n\
                        Author:      dev-core\n\
                        Base Branch: main\n\
                        Head Branch: feat/sync-guards\n\n\
                        Description:\n\
                          Guards dirty and diverged local clones from being overwritten during sync.\n\
                          Fast-forwards only when local branch is clean and behind origin.";

    let details_paragraph = Paragraph::new(details_text)
        .block(details_block)
        .wrap(Wrap { trim: true });

    frame.render_widget(details_paragraph, chunks[1]);
}
