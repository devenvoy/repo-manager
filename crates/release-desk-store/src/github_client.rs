//! GitHub REST API gateway adapter.
//!
//! Implements `GitHubGateway` for querying repositories, pull requests,
//! and release assets from GitHub API.

use release_desk_core::error::DomainError;
use release_desk_core::installer::{Asset, AssetConnection};
use release_desk_core::port::GitHubGateway;
use release_desk_core::project::{AccountIdentity, AccountKind, AccountLogin};
use release_desk_core::pull_request::{PullRequest, PullRequestState};
use release_desk_core::release::{Release, ReleaseTag};
use release_desk_core::repository::{GitHubRepositoryId, RepositoryIdentity, RepositoryName};
use serde::Deserialize;
use std::time::Duration;

/// HTTP client implementing `GitHubGateway`.
#[derive(Clone)]
pub struct HttpGitHubGateway {
    client: reqwest::blocking::Client,
    base_url: String,
    token: Option<String>,
}

impl HttpGitHubGateway {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Release-Desk-Gateway/1.0")
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url: "https://api.github.com".into(),
            token,
        }
    }

    /// Create with a custom base URL (useful for testing with wiremock/httptest).
    pub fn with_base_url(base_url: impl Into<String>, token: Option<String>) -> Self {
        let mut gateway = Self::new(token);
        gateway.base_url = base_url.into();
        gateway
    }

    fn request(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        let mut req = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github.v3+json");
        if let Some(ref token) = self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req
    }
}

#[derive(Deserialize)]
struct GhRepoOwner {
    login: String,
}

#[derive(Deserialize)]
struct GhRepo {
    id: u64,
    name: String,
    owner: GhRepoOwner,
}

#[derive(Deserialize)]
struct GhBranchRef {
    #[serde(rename = "ref")]
    branch_ref: String,
}

#[derive(Deserialize)]
struct GhPullRequest {
    number: u64,
    title: String,
    state: String,
    draft: Option<bool>,
    head: GhBranchRef,
    base: GhBranchRef,
    html_url: String,
    created_at: String,
    updated_at: String,
    merged_at: Option<String>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    created_at: Option<String>,
    html_url: Option<String>,
    prerelease: Option<bool>,
    draft: Option<bool>,
    assets: Vec<GhAsset>,
}

impl GitHubGateway for HttpGitHubGateway {
    fn list_repositories(
        &self,
        account: &AccountIdentity,
    ) -> Result<Vec<RepositoryIdentity>, DomainError> {
        let path = match account.kind {
            AccountKind::Organization => format!("/orgs/{}/repos", account.login.as_str()),
            AccountKind::User => format!("/users/{}/repos", account.login.as_str()),
        };

        let url = format!("{}{}?per_page=100", self.base_url, path);
        let resp = self
            .request(&url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "GitHub",
                message: format!("HTTP error: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "GitHub",
                message: format!("API returned HTTP status {}: {}", resp.status(), url),
            });
        }

        let gh_repos: Vec<GhRepo> = resp.json().map_err(|e| DomainError::PortFailure {
            port: "GitHub",
            message: format!("JSON parse error: {e}"),
        })?;

        let mut results = Vec::with_capacity(gh_repos.len());
        for repo in gh_repos {
            let id = GitHubRepositoryId::from_u64(repo.id);
            let owner = AccountLogin::new(&repo.owner.login).map_err(DomainError::from)?;
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
            "{}/repos/{}/{}/pulls?state=all&per_page=100",
            self.base_url,
            repo.owner.as_str(),
            repo.name.as_str()
        );

        let resp = self
            .request(&url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "GitHub",
                message: format!("HTTP error: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "GitHub",
                message: format!("API returned HTTP status {}: {}", resp.status(), url),
            });
        }

        let gh_prs: Vec<GhPullRequest> = resp.json().map_err(|e| DomainError::PortFailure {
            port: "GitHub",
            message: format!("JSON parse error: {e}"),
        })?;

        let mut results = Vec::with_capacity(gh_prs.len());
        for pr in gh_prs {
            let state = if pr.merged_at.is_some() {
                PullRequestState::Merged
            } else if pr.state == "closed" {
                PullRequestState::Closed
            } else if pr.draft.unwrap_or(false) {
                PullRequestState::Draft
            } else {
                PullRequestState::Open
            };

            results.push(PullRequest {
                number: pr.number,
                title: pr.title,
                state,
                draft: pr.draft.unwrap_or(false),
                author: None,
                head_ref: pr.head.branch_ref,
                base_ref: pr.base.branch_ref,
                html_url: pr.html_url,
                created_at: pr.created_at,
                updated_at: pr.updated_at,
                merged_at: pr.merged_at,
                labels: vec![],
            });
        }

        Ok(results)
    }

    fn fetch_releases(&self, repo: &RepositoryIdentity) -> Result<Vec<Release>, DomainError> {
        let url = format!(
            "{}/repos/{}/{}/releases?per_page=50",
            self.base_url,
            repo.owner.as_str(),
            repo.name.as_str()
        );

        let resp = self
            .request(&url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "GitHub",
                message: format!("HTTP error: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "GitHub",
                message: format!("API returned HTTP status {}: {}", resp.status(), url),
            });
        }

        let gh_releases: Vec<GhRelease> = resp.json().map_err(|e| DomainError::PortFailure {
            port: "GitHub",
            message: format!("JSON parse error: {e}"),
        })?;

        let mut releases = Vec::with_capacity(gh_releases.len());
        for rel in gh_releases {
            let tag = ReleaseTag::new(&rel.tag_name).map_err(DomainError::from)?;
            let assets: Vec<Asset> = rel
                .assets
                .into_iter()
                .map(|a| Asset {
                    name: a.name,
                    size: a.size,
                    download_url: a.browser_download_url,
                })
                .collect();

            releases.push(Release {
                name: rel.name,
                tag_name: tag.as_str().to_string(),
                description: rel.body,
                is_draft: rel.draft.unwrap_or(false),
                is_prerelease: rel.prerelease.unwrap_or(false),
                published_at: rel.published_at,
                created_at: rel.created_at.unwrap_or_default(),
                url: rel.html_url.unwrap_or_default(),
                release_assets: AssetConnection { nodes: assets },
            });
        }

        Ok(releases)
    }
}
