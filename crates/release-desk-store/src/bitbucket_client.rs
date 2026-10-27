//! Atlassian Bitbucket REST API gateway adapter.
//!
//! Implements `GitHostGateway` for querying Atlassian Bitbucket repositories,
//! pull requests, and downloads.

use release_desk_core::error::DomainError;
use release_desk_core::port::GitHostGateway;
use release_desk_core::project::{AccountIdentity, AccountLogin};
use release_desk_core::provider::GitProviderKind;
use release_desk_core::pull_request::{PullRequest, PullRequestState};
use release_desk_core::release::Release;
use release_desk_core::repository::{GitHubRepositoryId, RepositoryIdentity, RepositoryName};
use serde::Deserialize;
use std::time::Duration;

/// HTTP client implementing `GitHostGateway` for Atlassian Bitbucket.
#[derive(Clone)]
pub struct HttpBitbucketGateway {
    client: reqwest::blocking::Client,
    base_url: String,
    token: Option<String>,
}

impl HttpBitbucketGateway {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Release-Desk-Bitbucket/1.0")
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url: "https://api.bitbucket.org/2.0".into(),
            token,
        }
    }

    pub fn with_base_url(base_url: impl Into<String>, token: Option<String>) -> Self {
        let mut gateway = Self::new(token);
        gateway.base_url = base_url.into();
        gateway
    }

    fn request(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        let mut req = self.client.get(url);
        if let Some(ref token) = self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req
    }
}

#[derive(Deserialize)]
struct BbOwner {
    username: Option<String>,
}

#[derive(Deserialize)]
struct BbRepo {
    name: String,
    owner: Option<BbOwner>,
}

#[derive(Deserialize)]
struct BbRepoPage {
    values: Vec<BbRepo>,
}

#[derive(Deserialize)]
struct BbBranch {
    name: String,
}

#[derive(Deserialize)]
struct BbEndpoint {
    branch: Option<BbBranch>,
}

#[derive(Deserialize)]
struct BbHtmlLink {
    href: String,
}

#[derive(Deserialize)]
struct BbLinks {
    html: Option<BbHtmlLink>,
}

#[derive(Deserialize)]
struct BbPullRequest {
    id: u64,
    title: String,
    state: String,
    source: Option<BbEndpoint>,
    destination: Option<BbEndpoint>,
    created_on: Option<String>,
    updated_on: Option<String>,
    links: Option<BbLinks>,
}

#[derive(Deserialize)]
struct BbPrPage {
    values: Vec<BbPullRequest>,
}

impl GitHostGateway for HttpBitbucketGateway {
    fn provider_kind(&self) -> GitProviderKind {
        GitProviderKind::Bitbucket
    }

    fn list_repositories(
        &self,
        account: &AccountIdentity,
    ) -> Result<Vec<RepositoryIdentity>, DomainError> {
        let url = format!(
            "{}/repositories/{}?pagelen=100",
            self.base_url,
            account.login.as_str()
        );

        let resp = self
            .request(&url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "Bitbucket",
                message: format!("HTTP error: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "Bitbucket",
                message: format!("API returned HTTP status {}: {}", resp.status(), url),
            });
        }

        let page: BbRepoPage = resp.json().map_err(|e| DomainError::PortFailure {
            port: "Bitbucket",
            message: format!("JSON parse error: {e}"),
        })?;

        let mut results = Vec::with_capacity(page.values.len());
        for (idx, repo) in page.values.into_iter().enumerate() {
            let id = GitHubRepositoryId::from_u64((idx + 1) as u64);
            let owner_str = repo
                .owner
                .and_then(|o| o.username)
                .unwrap_or_else(|| account.login.as_str().to_string());
            let owner = AccountLogin::new(&owner_str).unwrap_or_else(|_| account.login.clone());
            let name = RepositoryName::new(&repo.name).map_err(DomainError::from)?;

            results.push(RepositoryIdentity { id, owner, name });
        }

        Ok(results)
    }

    fn fetch_pull_requests(
        &self,
        repo: &RepositoryIdentity,
    ) -> Result<Vec<PullRequest>, DomainError> {
        let url = format!(
            "{}/repositories/{}/{}/pullrequests?pagelen=100&state=ALL",
            self.base_url,
            repo.owner.as_str(),
            repo.name.as_str()
        );

        let resp = self
            .request(&url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "Bitbucket",
                message: format!("HTTP error: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "Bitbucket",
                message: format!("API returned HTTP status {}: {}", resp.status(), url),
            });
        }

        let page: BbPrPage = resp.json().map_err(|e| DomainError::PortFailure {
            port: "Bitbucket",
            message: format!("JSON parse error: {e}"),
        })?;

        let mut results = Vec::with_capacity(page.values.len());
        for pr in page.values {
            let state = match pr.state.as_str() {
                "MERGED" => PullRequestState::Merged,
                "DECLINED" | "SUPERSEDED" => PullRequestState::Closed,
                _ => PullRequestState::Open,
            };

            let head_ref = pr
                .source
                .and_then(|s| s.branch)
                .map(|b| b.name)
                .unwrap_or_else(|| "feature".into());
            let base_ref = pr
                .destination
                .and_then(|d| d.branch)
                .map(|b| b.name)
                .unwrap_or_else(|| "main".into());
            let html_url = pr
                .links
                .and_then(|l| l.html)
                .map(|h| h.href)
                .unwrap_or_default();

            results.push(PullRequest {
                number: pr.id,
                title: pr.title,
                state,
                draft: false,
                author: None,
                head_ref,
                base_ref,
                html_url,
                created_at: pr.created_on.unwrap_or_default(),
                updated_at: pr.updated_on.unwrap_or_default(),
                merged_at: None,
                labels: vec![],
            });
        }

        Ok(results)
    }

    fn fetch_releases(&self, _repo: &RepositoryIdentity) -> Result<Vec<Release>, DomainError> {
        // Bitbucket uses downloads or tags for releases
        Ok(vec![])
    }
}
