//! Desktop GUI workspace views.

pub mod hub;
pub mod installers;
pub mod onboarding;
pub mod overview;
pub mod pull_requests;
pub mod reconcile;
pub mod repositories;
pub mod settings;
pub mod store;
pub mod welcome;

pub use hub::{HubViewHandlers, HubViewProps, render_hub};
pub use installers::render_installers;
pub use onboarding::render_onboarding;
pub use overview::{OverviewMetrics, render_overview};
pub use pull_requests::render_pull_requests;
pub use reconcile::render_reconcile;
pub use repositories::render_repositories;
pub use settings::render_settings;
pub use store::render_store;
pub use welcome::render_welcome;
