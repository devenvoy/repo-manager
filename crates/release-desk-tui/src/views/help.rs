//! Modal help overlay showing key bindings.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render_help(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(60, 60, area);
    frame.render_widget(Clear, popup_area);

    let text = r#"
Keyboard Shortcuts:

  Navigation:
    1-6        Switch tabs directly
    Tab        Next tab
    BackTab    Previous tab
    j / Down   Move selection down
    k / Up     Move selection up
    Home / End Jump to start / end

  Actions:
    /          Search and filter items
    Esc        Clear search / dismiss modal
    ?          Toggle this help dialog
    q / Ctrl+C Quit Release Desk TUI

  Tab Specific:
    r          (Reconcile) Restore DB from backup data.json
    b          (Reconcile) Export DB to backup data.json
    s          (Repos) Trigger fast-forward sync
    t          (Repos) Toggle repository tracking
"#;

    let block = Block::default()
        .title(" Help — Keyboard Shortcuts ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    let paragraph = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(Color::White));

    frame.render_widget(paragraph, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
