//! Domain models re-exported for ergonomic use and backwards compatibility.

pub use crate::app_catalog::{
    AppActionState, AppMetadata, AppStoreItem, default_artcraft_catalog, find_known_app,
};
pub use crate::error::{DomainError, ValidationError};
pub use crate::git::{BranchRef, GitSyncState, Revision};
pub use crate::installer::{
    Asset, AssetConnection, InstallerExtension, InstallerObservation, InstallerStatus,
    RetentionPlanner, RotationPlan,
};
pub use crate::job::{
    DownloadProgress, IdempotencyKey, JobId, JobKind, JobPriority, JobState, RetryBackoff,
};
pub use crate::project::{
    AccountIdentity, AccountKind, AccountLogin, DiskLayout, ProfileSource, ProjectId,
    ProjectIdentity, ProjectName, SelectableRepo, TrackingPreferences,
};
pub use crate::pull_request::{PullRequest, PullRequestLabel, PullRequestNumber, PullRequestState};
pub use crate::release::{Architecture, Release, ReleaseConnection, ReleaseTag, latest_installer};
pub use crate::repository::{GitHubRepositoryId, RepositoryIdentity, RepositoryName};
pub use crate::schedule::{MissedRunPolicy, Recurrence, ScheduleCalculator, ScheduleId};
pub use crate::snapshot::{Language, Repository, Snapshot, parse_snapshot};
pub use crate::storage::{BackupId, BackupMode, FileSize, RelativeManagedPath, Sha256Checksum};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exports_are_accessible() {
        let name = ProjectName::new("Test").unwrap();
        assert_eq!(name.as_str(), "Test");
        let ext = InstallerExtension::new("dmg").unwrap();
        assert_eq!(ext.as_str(), "dmg");
    }
}
