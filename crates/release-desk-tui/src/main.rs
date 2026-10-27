//! Release Desk interactive Terminal User Interface entry point.

use release_desk_core::ProjectBackupPort;
use release_desk_core::port::ProjectStore;
use release_desk_store::boundary::ManagedLibrary;
use release_desk_store::project_backup::ProjectSnapshotManager;
use release_desk_store::sqlite_store::SqliteStore;
use release_desk_tui::app::AppState;
use release_desk_tui::event::{EventHandler, TuiEvent};
use release_desk_tui::terminal::TerminalGuard;
use release_desk_tui::views::render_ui;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const DEFAULT_TICK_RATE: Duration = Duration::from_millis(250);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("release-desk-tui: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    // 1. Resolve library root and open store
    let library_dir = resolve_library_path();
    fs::create_dir_all(&library_dir)?;
    let library = ManagedLibrary::new(&library_dir)?;
    let db_path = library_dir.join("release-desk.sqlite3");
    let store = SqliteStore::open(&db_path, library.clone())?;

    // 2. Initialize application state
    let mut state = AppState::default();
    let projects = store.list_projects().unwrap_or_default();
    state.projects = projects.clone();

    if let Some(first) = projects.first() {
        state.active_project = Some(first.clone());

        // Check local backup snapshot if clone location exists
        if let Some(ref clone_loc) = first.clone_location {
            let backup_mgr = ProjectSnapshotManager::new();
            if let Ok(report) = backup_mgr.detect_mismatch(clone_loc, &[]) {
                state.mismatch_report = Some(report);
            }

            // Load backup snapshot repos if present
            if let Ok(Some(snap)) = backup_mgr.read_backup_snapshot(clone_loc) {
                state.repositories = snap.repos;
            }
        }
    }

    // Provider authentication statuses
    let mut auth_map = HashMap::new();
    auth_map.insert("GitHub".into(), true);
    auth_map.insert("GitLab".into(), false);
    auth_map.insert("Bitbucket".into(), false);
    state.auth_status = auth_map;

    // 3. Initialize terminal with RAII guard
    let (mut terminal, mut guard) = TerminalGuard::init()?;
    let mut events = EventHandler::new(DEFAULT_TICK_RATE);

    // 4. Main Event Loop
    while !state.should_quit {
        terminal.draw(|f| render_ui(f, &state))?;

        match events.next()? {
            TuiEvent::Key(key) => {
                if state.handle_key(key) {
                    break;
                }
            }
            TuiEvent::Tick => {
                // Periodic background updates or refresh
            }
            TuiEvent::Resize(_, _) => {}
        }
    }

    // Explicitly restore terminal
    guard.restore()?;
    Ok(())
}

fn resolve_library_path() -> PathBuf {
    if let Ok(env_path) = env::var("RELEASE_DESK_LIBRARY") {
        let p = PathBuf::from(env_path);
        if p.is_absolute() {
            return p;
        }
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".release-desk/library");
    }
    PathBuf::from(".release-desk-library")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn keyboard_navigation_and_search_are_bounded() {
        let mut state = AppState::default();
        assert!(!state.should_quit);

        // Enter search
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(state.is_searching);

        // Type query
        state.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(state.search_query, "app");

        // Finish search
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!state.is_searching);

        // Switch to Reconcile tab
        state.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
        assert_eq!(state.active_tab, release_desk_tui::app::NavTab::Reconcile);

        // Quit with 'q'
        state.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(state.should_quit);
    }
}
