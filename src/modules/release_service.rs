//! Release service for fetching and downloading project release assets.

use crate::workspace::WorkspaceProject;
use crate::workspace_releases::RepositoryReleases;
use release_desk_store::workspace_releases::{download_asset, fetch_releases_with_project};
use std::path::{Path, PathBuf};

/// Fetch release history for all selected repositories within a project.
pub fn fetch_project_releases(
    project: &WorkspaceProject,
    token: Option<String>,
) -> Vec<RepositoryReleases> {
    let mut jobs = Vec::new();

    for source in &project.sources {
        for repo in source.repositories.iter().filter(|r| r.selected) {
            let effective_provider = if source.provider == "local" {
                if repo.clone_url.contains("gitlab.com")
                    || repo.clone_url.starts_with("git@gitlab.com:")
                {
                    "gitlab".to_string()
                } else {
                    "github".to_string()
                }
            } else {
                source.provider.clone()
            };

            let raw_owner = crate::repository_git::repository_owner(source, repo);
            let effective_owner = if raw_owner == "local" || raw_owner.is_empty() {
                if let Some(pos) = repo.clone_url.find("github.com/") {
                    let path = repo.clone_url[pos + 11..].trim_end_matches(".git");
                    path.split_once('/')
                        .map(|(o, _)| o.to_string())
                        .unwrap_or_else(|| "storytold".to_string())
                } else if let Some(pos) = repo.clone_url.find("github.com:") {
                    let path = repo.clone_url[pos + 11..].trim_end_matches(".git");
                    path.split_once('/')
                        .map(|(o, _)| o.to_string())
                        .unwrap_or_else(|| "storytold".to_string())
                } else {
                    "storytold".to_string()
                }
            } else {
                raw_owner.to_string()
            };

            let effective_token = if effective_provider == project.provider.as_str() {
                token.clone()
            } else {
                None
            };

            jobs.push((
                effective_provider,
                effective_owner,
                repo.name.clone(),
                effective_token,
            ));
        }
    }

    jobs.into_iter()
        .map(|(provider, owner, name, repo_token)| {
            let result = fetch_releases_with_project(
                &provider,
                &owner,
                &name,
                repo_token,
                Some(&project.directory),
            );
            RepositoryReleases {
                provider,
                owner,
                repository: name,
                result,
            }
        })
        .collect()
}

/// Download a release asset to the project's releases directory.
#[allow(clippy::too_many_arguments)]
pub fn download_project_asset(
    project_dir: &Path,
    _provider: &str,
    owner: &str,
    repo: &str,
    tag: &str,
    asset_name: &str,
    download_url: &str,
    expected_size: Option<u64>,
    _token: Option<&str>,
) -> Result<PathBuf, String> {
    download_asset(
        project_dir,
        owner,
        repo,
        tag,
        asset_name,
        download_url,
        expected_size,
    )
}
