//! Desktop GUI workspace views.

pub mod hub;
pub mod installers;
pub mod onboarding;
pub mod overview;
pub mod project_repository_status;
pub mod project_setup;
pub mod project_sources;
pub mod project_workspace;
pub mod pull_requests;
pub mod reconcile;
pub mod repositories;
pub mod settings;
pub mod store;
pub mod welcome;

pub use hub::render_hub;
pub use installers::render_installers;
pub use onboarding::render_onboarding;
pub use overview::{OverviewMetrics, render_overview};
pub use project_setup::{ProjectSetupActions, ProjectSetupProps, render_project_setup};
pub use project_sources::{
    render_clone_review_modal, render_project_repositories, render_project_sources,
};
pub use project_workspace::render_project_workspace;
pub use pull_requests::render_pull_requests;
pub use reconcile::render_reconcile;
pub use repositories::render_repositories;
pub use settings::render_settings;
pub use store::render_store;
pub use welcome::render_welcome;

pub use project_repository_status::render_project_repository_status;

pub mod project_releases;
pub use project_releases::render_live_releases;

pub mod import_dialog;
pub use import_dialog::{ImportDialogState, render_import_dialog};
