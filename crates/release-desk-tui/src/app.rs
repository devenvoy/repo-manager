//! Application state, navigation tabs, and keyboard action dispatch.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use release_desk_core::project::ProjectIdentity;
use release_desk_core::snapshot::{MismatchReport, Repository};
use std::collections::HashMap;

/// Navigation tabs in the main TUI screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavTab {
    Repositories = 0,
    Installers = 1,
    PullRequests = 2,
    Reconcile = 3,
    Projects = 4,
    Auth = 5,
}

impl NavTab {
    pub const ALL: [Self; 6] = [
        Self::Repositories,
        Self::Installers,
        Self::PullRequests,
        Self::Reconcile,
        Self::Projects,
        Self::Auth,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Repositories => "1: Repositories",
            Self::Installers => "2: Installers",
            Self::PullRequests => "3: Pull Requests",
            Self::Reconcile => "4: Backup & Reconcile",
            Self::Projects => "5: Projects",
            Self::Auth => "6: Auth",
        }
    }
}

/// Central application state holding UI controls and domain data.
pub struct AppState {
    pub active_tab: NavTab,
    pub selected_index: usize,
    pub search_query: String,
    pub is_searching: bool,
    pub show_help: bool,
    pub status_message: Option<String>,
    pub should_quit: bool,

    // Domain data
    pub active_project: Option<ProjectIdentity>,
    pub projects: Vec<ProjectIdentity>,
    pub repositories: Vec<Repository>,
    pub mismatch_report: Option<MismatchReport>,
    pub auth_status: HashMap<String, bool>,
    pub sync_statuses: HashMap<String, String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_tab: NavTab::Repositories,
            selected_index: 0,
            search_query: String::new(),
            is_searching: false,
            show_help: false,
            status_message: Some("Press '?' for help, 'q' to quit".to_string()),
            should_quit: false,
            active_project: None,
            projects: Vec::new(),
            repositories: Vec::new(),
            mismatch_report: None,
            auth_status: HashMap::new(),
            sync_statuses: HashMap::new(),
        }
    }
}

impl AppState {
    /// Return filtered repositories matching search query.
    pub fn filtered_repositories(&self) -> Vec<&Repository> {
        let needle = self.search_query.trim().to_lowercase();
        self.repositories
            .iter()
            .filter(|repo| {
                needle.is_empty()
                    || repo.name.to_lowercase().contains(&needle)
                    || repo
                        .description
                        .as_deref()
                        .is_some_and(|d| d.to_lowercase().contains(&needle))
            })
            .collect()
    }

    /// Set or update the status bar message.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Handle key input and return true if action requests application exit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Global exit shortcut
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return true;
        }

        // Search input mode
        if self.is_searching {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.is_searching = false;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.selected_index = 0;
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.selected_index = 0;
                }
                _ => {}
            }
            return false;
        }

        // Help modal dismiss
        if self.show_help {
            self.show_help = false;
            return false;
        }

        // Normal mode navigation
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return true;
            }
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('/') => {
                self.is_searching = true;
                self.search_query.clear();
            }
            KeyCode::Esc => {
                self.search_query.clear();
                self.selected_index = 0;
            }
            // Tab switching
            KeyCode::Char('1') => self.switch_tab(NavTab::Repositories),
            KeyCode::Char('2') => self.switch_tab(NavTab::Installers),
            KeyCode::Char('3') => self.switch_tab(NavTab::PullRequests),
            KeyCode::Char('4') => self.switch_tab(NavTab::Reconcile),
            KeyCode::Char('5') => self.switch_tab(NavTab::Projects),
            KeyCode::Char('6') => self.switch_tab(NavTab::Auth),
            KeyCode::Tab => {
                let current = self.active_tab as usize;
                let next = (current + 1) % NavTab::ALL.len();
                self.switch_tab(NavTab::ALL[next]);
            }
            KeyCode::BackTab => {
                let current = self.active_tab as usize;
                let prev = if current == 0 {
                    NavTab::ALL.len() - 1
                } else {
                    current - 1
                };
                self.switch_tab(NavTab::ALL[prev]);
            }
            // List item selection
            KeyCode::Down | KeyCode::Char('j') => {
                let count = self.active_list_count();
                if count > 0 {
                    self.selected_index = (self.selected_index + 1).min(count - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            KeyCode::Home => self.selected_index = 0,
            KeyCode::End => {
                let count = self.active_list_count();
                if count > 0 {
                    self.selected_index = count - 1;
                }
            }
            _ => {}
        }

        false
    }

    fn switch_tab(&mut self, tab: NavTab) {
        self.active_tab = tab;
        self.selected_index = 0;
        self.set_status(format!("Switched to {}", tab.title()));
    }

    fn active_list_count(&self) -> usize {
        match self.active_tab {
            NavTab::Repositories => self.filtered_repositories().len(),
            NavTab::Installers => self.filtered_repositories().len(),
            NavTab::Projects => self.projects.len(),
            NavTab::PullRequests => 5, // mock or PR count
            NavTab::Reconcile => {
                if let Some(ref r) = self.mismatch_report {
                    r.missing_in_database.len()
                        + r.missing_on_disk.len()
                        + r.revision_mismatches.len()
                } else {
                    0
                }
            }
            NavTab::Auth => self.auth_status.len(),
        }
    }
}
