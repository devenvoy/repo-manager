//! Application use case modules.
//!
//! Each area is partitioned into a separate file for clear responsibilities.

pub mod auth;
pub mod discovery;
pub mod installer;
pub mod migration;
pub mod project;
pub mod pull_request;
pub mod reconcile;
pub mod sync;

pub use auth::{AuthResult, AuthenticateCommand, AuthenticateProviderUseCase, OAuthConfig};

pub use discovery::{
    DiscoverRepositoriesUseCase, DiscoveredRepository, InMemoryTrackingStore,
    ProjectRepositoryRecord, SetTrackingPreferencesUseCase, TrackingStatus, TrackingStore,
};
pub use installer::{
    CheckInstallerStatusUseCase, CheckInstallerUpdatesUseCase, DownloadAndPlanInstallerUseCase,
    DownloadInstallerUseCase, DownloadPort, InstallAssetCommand, InstallAssetOutcome,
};
pub use migration::{
    AnalyzeLegacySetupUseCase, ExecuteLegacyMigrationUseCase, PreviewMigrationPlanUseCase,
};
pub use project::{CreateProjectCommand, CreateProjectUseCase, ListProjectsUseCase};
pub use pull_request::{
    InMemoryPullRequestStore, ListPullRequestsUseCase, PullRequestFilter, PullRequestStore,
    SyncPullRequestsUseCase,
};
pub use reconcile::ReconcileProjectSnapshotUseCase;
pub use sync::{SyncAllRepositoriesUseCase, SyncOutcome, SyncRepositoryUseCase, SyncTarget};
