//! Modular view routers and layout manager for the TUI.

pub mod auth;
pub mod help;
pub mod installers;
pub mod projects;
pub mod pull_requests;
pub mod reconcile;
pub mod repositories;

use crate::app::{AppState, NavTab};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

/// Render the complete top-level TUI interface.
pub fn render_ui(frame: &mut Frame, state: &AppState) {
    let size = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header tabs
            Constraint::Length(3), // Search / filter bar
            Constraint::Min(10),   // Active tab body
            Constraint::Length(1), // Footer status bar
        ])
        .split(size);

    // 1. Navigation Header Tabs
    let tab_titles: Vec<Line> = NavTab::ALL.iter().map(|t| Line::from(t.title())).collect();

    let tabs = Tabs::new(tab_titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Release Desk — Native Git Release Manager "),
        )
        .select(state.active_tab as usize)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        );

    frame.render_widget(tabs, chunks[0]);

    // 2. Search / Filter bar
    let search_style = if state.is_searching {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let search_prompt = if state.is_searching {
        "Search [/ to type, Enter/Esc to finish]: "
    } else if state.search_query.is_empty() {
        "Press [/] to filter repositories"
    } else {
        "Filter active: "
    };

    let search_line = Line::from(vec![
        Span::styled(search_prompt, search_style),
        Span::styled(
            &state.search_query,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let search_block = Block::default().borders(Borders::ALL).style(search_style);

    let search_bar = Paragraph::new(search_line).block(search_block);
    frame.render_widget(search_bar, chunks[1]);

    // 3. Active Tab Body
    match state.active_tab {
        NavTab::Repositories => repositories::render_repositories(frame, chunks[2], state),
        NavTab::Installers => installers::render_installers(frame, chunks[2], state),
        NavTab::PullRequests => pull_requests::render_pull_requests(frame, chunks[2], state),
        NavTab::Reconcile => reconcile::render_reconcile(frame, chunks[2], state),
        NavTab::Projects => projects::render_projects(frame, chunks[2], state),
        NavTab::Auth => auth::render_auth(frame, chunks[2], state),
    }

    // 4. Footer Status Bar
    let proj_label = state
        .active_project
        .as_ref()
        .map(|p| p.name.as_str())
        .unwrap_or("No Active Project");

    let status_text = state.status_message.as_deref().unwrap_or("Ready");

    let footer_line = Line::from(vec![
        Span::styled(
            format!(" [Project: {}] ", proj_label),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(" "),
        Span::styled(status_text, Style::default().fg(Color::White)),
    ]);

    let footer = Paragraph::new(footer_line);
    frame.render_widget(footer, chunks[3]);

    // 5. Help Modal Overlay (if open)
    if state.show_help {
        help::render_help(frame, size);
    }
}
