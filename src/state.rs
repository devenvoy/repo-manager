//! Desktop application state and navigation models.

use crate::settings::HubViewMode;
use release_desk_application::use_case::app_store::AppStoreMetrics;
use release_desk_core::app_catalog::AppStoreItem;
use release_desk_core::project::{ProjectId, ProjectIdentity, SelectableRepo};
use release_desk_core::snapshot::{MismatchReport, Repository};
use std::path::PathBuf;

/// Active workspace tabs in the desktop interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceTab {
    AppStore,
    ProjectHub,
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
            Self::AppStore => "App Store",
            Self::ProjectHub => "Project Hub",
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
    pub app_store_items: Vec<AppStoreItem>,
    pub selected_app_id: Option<String>,
    pub app_store_metrics: AppStoreMetrics,
    pub hub_view_mode: HubViewMode,
    pub terms_accepted: bool,
    pub show_onboarding_modal: bool,
    pub show_new_project_modal: bool,
    pub show_ingest_source_modal: bool,
    pub ingest_target_project_id: Option<ProjectId>,
    pub selectable_repos: Vec<SelectableRepo>,
    pub install_progress_pct: Option<u8>,
}

impl Default for DesktopState {
    fn default() -> Self {
        Self {
            active_tab: WorkspaceTab::AppStore,
            active_project: None,
            projects: Vec::new(),
            repositories: Vec::new(),
            selected_repo_index: None,
            search_query: String::new(),
            mismatch_report: None,
            status_message: "Ready".to_string(),
            library_path: None,
            app_store_items: Vec::new(),
            selected_app_id: None,
            app_store_metrics: AppStoreMetrics::default(),
            hub_view_mode: HubViewMode::Grouped,
            terms_accepted: false,
            show_onboarding_modal: false,
            show_new_project_modal: false,
            show_ingest_source_modal: false,
            ingest_target_project_id: None,
            selectable_repos: Vec::new(),
            install_progress_pct: None,
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

    pub fn select_all_repos(&mut self, select: bool) {
        for repo in &mut self.selectable_repos {
            repo.selected = select;
        }
    }

    pub fn toggle_repo_selection(&mut self, repo_name: &str) {
        if let Some(repo) = self
            .selectable_repos
            .iter_mut()
            .find(|r| r.name == repo_name)
        {
            repo.selected = !repo.selected;
        }
    }
}
