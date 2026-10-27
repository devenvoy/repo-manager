//! Headless TUI rendering and interaction tests using Ratatui TestBackend.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use release_desk_core::git::{BranchRef, Revision};
use release_desk_core::project::{ProjectId, ProjectIdentity, ProjectName};
use release_desk_core::snapshot::{MismatchReport, Repository, RevisionMismatch};
use release_desk_tui::app::{AppState, NavTab};
use release_desk_tui::views::render_ui;
use std::collections::HashMap;
use std::path::PathBuf;

fn create_sample_state() -> AppState {
    let mut state = AppState::default();

    let proj_id = ProjectId::from_u128(100);
    let proj_name = ProjectName::new("Test-Project").unwrap();
    let project = ProjectIdentity::with_clone_location(
        proj_id,
        proj_name,
        Some(PathBuf::from("/tmp/test-clone")),
    );

    state.projects = vec![project.clone()];
    state.active_project = Some(project);

    state.repositories = vec![
        Repository {
            name: "app-core".into(),
            description: Some("Core domain logic and algorithms".into()),
            url: "https://github.com/org/app-core".into(),
            is_private: false,
            is_archived: false,
            pushed_at: None,
            primary_language: None,
            default_branch_ref: Some(BranchRef {
                name: "main".into(),
                target: Revision {
                    oid: "abc123456789".into(),
                },
            }),
            releases: None,
        },
        Repository {
            name: "app-cli".into(),
            description: Some("Command line tool".into()),
            url: "https://github.com/org/app-cli".into(),
            is_private: false,
            is_archived: false,
            pushed_at: None,
            primary_language: None,
            default_branch_ref: Some(BranchRef {
                name: "main".into(),
                target: Revision {
                    oid: "def987654321".into(),
                },
            }),
            releases: None,
        },
    ];

    let mut auth_map = HashMap::new();
    auth_map.insert("GitHub".into(), true);
    auth_map.insert("GitLab".into(), false);
    state.auth_status = auth_map;

    state
}

#[test]
fn headless_render_all_tabs_without_panic() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = create_sample_state();

    for tab in NavTab::ALL {
        state.active_tab = tab;
        terminal
            .draw(|f| render_ui(f, &state))
            .expect("render succeeds");
        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
    }
}

#[test]
fn search_filters_repositories_interactively() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = create_sample_state();

    assert_eq!(state.filtered_repositories().len(), 2);

    // Type query: "core"
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(state.filtered_repositories().len(), 1);
    assert_eq!(state.filtered_repositories()[0].name, "app-core");

    terminal.draw(|f| render_ui(f, &state)).unwrap();
}

#[test]
fn reconcile_panel_renders_mismatch_and_actions() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = create_sample_state();
    state.active_tab = NavTab::Reconcile;

    state.mismatch_report = Some(MismatchReport {
        project_root: PathBuf::from("/tmp/my-project"),
        disk_snapshot_present: true,
        matched_repositories: vec!["repo1".into()],
        missing_in_database: vec!["extra-backup-repo".into()],
        missing_on_disk: vec![],
        revision_mismatches: vec![RevisionMismatch {
            repository: "repo1".into(),
            disk_revision: Some("rev1".into()),
            db_revision: Some("rev2".into()),
        }],
    });

    terminal.draw(|f| render_ui(f, &state)).unwrap();

    let buffer = terminal.backend().buffer();
    let text = format!("{:?}", buffer);
    assert!(text.contains("MISMATCH DETECTED") || text.contains("Mismatch"));
    assert!(text.contains("Restore Database") || text.contains("[r]"));
}

#[test]
fn help_modal_overlay_toggles_and_dismisses() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = create_sample_state();

    assert!(!state.show_help);

    // Press '?' to show help
    state.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(state.show_help);

    terminal.draw(|f| render_ui(f, &state)).unwrap();

    // Press Esc to dismiss
    state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!state.show_help);
}
