//! Desktop application state and navigation models.

use release_desk_core::project::ProjectIdentity;
use release_desk_core::snapshot::{MismatchReport, Repository};
use std::path::PathBuf;

/// Active workspace tabs in the desktop interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceTab {
    Overview,
    Repositories,
    Installers,
    PullRequests,
    Reconcile,
    Settings,
    Welcome,
}

impl WorkspaceTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Repositories => "Repositories",
            Self::Installers => "Installers",
            Self::PullRequests => "Pull Requests",
            Self::Reconcile => "Backup & Reconcile",
            Self::Settings => "Settings",
            Self::Welcome => "Get Started",
        }
    }
}

/// Central state model for the desktop GUI.
#[derive(Clone, Debug)]
pub struct DesktopState {
    pub active_tab: WorkspaceTab,
    pub active_project: Option<ProjectIdentity>,
    pub projects: Vec<ProjectIdentity>,
    pub repositories: Vec<Repository>,
    pub selected_repo_index: Option<usize>,
    pub search_query: String,
    pub mismatch_report: Option<MismatchReport>,
    pub status_message: String,
    pub library_path: Option<PathBuf>,
}

impl Default for DesktopState {
    fn default() -> Self {
        Self {
            active_tab: WorkspaceTab::Overview,
            active_project: None,
            projects: Vec::new(),
            repositories: Vec::new(),
            selected_repo_index: None,
            search_query: String::new(),
            mismatch_report: None,
            status_message: "Ready".to_string(),
            library_path: None,
        }
    }
}

impl DesktopState {
    /// Return repositories matching current search query.
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
}
