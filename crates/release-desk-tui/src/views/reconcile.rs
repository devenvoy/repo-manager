//! Local backup snapshot reconciliation and disaster recovery panel.

use crate::app::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub fn render_reconcile(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(8)])
        .split(area);

    let report_opt = &state.mismatch_report;

    // Header Status Card
    let (status_text, status_color) = match report_opt {
        Some(report) if report.is_synchronized() => (
            "SYNCHRONIZED: Database and local .release-desk/data.json backup match perfectly.",
            Color::Green,
        ),
        Some(_) => (
            "MISMATCH DETECTED: Discrepancy found between SQLite database and local backup snapshot.",
            Color::Yellow,
        ),
        None => (
            "Backup status: checking or no local backup snapshot recorded.",
            Color::DarkGray,
        ),
    };

    let header_block = Block::default()
        .borders(Borders::ALL)
        .title(" Local Backup Snapshot Status ")
        .style(Style::default().fg(status_color));

    let header_lines = vec![
        Line::from(Span::styled(
            status_text,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(
            "Every project clone location automatically maintains a local data.json backup for disaster recovery.",
        ),
    ];

    let header_paragraph = Paragraph::new(header_lines).block(header_block);
    frame.render_widget(header_paragraph, chunks[0]);

    // Detail Panel
    let details_block = Block::default()
        .borders(Borders::ALL)
        .title(" Discrepancy Breakdown & Actions ")
        .style(Style::default().fg(Color::Cyan));

    let detail_text = if let Some(report) = report_opt {
        let matched_count = report.matched_repositories.len();
        let missing_in_db = if report.missing_in_database.is_empty() {
            "None (0)".to_string()
        } else {
            format!(
                "{} repo(s): {:?}",
                report.missing_in_database.len(),
                report.missing_in_database
            )
        };
        let missing_on_disk = if report.missing_on_disk.is_empty() {
            "None (0)".to_string()
        } else {
            format!(
                "{} repo(s): {:?}",
                report.missing_on_disk.len(),
                report.missing_on_disk
            )
        };
        let rev_mismatches = if report.revision_mismatches.is_empty() {
            "None (0)".to_string()
        } else {
            format!("{} repo(s) diverged", report.revision_mismatches.len())
        };

        format!(
            "Project Root:                  {}\n\
             Disk Snapshot Present:         {}\n\
             Matched Repositories:          {}\n\
             Missing in Database (Backup Only): {}\n\
             Missing on Disk (DB Only):     {}\n\
             Head Revision Divergences:     {}\n\n\
             Disaster Recovery & Sync Commands:\n\
               [r]  Restore Database from Backup  (Import missing repos from data.json into DB)\n\
               [b]  Sync Database to Backup       (Export current DB tracked repos to data.json)\n\
               [c]  Refresh / Recheck Mismatches",
            report.project_root.display(),
            if report.disk_snapshot_present {
                "Yes"
            } else {
                "No"
            },
            matched_count,
            missing_in_db,
            missing_on_disk,
            rev_mismatches
        )
    } else {
        "Select a project with a clone location to inspect and reconcile its local backup snapshot."
            .to_string()
    };

    let details_paragraph = Paragraph::new(detail_text)
        .block(details_block)
        .wrap(Wrap { trim: true });

    frame.render_widget(details_paragraph, chunks[1]);
}
