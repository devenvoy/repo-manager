//! Repository discovery and tracking selection use cases.

use release_desk_core::error::DomainError;
use release_desk_core::port::GitHubGateway;
use release_desk_core::project::{AccountIdentity, ProjectId, TrackingPreferences};
use release_desk_core::repository::RepositoryIdentity;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Tracking status of a discovered repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrackingStatus {
    Inbox,
    Tracked(TrackingPreferences),
    Ignored,
}

/// A discovered repository item with its current tracking status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredRepository {
    pub identity: RepositoryIdentity,
    pub status: TrackingStatus,
}

/// A tracked repository record associated with a project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRepositoryRecord {
    pub repo_id: release_desk_core::repository::GitHubRepositoryId,
    pub repo_name: String,
    pub status: TrackingStatus,
}

/// Abstract port for tracking preferences store.
pub trait TrackingStore: Send + Sync {
    fn get_preferences(
        &self,
        project_id: ProjectId,
        repo_id: release_desk_core::repository::GitHubRepositoryId,
    ) -> Result<TrackingStatus, DomainError>;

    fn set_preferences(
        &self,
        project_id: ProjectId,
        repo_id: release_desk_core::repository::GitHubRepositoryId,
        status: TrackingStatus,
    ) -> Result<(), DomainError>;

    fn list_project_repositories(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<ProjectRepositoryRecord>, DomainError>;

    fn register_repository(
        &self,
        project_id: ProjectId,
        repo_id: release_desk_core::repository::GitHubRepositoryId,
        repo_name: &str,
        status: TrackingStatus,
    ) -> Result<(), DomainError>;
}

/// In-memory implementation of tracking store for default or mock use.
#[derive(Default)]
pub struct InMemoryTrackingStore {
    data: RwLock<
        HashMap<
            (ProjectId, release_desk_core::repository::GitHubRepositoryId),
            (String, TrackingStatus),
        >,
    >,
}

impl TrackingStore for InMemoryTrackingStore {
    fn get_preferences(
        &self,
        project_id: ProjectId,
        repo_id: release_desk_core::repository::GitHubRepositoryId,
    ) -> Result<TrackingStatus, DomainError> {
        let lock = self.data.read().unwrap();
        Ok(lock
            .get(&(project_id, repo_id))
            .map(|(_, status)| status.clone())
            .unwrap_or(TrackingStatus::Inbox))
    }

    fn set_preferences(
        &self,
        project_id: ProjectId,
        repo_id: release_desk_core::repository::GitHubRepositoryId,
        status: TrackingStatus,
    ) -> Result<(), DomainError> {
        let mut lock = self.data.write().unwrap();
        let repo_name = lock
            .get(&(project_id, repo_id))
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| "repository".to_string());
        lock.insert((project_id, repo_id), (repo_name, status));
        Ok(())
    }

    fn list_project_repositories(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<ProjectRepositoryRecord>, DomainError> {
        let lock = self.data.read().unwrap();
        let mut records: Vec<ProjectRepositoryRecord> = lock
            .iter()
            .filter(|((p_id, _), _)| *p_id == project_id)
            .map(
                |((_, repo_id), (repo_name, status))| ProjectRepositoryRecord {
                    repo_id: *repo_id,
                    repo_name: repo_name.clone(),
                    status: status.clone(),
                },
            )
            .collect();
        records.sort_by(|a, b| a.repo_name.cmp(&b.repo_name));
        Ok(records)
    }

    fn register_repository(
        &self,
        project_id: ProjectId,
        repo_id: release_desk_core::repository::GitHubRepositoryId,
        repo_name: &str,
        status: TrackingStatus,
    ) -> Result<(), DomainError> {
        let mut lock = self.data.write().unwrap();
        lock.insert((project_id, repo_id), (repo_name.to_string(), status));
        Ok(())
    }
}

/// Use case for discovering repositories for an account and annotating tracking status.
pub struct DiscoverRepositoriesUseCase {
    gateway: Arc<dyn GitHubGateway>,
    store: Arc<dyn TrackingStore>,
}

impl DiscoverRepositoriesUseCase {
    pub fn new(gateway: Arc<dyn GitHubGateway>, store: Arc<dyn TrackingStore>) -> Self {
        Self { gateway, store }
    }

    pub fn execute(
        &self,
        project_id: ProjectId,
        account: &AccountIdentity,
    ) -> Result<Vec<DiscoveredRepository>, DomainError> {
        let repos = self.gateway.list_repositories(account)?;
        let mut discovered = Vec::with_capacity(repos.len());
        for repo in repos {
            let status = self.store.get_preferences(project_id, repo.id)?;
            discovered.push(DiscoveredRepository {
                identity: repo,
                status,
            });
        }
        Ok(discovered)
    }
}

/// Use case for setting repository tracking preferences.
pub struct SetTrackingPreferencesUseCase {
    store: Arc<dyn TrackingStore>,
}

impl SetTrackingPreferencesUseCase {
    pub fn new(store: Arc<dyn TrackingStore>) -> Self {
        Self { store }
    }

    pub fn execute(
        &self,
        project_id: ProjectId,
        repo_id: release_desk_core::repository::GitHubRepositoryId,
        status: TrackingStatus,
    ) -> Result<(), DomainError> {
        self.store.set_preferences(project_id, repo_id, status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use release_desk_core::repository::GitHubRepositoryId;

    #[test]
    fn tracking_preference_lifecycle() {
        let store = Arc::new(InMemoryTrackingStore::default());
        let use_case = SetTrackingPreferencesUseCase::new(store.clone());

        let project_id = ProjectId::from_u128(1);
        let repo_id = GitHubRepositoryId::from_u64(42);

        assert_eq!(
            store.get_preferences(project_id, repo_id).unwrap(),
            TrackingStatus::Inbox
        );

        use_case
            .execute(
                project_id,
                repo_id,
                TrackingStatus::Tracked(TrackingPreferences::all()),
            )
            .unwrap();

        assert_eq!(
            store.get_preferences(project_id, repo_id).unwrap(),
            TrackingStatus::Tracked(TrackingPreferences::all())
        );
    }
}
