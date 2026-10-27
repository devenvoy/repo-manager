//! In-memory test fakes for domain ports.
//!
//! Provides isolated, deterministic implementations for testing application use cases
//! without touching disk, network, or OS services.

use crate::use_case::installer::DownloadPort;
use release_desk_core::error::DomainError;
use release_desk_core::git::GitSyncState;
use release_desk_core::port::{ClockPort, GitHubGateway, GitPort, ProjectStore, SecretStorePort};
use release_desk_core::project::{AccountIdentity, AccountLogin, ProjectId, ProjectIdentity};
use release_desk_core::pull_request::PullRequest;
use release_desk_core::release::Release;
use release_desk_core::repository::{GitHubRepositoryId, RepositoryIdentity};
use release_desk_core::storage::{RelativeManagedPath, Sha256Checksum};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Injectable, thread-safe simulated clock.
#[derive(Debug)]
pub struct FakeClock {
    now: AtomicU64,
}

impl FakeClock {
    pub fn new(epoch_secs: u64) -> Self {
        Self {
            now: AtomicU64::new(epoch_secs),
        }
    }

    pub fn advance_seconds(&self, seconds: u64) {
        self.now.fetch_add(seconds, Ordering::Relaxed);
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new(1_700_000_000)
    }
}

impl ClockPort for FakeClock {
    fn now_utc_epoch_secs(&self) -> u64 {
        self.now.load(Ordering::Relaxed)
    }

    fn now_utc_rfc3339(&self) -> String {
        format!("epoch:{}", self.now_utc_epoch_secs())
    }
}

/// In-memory project store for use case integration tests.
#[derive(Default, Debug)]
pub struct FakeProjectStore {
    projects: RwLock<Vec<ProjectIdentity>>,
}

impl FakeProjectStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectStore for FakeProjectStore {
    fn list_projects(&self) -> Result<Vec<ProjectIdentity>, DomainError> {
        let guard = self.projects.read().unwrap();
        Ok(guard.clone())
    }

    fn get_project(&self, id: ProjectId) -> Result<Option<ProjectIdentity>, DomainError> {
        let guard = self.projects.read().unwrap();
        Ok(guard.iter().find(|p| p.id == id).cloned())
    }

    fn save_project(&self, project: &ProjectIdentity) -> Result<(), DomainError> {
        let mut guard = self.projects.write().unwrap();
        if let Some(pos) = guard.iter().position(|p| p.id == project.id) {
            guard[pos] = project.clone();
        } else {
            guard.push(project.clone());
        }
        Ok(())
    }

    fn delete_project(&self, id: ProjectId) -> Result<(), DomainError> {
        let mut guard = self.projects.write().unwrap();
        guard.retain(|p| p.id != id);
        Ok(())
    }
}

/// In-memory Git port simulating various clone states.
#[derive(Default, Debug)]
pub struct FakeGitPort {
    states: RwLock<HashMap<PathBuf, GitSyncState>>,
}

impl FakeGitPort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_state(&self, path: impl Into<PathBuf>, state: GitSyncState) {
        self.states.write().unwrap().insert(path.into(), state);
    }
}

impl GitPort for FakeGitPort {
    fn inspect_clone(
        &self,
        path: &Path,
        _default_branch: &str,
    ) -> Result<GitSyncState, DomainError> {
        let guard = self.states.read().unwrap();
        Ok(guard.get(path).cloned().unwrap_or(GitSyncState::NotCloned))
    }

    fn fetch_origin(&self, _path: &Path) -> Result<(), DomainError> {
        Ok(())
    }

    fn fast_forward(
        &self,
        path: &Path,
        _default_branch: &str,
    ) -> Result<GitSyncState, DomainError> {
        let mut guard = self.states.write().unwrap();
        let new_state = GitSyncState::Current {
            revision: "updated_commit_hash".into(),
        };
        guard.insert(path.to_path_buf(), new_state.clone());
        Ok(new_state)
    }

    fn clone_repo(
        &self,
        _remote_url: &str,
        destination: &Path,
        _default_branch: &str,
    ) -> Result<String, DomainError> {
        let mut guard = self.states.write().unwrap();
        guard.insert(
            destination.to_path_buf(),
            GitSyncState::Current {
                revision: "cloned_commit_hash".into(),
            },
        );
        Ok("cloned_commit_hash".into())
    }
}

