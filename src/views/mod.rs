//! Desktop GUI workspace views.

pub mod installers;
pub mod overview;
pub mod pull_requests;
pub mod reconcile;
pub mod repositories;
pub mod settings;
pub mod welcome;

pub use installers::render_installers;
pub use overview::{OverviewMetrics, render_overview};
pub use pull_requests::render_pull_requests;
pub use reconcile::render_reconcile;
pub use repositories::render_repositories;
pub use settings::render_settings;
pub use welcome::render_welcome;
