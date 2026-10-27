//! Pure domain types, deterministic rules, and abstract ports for Release Desk.

pub mod error;
pub mod git;
pub mod installer;
pub mod job;
pub mod model;
pub mod port;
pub mod project;
pub mod provider;
pub mod pull_request;
pub mod release;
pub mod repository;
pub mod schedule;
pub mod service;
pub mod snapshot;
pub mod storage;

// Top-level re-exports for clean, direct domain usage
pub use error::{DomainError, ValidationError};
pub use git::GitSyncState;
pub use installer::{Asset, AssetConnection, InstallerExtension, InstallerStatus};
pub use project::{
    AccountIdentity, AccountKind, AccountLogin, ProjectId, ProjectIdentity, ProjectName,
    TrackingPreferences,
};
pub use provider::{
    AuthCallbackStatus, AuthMethod, GitProviderKind, ProviderAccount, WebviewWindowSpec,
};
pub use pull_request::{PullRequest, PullRequestLabel, PullRequestNumber, PullRequestState};
pub use release::{Architecture, Release, ReleaseConnection, ReleaseTag, latest_installer};
pub use repository::{GitHubRepositoryId, RepositoryIdentity, RepositoryName};
pub use snapshot::{Language, Repository, Snapshot, parse_snapshot};
pub use storage::{BackupId, BackupMode, FileSize, RelativeManagedPath, Sha256Checksum};
