//! GitLab REST API gateway adapter.
//!
//! Implements `GitHostGateway` for querying GitLab projects, merge requests,
//! and releases from gitlab.com or self-hosted GitLab instances.

use release_desk_core::error::DomainError;
use release_desk_core::installer::{Asset, AssetConnection};
use release_desk_core::port::GitHostGateway;
use release_desk_core::project::{AccountIdentity, AccountLogin};
use release_desk_core::provider::GitProviderKind;
use release_desk_core::pull_request::{PullRequest, PullRequestState};
use release_desk_core::release::{Release, ReleaseTag};
use release_desk_core::repository::{GitHubRepositoryId, RepositoryIdentity, RepositoryName};
use serde::Deserialize;
use std::time::Duration;

/// HTTP client implementing `GitHostGateway` for GitLab.
#[derive(Clone)]
pub struct HttpGitLabGateway {
    client: reqwest::blocking::Client,
    base_url: String,
    token: Option<String>,
}

impl HttpGitLabGateway {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Release-Desk-GitLab/1.0")
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url: "https://gitlab.com/api/v4".into(),
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
            req = req.header("PRIVATE-TOKEN", token);
        }
        req
    }
}

#[derive(Deserialize)]
struct GlProject {
    id: u64,
    name: String,
    path_with_namespace: String,
}

#[derive(Deserialize)]
struct GlMergeRequest {
    iid: u64,
    title: String,
    state: String,
    draft: Option<bool>,
    source_branch: String,
    target_branch: String,
    web_url: String,
    created_at: String,
    updated_at: String,
    merged_at: Option<String>,
}

#[derive(Deserialize)]
struct GlReleaseAssetLink {
    name: String,
    url: String,
}

#[derive(Deserialize)]
struct GlReleaseAssets {
    links: Option<Vec<GlReleaseAssetLink>>,
}

#[derive(Deserialize)]
struct GlRelease {
    tag_name: String,
    name: Option<String>,
    description: Option<String>,
    released_at: Option<String>,
    created_at: Option<String>,
    assets: Option<GlReleaseAssets>,
}

impl GitHostGateway for HttpGitLabGateway {
    fn provider_kind(&self) -> GitProviderKind {
        GitProviderKind::GitLab
    }

    fn list_repositories(
        &self,
        account: &AccountIdentity,
    ) -> Result<Vec<RepositoryIdentity>, DomainError> {
        let url = format!(
            "{}/users/{}/projects?per_page=100",
            self.base_url,
            account.login.as_str()
        );

        let resp = self
            .request(&url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "GitLab",
                message: format!("HTTP error: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "GitLab",
                message: format!("API returned HTTP status {}: {}", resp.status(), url),
            });
        }

        let projects: Vec<GlProject> = resp.json().map_err(|e| DomainError::PortFailure {
            port: "GitLab",
            message: format!("JSON parse error: {e}"),
        })?;

        let mut results = Vec::with_capacity(projects.len());
        for proj in projects {
            let id = GitHubRepositoryId::from_u64(proj.id);
            let owner_name = proj
                .path_with_namespace
                .split('/')
                .next()
                .unwrap_or(account.login.as_str());
            let owner = AccountLogin::new(owner_name).unwrap_or_else(|_| account.login.clone());
            let name = RepositoryName::new(&proj.name).map_err(DomainError::from)?;

            results.push(RepositoryIdentity { id, owner, name });
        }

        Ok(results)
    }

    fn fetch_pull_requests(
        &self,
        repo: &RepositoryIdentity,
    ) -> Result<Vec<PullRequest>, DomainError> {
        let url = format!(
            "{}/projects/{}/merge_requests?per_page=100",
            self.base_url,
            repo.id.as_u64()
        );

        let resp = self
            .request(&url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "GitLab",
                message: format!("HTTP error: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "GitLab",
                message: format!("API returned HTTP status {}: {}", resp.status(), url),
            });
        }

        let mrs: Vec<GlMergeRequest> = resp.json().map_err(|e| DomainError::PortFailure {
            port: "GitLab",
            message: format!("JSON parse error: {e}"),
        })?;

        let mut results = Vec::with_capacity(mrs.len());
        for mr in mrs {
            let state = match mr.state.as_str() {
                "merged" => PullRequestState::Merged,
                "closed" => PullRequestState::Closed,
                _ if mr.draft.unwrap_or(false) => PullRequestState::Draft,
                _ => PullRequestState::Open,
            };

            results.push(PullRequest {
                number: mr.iid,
                title: mr.title,
                state,
                draft: mr.draft.unwrap_or(false),
                author: None,
                head_ref: mr.source_branch,
                base_ref: mr.target_branch,
                html_url: mr.web_url,
                created_at: mr.created_at,
                updated_at: mr.updated_at,
                merged_at: mr.merged_at,
                labels: vec![],
            });
        }

        Ok(results)
    }

    fn fetch_releases(&self, repo: &RepositoryIdentity) -> Result<Vec<Release>, DomainError> {
        let url = format!(
            "{}/projects/{}/releases?per_page=50",
            self.base_url,
            repo.id.as_u64()
        );

        let resp = self
            .request(&url)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "GitLab",
                message: format!("HTTP error: {e}"),
            })?;

        if !resp.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "GitLab",
                message: format!("API returned HTTP status {}: {}", resp.status(), url),
            });
        }

        let gl_releases: Vec<GlRelease> = resp.json().map_err(|e| DomainError::PortFailure {
            port: "GitLab",
            message: format!("JSON parse error: {e}"),
        })?;

        let mut releases = Vec::with_capacity(gl_releases.len());
        for rel in gl_releases {
            let tag = ReleaseTag::new(&rel.tag_name).map_err(DomainError::from)?;
            let assets: Vec<Asset> = rel
                .assets
                .and_then(|a| a.links)
                .unwrap_or_default()
                .into_iter()
                .map(|l| Asset {
                    name: l.name,
                    size: 0,
                    download_url: l.url,
                })
                .collect();

            releases.push(Release {
                name: rel.name,
                tag_name: tag.as_str().to_string(),
                description: rel.description,
                is_draft: false,
                is_prerelease: false,
                published_at: rel.released_at,
                created_at: rel.created_at.unwrap_or_default(),
                url: format!("{}/-/releases/{}", repo.name.as_str(), tag.as_str()),
                release_assets: AssetConnection { nodes: assets },
            });
        }

        Ok(releases)
    }
}
