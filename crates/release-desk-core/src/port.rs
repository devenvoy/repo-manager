//! Inversion-of-control domain port definitions.
//!
//! These traits define abstract boundaries for infrastructure adapters without
//! tying the domain to concrete libraries, databases, or operating systems.

use crate::model::{
    AccountIdentity, GitSyncState, IdempotencyKey, JobId, JobKind, JobPriority, JobState,
    ProjectId, ProjectIdentity, PullRequest, Release, RepositoryIdentity,
};
use crate::provider::{AuthCallbackStatus, GitProviderKind, WebviewWindowSpec};
use crate::snapshot::{MismatchReport, Repository, Snapshot};
use std::path::{Path, PathBuf};

pub use crate::error::DomainError;

/// Abstract Git hosting service gateway (GitHub, GitLab, Bitbucket, Atlassian).
pub trait GitHostGateway: Send + Sync {
    /// Return the provider kind this gateway serves.
    fn provider_kind(&self) -> GitProviderKind {
        GitProviderKind::GitHub
    }

    /// List repositories owned by the given account.
    fn list_repositories(
        &self,
        account: &AccountIdentity,
    ) -> Result<Vec<RepositoryIdentity>, DomainError>;

    /// Fetch pull requests for a repository.
    fn fetch_pull_requests(
        &self,
        repo: &RepositoryIdentity,
    ) -> Result<Vec<PullRequest>, DomainError>;

    /// Fetch releases and their assets for a repository.
    fn fetch_releases(&self, repo: &RepositoryIdentity) -> Result<Vec<Release>, DomainError>;
}

/// Backwards-compatible alias for GitHub gateway.
pub use GitHostGateway as GitHubGateway;

/// Abstract system default browser launcher for OAuth login.
pub trait BrowserAuthPort: Send + Sync {
    /// Attempt to launch the default system browser with the specified URL.
    fn open_url(&self, url: &str) -> Result<(), DomainError>;
}

/// Abstract custom webview window launcher for modal OAuth authentication.
pub trait WebviewAuthPort: Send + Sync {
    /// Open a modal webview window panel with the specified spec,
    /// automatically closing the window when auth succeeds, fails, or is cancelled.
    fn open_auth_window(&self, spec: &WebviewWindowSpec)
    -> Result<AuthCallbackStatus, DomainError>;
}

/// Abstract system time provider for deterministic testing.
pub trait ClockPort: Send + Sync {
    /// Return current UTC timestamp in seconds since UNIX epoch.
    fn now_utc_epoch_secs(&self) -> u64;

    /// Return current UTC timestamp formatted as an RFC 3339 string.
    fn now_utc_rfc3339(&self) -> String;
}

/// Abstract OS secure secret storage (Keychain / Credential Manager).
pub trait SecretStorePort: Send + Sync {
    /// Retrieve a secret by key. Returns `None` if absent.
    fn get_secret(&self, key: &str) -> Result<Option<String>, DomainError>;

    /// Store a secret by key.
    fn set_secret(&self, key: &str, secret: &str) -> Result<(), DomainError>;

    /// Delete a secret by key.
    fn delete_secret(&self, key: &str) -> Result<(), DomainError>;
}

/// Abstract Git repository operations.
pub trait GitPort: Send + Sync {
    /// Inspect the local repository status relative to its default branch.
    fn inspect_clone(&self, path: &Path, default_branch: &str)
    -> Result<GitSyncState, DomainError>;

    /// Fetch remote changes and prune dead branches without modifying the working tree.
    fn fetch_origin(&self, path: &Path) -> Result<(), DomainError>;

    /// Fast-forward a clean default branch to its fetched remote head.
    fn fast_forward(&self, path: &Path, default_branch: &str) -> Result<GitSyncState, DomainError>;

    /// Clone a repository from remote URL into the given destination path.
    fn clone_repo(
        &self,
        remote_url: &str,
        destination: &Path,
        default_branch: &str,
    ) -> Result<String, DomainError>;
}

/// Abstract project persistence port.
pub trait ProjectStore: Send + Sync {
    /// List all registered projects.
    fn list_projects(&self) -> Result<Vec<ProjectIdentity>, DomainError>;

    /// Retrieve a single project by ID.
    fn get_project(&self, id: ProjectId) -> Result<Option<ProjectIdentity>, DomainError>;

    /// Save or update a project record.
    fn save_project(&self, project: &ProjectIdentity) -> Result<(), DomainError>;

    /// Remove a project by ID.
    fn delete_project(&self, id: ProjectId) -> Result<(), DomainError>;
}

/// Abstract asynchronous job queue port.
pub trait JobStore: Send + Sync {
    /// Enqueue a new job if not already present by idempotency key.
    fn enqueue_job(
        &self,
        kind: JobKind,
        priority: JobPriority,
        idempotency: &IdempotencyKey,
    ) -> Result<JobId, DomainError>;

    /// Update the state of an existing job.
    fn update_job_state(&self, id: JobId, state: JobState) -> Result<(), DomainError>;
}

/// Abstract OS scheduler service registration port.
pub trait SchedulerPort: Send + Sync {
    /// Register the background daemon service with the OS (launchd/systemd).
    fn register_daemon(&self) -> Result<(), DomainError>;

    /// Unregister the background daemon service from the OS.
    fn unregister_daemon(&self) -> Result<(), DomainError>;
}

/// Abstract archive packaging port for portable backups.
pub trait ArchivePort: Send + Sync {
    /// Create a compressed archive of `source_dir` at `output_file`, returning byte size.
    fn create_archive(&self, source_dir: &Path, output_file: &Path) -> Result<u64, DomainError>;

    /// Extract an archive into `target_dir` with traversal protection.
    fn extract_archive(&self, archive_file: &Path, target_dir: &Path) -> Result<(), DomainError>;
}

/// Abstract project local backup and mismatch reconciliation port.
pub trait ProjectBackupPort: Send + Sync {
    /// Write or update the local `data.json` backup snapshot inside the project directory.
    fn write_backup_snapshot(
        &self,
        project_root: &Path,
        snapshot: &Snapshot,
    ) -> Result<PathBuf, DomainError>;

    /// Read the local `data.json` backup snapshot if it exists.
    fn read_backup_snapshot(&self, project_root: &Path) -> Result<Option<Snapshot>, DomainError>;

    /// Compare the local `.release-desk/data.json` backup snapshot against the database records.
    fn detect_mismatch(
        &self,
        project_root: &Path,
        db_repos: &[Repository],
    ) -> Result<MismatchReport, DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockClock(u64);

    impl ClockPort for MockClock {
        fn now_utc_epoch_secs(&self) -> u64 {
            self.0
        }

        fn now_utc_rfc3339(&self) -> String {
            "2026-10-27T00:00:00Z".into()
        }
    }

    #[test]
    fn clock_port_mock() {
        let clock = MockClock(1700000000);
        assert_eq!(clock.now_utc_epoch_secs(), 1700000000);
        assert_eq!(clock.now_utc_rfc3339(), "2026-10-27T00:00:00Z");
    }

    #[test]
    fn domain_error_display() {
        let err = DomainError::NotFound {
            entity: "Project",
            id: "123".into(),
        };
        assert_eq!(err.to_string(), "Project not found: 123");
    }
}
