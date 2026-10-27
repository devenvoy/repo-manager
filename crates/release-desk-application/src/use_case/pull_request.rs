//! Pull request synchronization and filtering use cases.

use release_desk_core::error::DomainError;
use release_desk_core::port::GitHubGateway;
use release_desk_core::pull_request::{PullRequest, PullRequestState};
use release_desk_core::repository::{GitHubRepositoryId, RepositoryIdentity};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Abstract port for pull request caching and retrieval.
pub trait PullRequestStore: Send + Sync {
    fn get_pull_requests(
        &self,
        repo_id: GitHubRepositoryId,
    ) -> Result<Vec<PullRequest>, DomainError>;

    fn save_pull_requests(
        &self,
        repo_id: GitHubRepositoryId,
        prs: &[PullRequest],
    ) -> Result<(), DomainError>;
}

/// In-memory pull request store for testing.
#[derive(Default)]
pub struct InMemoryPullRequestStore {
    data: RwLock<HashMap<GitHubRepositoryId, Vec<PullRequest>>>,
}

impl PullRequestStore for InMemoryPullRequestStore {
    fn get_pull_requests(
        &self,
        repo_id: GitHubRepositoryId,
    ) -> Result<Vec<PullRequest>, DomainError> {
        let lock = self.data.read().unwrap();
        Ok(lock.get(&repo_id).cloned().unwrap_or_default())
    }

    fn save_pull_requests(
        &self,
        repo_id: GitHubRepositoryId,
        prs: &[PullRequest],
    ) -> Result<(), DomainError> {
        let mut lock = self.data.write().unwrap();
        lock.insert(repo_id, prs.to_vec());
        Ok(())
    }
}

/// Use case for refreshing pull requests from GitHub and caching them in local storage.
pub struct SyncPullRequestsUseCase {
    gateway: Arc<dyn GitHubGateway>,
    store: Arc<dyn PullRequestStore>,
}

impl SyncPullRequestsUseCase {
    pub fn new(gateway: Arc<dyn GitHubGateway>, store: Arc<dyn PullRequestStore>) -> Self {
        Self { gateway, store }
    }

    pub fn execute(&self, repo: &RepositoryIdentity) -> Result<Vec<PullRequest>, DomainError> {
        let prs = self.gateway.fetch_pull_requests(repo)?;
        self.store.save_pull_requests(repo.id, &prs)?;
        Ok(prs)
    }
}

/// Filter options for querying pull requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PullRequestFilter {
    All,
    Open,
    Merged,
    Closed,
    Draft,
}

/// Use case for listing and filtering pull requests for a repository.
pub struct ListPullRequestsUseCase {
    store: Arc<dyn PullRequestStore>,
}

impl ListPullRequestsUseCase {
    pub fn new(store: Arc<dyn PullRequestStore>) -> Self {
        Self { store }
    }

    pub fn execute(
        &self,
        repo_id: GitHubRepositoryId,
        filter: PullRequestFilter,
    ) -> Result<Vec<PullRequest>, DomainError> {
        let prs = self.store.get_pull_requests(repo_id)?;
        let filtered = match filter {
            PullRequestFilter::All => prs,
            PullRequestFilter::Open => prs
                .into_iter()
                .filter(|pr| pr.state == PullRequestState::Open)
                .collect(),
            PullRequestFilter::Merged => prs
                .into_iter()
                .filter(|pr| pr.state == PullRequestState::Merged)
                .collect(),
            PullRequestFilter::Closed => prs
                .into_iter()
                .filter(|pr| pr.state == PullRequestState::Closed)
                .collect(),
            PullRequestFilter::Draft => prs
                .into_iter()
                .filter(|pr| pr.state == PullRequestState::Draft)
                .collect(),
        };
        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_filtering() {
        let store = Arc::new(InMemoryPullRequestStore::default());
        let repo_id = GitHubRepositoryId::from_u64(1);

        let pr1 = PullRequest {
            number: 1,
            title: "Feature 1".into(),
            state: PullRequestState::Open,
            draft: false,
            author: None,
            head_ref: "feat".into(),
            base_ref: "main".into(),
            html_url: "".into(),
            created_at: "".into(),
            updated_at: "".into(),
            merged_at: None,
            labels: vec![],
        };

        let pr2 = PullRequest {
            number: 2,
            title: "Feature 2".into(),
            state: PullRequestState::Merged,
            draft: false,
            author: None,
            head_ref: "feat2".into(),
            base_ref: "main".into(),
            html_url: "".into(),
            created_at: "".into(),
            updated_at: "".into(),
            merged_at: Some("2026-10-27T00:00:00Z".into()),
            labels: vec![],
        };

        store.save_pull_requests(repo_id, &[pr1, pr2]).unwrap();

        let use_case = ListPullRequestsUseCase::new(store);

        assert_eq!(
            use_case
                .execute(repo_id, PullRequestFilter::All)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            use_case
                .execute(repo_id, PullRequestFilter::Open)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            use_case
                .execute(repo_id, PullRequestFilter::Merged)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            use_case
                .execute(repo_id, PullRequestFilter::Closed)
                .unwrap()
                .len(),
            0
        );
    }
}
