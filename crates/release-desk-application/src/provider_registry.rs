//! Multi-host Git provider registry.
//!
//! Maintains registered adapters for GitHub, GitLab, Atlassian Bitbucket,
//! and self-hosted instances.

use release_desk_core::error::DomainError;
use release_desk_core::port::GitHostGateway;
use release_desk_core::provider::GitProviderKind;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Registry of Git host gateways keyed by provider kind.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    gateways: Arc<RwLock<HashMap<GitProviderKind, Arc<dyn GitHostGateway>>>>,
}

impl ProviderRegistry {
    /// Create an empty provider registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a gateway for a Git hosting platform.
    pub fn register(&self, gateway: Arc<dyn GitHostGateway>) {
        let kind = gateway.provider_kind();
        let mut map = self.gateways.write().unwrap();
        map.insert(kind, gateway);
    }

    /// Retrieve the gateway for a specific Git provider kind.
    pub fn get(&self, kind: &GitProviderKind) -> Result<Arc<dyn GitHostGateway>, DomainError> {
        let map = self.gateways.read().unwrap();
        map.get(kind).cloned().ok_or_else(|| DomainError::NotFound {
            entity: "GitHostGateway",
            id: kind.to_string(),
        })
    }

    /// List all currently registered Git provider kinds.
    pub fn supported_providers(&self) -> Vec<GitProviderKind> {
        let map = self.gateways.read().unwrap();
        let mut keys: Vec<_> = map.keys().cloned().collect();
        keys.sort();
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use release_desk_core::project::AccountIdentity;
    use release_desk_core::pull_request::PullRequest;
    use release_desk_core::release::Release;
    use release_desk_core::repository::RepositoryIdentity;

    struct DummyGateway(GitProviderKind);
    impl GitHostGateway for DummyGateway {
        fn provider_kind(&self) -> GitProviderKind {
            self.0.clone()
        }
        fn list_repositories(
            &self,
            _a: &AccountIdentity,
        ) -> Result<Vec<RepositoryIdentity>, DomainError> {
            Ok(vec![])
        }
        fn fetch_pull_requests(
            &self,
            _r: &RepositoryIdentity,
        ) -> Result<Vec<PullRequest>, DomainError> {
            Ok(vec![])
        }
        fn fetch_releases(&self, _r: &RepositoryIdentity) -> Result<Vec<Release>, DomainError> {
            Ok(vec![])
        }
    }

    #[test]
    fn registry_register_and_lookup() {
        let registry = ProviderRegistry::new();
        registry.register(Arc::new(DummyGateway(GitProviderKind::GitHub)));
        registry.register(Arc::new(DummyGateway(GitProviderKind::GitLab)));
        registry.register(Arc::new(DummyGateway(GitProviderKind::Bitbucket)));

        assert!(registry.get(&GitProviderKind::GitHub).is_ok());
        assert!(registry.get(&GitProviderKind::GitLab).is_ok());
        assert!(registry.get(&GitProviderKind::Bitbucket).is_ok());

        let providers = registry.supported_providers();
        assert_eq!(providers.len(), 3);
        assert_eq!(providers[0], GitProviderKind::GitHub);
        assert_eq!(providers[1], GitProviderKind::GitLab);
        assert_eq!(providers[2], GitProviderKind::Bitbucket);
    }
}
