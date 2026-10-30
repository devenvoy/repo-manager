//! Application use case modules.
//!
//! Each area is partitioned into a separate file for clear responsibilities.

pub mod app_store;
pub mod auth;
pub mod discovery;
pub mod installer;
pub mod macos_installer;
pub mod profile_ingestion;
pub mod project;
pub mod pull_request;
pub mod reconcile;
pub mod sync;

pub use app_store::{
    AppInstallationProbe, AppStoreAggregatorUseCase, AppStoreMetrics, FsAppInstallationProbe,
};
pub use auth::{AuthResult, AuthenticateCommand, AuthenticateProviderUseCase, OAuthConfig};

pub use discovery::{
    DiscoverRepositoriesUseCase, DiscoveredRepository, InMemoryTrackingStore,
    ProjectRepositoryRecord, SetTrackingPreferencesUseCase, TrackingStatus, TrackingStore,
};
pub use installer::{
    CheckInstallerStatusUseCase, CheckInstallerUpdatesUseCase, DownloadAndPlanInstallerUseCase,
    DownloadInstallerUseCase, DownloadPort, InstallAssetCommand, InstallAssetOutcome,
};
pub use macos_installer::{DiskImagePort, HdiutilAdapter, InstallMacOsAppUseCase, InstallStep};
pub use profile_ingestion::{
    DiscoveredRepoItem, IngestProfileUseCase, ParsedProfileRef, RepoClonePlan, parse_profile_input,
};
pub use project::{CreateProjectCommand, CreateProjectUseCase, ListProjectsUseCase};
pub use pull_request::{
    InMemoryPullRequestStore, ListPullRequestsUseCase, PullRequestFilter, PullRequestStore,
    SyncPullRequestsUseCase,
};
pub use reconcile::ReconcileProjectSnapshotUseCase;
pub use sync::{SyncAllRepositoriesUseCase, SyncOutcome, SyncRepositoryUseCase, SyncTarget};
