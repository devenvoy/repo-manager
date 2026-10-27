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
}

/// In-memory implementation of tracking store for default or mock use.
#[derive(Default)]
pub struct InMemoryTrackingStore {
    data: RwLock<
        HashMap<(ProjectId, release_desk_core::repository::GitHubRepositoryId), TrackingStatus>,
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
            .cloned()
            .unwrap_or(TrackingStatus::Inbox))
    }

    fn set_preferences(
        &self,
        project_id: ProjectId,
        repo_id: release_desk_core::repository::GitHubRepositoryId,
        status: TrackingStatus,
    ) -> Result<(), DomainError> {
        let mut lock = self.data.write().unwrap();
        lock.insert((project_id, repo_id), status);
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
