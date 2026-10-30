use super::{
    SourceAdapter, SourceRequest, api_client, clean_clone_url, failure, safe_origin, safe_path,
};
use crate::source_fetcher::{FetchedProfile, FetchedRepo};
use release_desk_core::error::DomainError;
use serde::Deserialize;

pub(super) struct AzureSource;

#[derive(Deserialize)]
struct RepoList {
    value: Vec<AzureRepo>,
}

#[derive(Deserialize)]
struct AzureRepo {
    name: String,
    #[serde(rename = "remoteUrl")]
    remote_url: String,
    #[serde(rename = "defaultBranch")]
    default_branch: Option<String>,
    #[serde(rename = "isFork", default)]
    is_fork: bool,
}

impl SourceAdapter for AzureSource {
    fn discover(
        &self,
        request: &SourceRequest<'_>,
    ) -> Result<(FetchedProfile, Option<String>), DomainError> {
        let endpoint = request.endpoint.ok_or_else(|| {
            failure("Enter an Azure DevOps organization URL, such as https://dev.azure.com/example")
        })?;
        let base = safe_origin(endpoint)?;
        if base.host_str() != Some("dev.azure.com") {
            return Err(failure("Azure DevOps organization must use dev.azure.com"));
        }
        let base_parts = safe_path(base.path())?;
        if base_parts.len() != 1 {
            return Err(failure(
                "Azure DevOps organization URL must contain one organization",
            ));
        }
        let organization = base_parts[0];
        let path = if request.input.starts_with("https://") {
            let url = safe_origin(request.input)?;
            if url.origin() != base.origin() {
                return Err(failure("Azure repository URL belongs to another host"));
            }
            if url.path_segments().and_then(|mut segments| segments.next()) != Some(organization) {
                return Err(failure(
                    "Azure repository URL belongs to another organization",
                ));
            }
            url.path().trim_matches('/').to_string()
        } else {
            request
                .input
                .trim_start_matches('@')
                .trim_matches('/')
                .to_string()
        };
        let parts = safe_path(&path)?;
        let (project, selected) = match parts.as_slice() {
            [project] => (*project, None),
            [org, project] if *org == organization => (*project, None),
            [project, repo] => (*project, Some((*repo).to_string())),
            [org, project, "_git", repo] if *org == organization => {
                (*project, Some((*repo).to_string()))
            }
            _ => return Err(failure("Enter an Azure project or project/repository URL")),
        };
        let client = api_client()?;
        let url = format!(
            "https://dev.azure.com/{organization}/{project}/_apis/git/repositories?api-version=7.1"
        );
        let mut call = client.get(&url);
        if let Some(token) = request.credential {
            call = call.basic_auth("", Some(token));
        }
        let mut response = call.send().map_err(|error| failure(error.to_string()))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Some(token) = request.credential {
                response = client
                    .get(&url)
                    .bearer_auth(token)
                    .send()
                    .map_err(|error| failure(error.to_string()))?;
            }
        }
        if !response.status().is_success() {
            return Err(failure(format!(
                "Azure repository listing returned HTTP {}",
                response.status()
            )));
        }
        let listing: RepoList = response
            .json()
            .map_err(|error| failure(format!("Invalid Azure repository listing: {error}")))?;
        let repositories: Vec<_> = listing
            .value
            .into_iter()
            .map(|repo| {
                Ok(FetchedRepo {
                    full_name: format!("{organization}/{project}/{}", repo.name),
                    name: repo.name,
                    description: None,
                    stars: 0,
                    forks: 0,
                    primary_language: None,
                    is_private: true,
                    is_fork: repo.is_fork,
                    clone_url: clean_clone_url(&repo.remote_url, &base, false)?,
                    default_branch: repo
                        .default_branch
                        .unwrap_or_default()
                        .trim_start_matches("refs/heads/")
                        .to_string(),
                })
            })
            .collect::<Result<_, DomainError>>()?;
        if let Some(name) = &selected {
            if !repositories
                .iter()
                .any(|repo| repo.name.eq_ignore_ascii_case(name))
            {
                return Err(failure(format!(
                    "Azure repository {project}/{name} was not found"
                )));
            }
        }
        Ok((
            FetchedProfile {
                handle: format!("{organization}/{project}"),
                display_name: Some(project.to_string()),
                avatar_url: None,
                bio: None,
                provider: "azure_devops".into(),
                repositories,
            },
            selected,
        ))
    }
}