/// In-memory GitHub gateway simulating repositories, PRs, and releases.
#[derive(Default, Debug)]
pub struct FakeGitHubGateway {
    repos: RwLock<HashMap<AccountLogin, Vec<RepositoryIdentity>>>,
    pull_requests: RwLock<HashMap<GitHubRepositoryId, Vec<PullRequest>>>,
    releases: RwLock<HashMap<GitHubRepositoryId, Vec<Release>>>,
}

impl FakeGitHubGateway {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_repository(&self, account: AccountLogin, repo: RepositoryIdentity) {
        self.repos
            .write()
            .unwrap()
            .entry(account)
            .or_default()
            .push(repo);
    }

    pub fn add_pull_request(&self, repo_id: GitHubRepositoryId, pr: PullRequest) {
        self.pull_requests
            .write()
            .unwrap()
            .entry(repo_id)
            .or_default()
            .push(pr);
    }

    pub fn add_release(&self, repo_id: GitHubRepositoryId, release: Release) {
        self.releases
            .write()
            .unwrap()
            .entry(repo_id)
            .or_default()
            .push(release);
    }
}

impl GitHubGateway for FakeGitHubGateway {
    fn list_repositories(
        &self,
        account: &AccountIdentity,
    ) -> Result<Vec<RepositoryIdentity>, DomainError> {
        let guard = self.repos.read().unwrap();
        Ok(guard.get(&account.login).cloned().unwrap_or_default())
    }

    fn fetch_pull_requests(
        &self,
        repo: &RepositoryIdentity,
    ) -> Result<Vec<PullRequest>, DomainError> {
        let guard = self.pull_requests.read().unwrap();
        Ok(guard.get(&repo.id).cloned().unwrap_or_default())
    }

    fn fetch_releases(&self, repo: &RepositoryIdentity) -> Result<Vec<Release>, DomainError> {
        let guard = self.releases.read().unwrap();
        Ok(guard.get(&repo.id).cloned().unwrap_or_default())
    }
}

/// In-memory downloader recording downloads and returning deterministic checksums.
#[derive(Default, Debug)]
pub struct FakeDownloadPort {
    downloads: RwLock<Vec<(String, u64, RelativeManagedPath)>>,
    canned_checksums: RwLock<HashMap<String, Sha256Checksum>>,
}

impl FakeDownloadPort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_checksum(&self, url: impl Into<String>, checksum: Sha256Checksum) {
        self.canned_checksums
            .write()
            .unwrap()
            .insert(url.into(), checksum);
    }

    pub fn downloads(&self) -> Vec<(String, u64, RelativeManagedPath)> {
        self.downloads.read().unwrap().clone()
    }
}

impl DownloadPort for FakeDownloadPort {
    fn download_to_file(
        &self,
        url: &str,
        expected_size: u64,
        destination: &RelativeManagedPath,
    ) -> Result<Sha256Checksum, DomainError> {
        self.downloads
            .write()
            .unwrap()
            .push((url.to_owned(), expected_size, destination.clone()));

        if let Some(cs) = self.canned_checksums.read().unwrap().get(url) {
            Ok(cs.clone())
        } else {
            Sha256Checksum::new("0000000000000000000000000000000000000000000000000000000000000000")
                .map_err(DomainError::from)
        }
    }
}

/// In-memory secret store for testing.
#[derive(Default, Debug)]
pub struct FakeSecretStore {
    secrets: RwLock<HashMap<String, String>>,
}

impl FakeSecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStorePort for FakeSecretStore {
    fn get_secret(&self, key: &str) -> Result<Option<String>, DomainError> {
        Ok(self.secrets.read().unwrap().get(key).cloned())
    }

    fn set_secret(&self, key: &str, secret: &str) -> Result<(), DomainError> {
        self.secrets
            .write()
            .unwrap()
            .insert(key.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete_secret(&self, key: &str) -> Result<(), DomainError> {
        self.secrets.write().unwrap().remove(key);
        Ok(())
    }
}
