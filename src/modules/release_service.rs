//! Release service for fetching and downloading project release assets.

use crate::workspace::WorkspaceProject;
use crate::workspace_releases::RepositoryReleases;
use release_desk_store::workspace_releases::{download_asset, fetch_releases_with_project_host};
use std::path::{Path, PathBuf};

/// Fetch release history for all selected repositories within a project.
pub fn fetch_project_releases(
    project: &WorkspaceProject,
    token: Option<String>,
    secret_store: &release_desk_store::OsSecretStore,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> Vec<RepositoryReleases> {
    let mut jobs = Vec::new();
    let mut credentials = std::collections::HashMap::<String, Option<(String, String)>>::new();

    for source in &project.sources {
        for repo in source.repositories.iter().filter(|r| r.selected) {
            if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
                return Vec::new();
            }
            let effective_provider = if source.provider == "local" {
                provider_from_clone_url(&repo.clone_url)
                    .unwrap_or("local")
                    .to_string()
            } else {
                source.provider.clone()
            };

            let raw_owner = crate::repository_git::repository_owner(source, repo);
            let effective_owner = if raw_owner == "local" || raw_owner.is_empty() {
                repo.full_name
                    .rsplit_once('/')
                    .map(|(owner, _)| owner.to_string())
                    .or_else(|| owner_from_clone_url(&repo.clone_url))
                    .unwrap_or_else(|| source.handle.clone())
            } else {
                raw_owner.to_string()
            };
            let host = (effective_provider == "custom")
                .then(|| {
                    project.custom_host.clone().or_else(|| {
                        clone_host(&repo.clone_url).map(|host| format!("https://{host}"))
                    })
                })
                .flatten();

            let (effective_token, username) = if effective_provider == project.provider.as_str() {
                (token.clone(), Some(project.account.clone()))
            } else {
                let credential_host = host
                    .as_deref()
                    .and_then(|value| url::Url::parse(value).ok())
                    .and_then(|value| value.host_str().map(str::to_string))
                    .unwrap_or_else(|| {
                        crate::workspace::ProviderChoice::from_source_provider(&effective_provider)
                            .default_host()
                            .to_string()
                    });
                let credential = credentials
                    .entry(credential_host.clone())
                    .or_insert_with(|| {
                        secret_store.discover_cached_system_git_credentials(&credential_host)
                    });
                match credential {
                    Some((username, secret)) => (Some(secret.clone()), Some(username.clone())),
                    None => (None, None),
                }
            };
            let repo_dir = crate::repository_git::resolve_target_path(
                &project.directory,
                project.disk_layout,
                &effective_owner,
                &repo.name,
            );

            jobs.push((
                effective_provider,
                effective_owner,
                repo.name.clone(),
                effective_token,
                username,
                host,
                repo_dir,
            ));
        }
    }

    crate::modules::parallel::map_bounded(
        &jobs,
        crate::modules::parallel::IO_WORKERS,
        |(provider, owner, name, repo_token, username, host, repo_dir)| {
            let result = if cancelled
                .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
            {
                return RepositoryReleases {
                    provider: provider.clone(),
                    owner: owner.clone(),
                    repository: name.clone(),
                    result: Err("Release refresh cancelled".into()),
                };
            } else {
                fetch_releases_with_project_host(
                    provider,
                    owner,
                    name,
                    repo_token.clone(),
                    Some(&project.directory),
                    host.as_deref(),
                    username.as_deref(),
                )
                .or_else(|fetch_error| {
                    match release_desk_store::provider_releases::load_declared_releases(repo_dir) {
                        Ok(Some(releases)) => Ok(releases),
                        Ok(None) => Err(fetch_error),
                        Err(manifest_error) => Err(format!("{fetch_error}; {manifest_error}")),
                    }
                })
            };
            RepositoryReleases {
                provider: provider.clone(),
                owner: owner.clone(),
                repository: name.clone(),
                result,
            }
        },
    )
}

fn provider_from_clone_url(raw: &str) -> Option<&'static str> {
    let host = clone_host(raw)?;
    if host == "github.com" {
        Some("github")
    } else if host == "gitlab.com" {
        Some("gitlab")
    } else if host == "bitbucket.org" {
        Some("bitbucket")
    } else if host == "codeberg.org" {
        Some("codeberg")
    } else if host == "gitea.com" {
        Some("gitea")
    } else if host == "dev.azure.com"
        || host == "ssh.dev.azure.com"
        || host.ends_with(".visualstudio.com")
    {
        Some("azure_devops")
    } else if host.ends_with(".amazonaws.com") {
        Some("aws_codecommit")
    } else {
        None
    }
}

pub(crate) fn clone_host(raw: &str) -> Option<String> {
    if let Some(rest) = raw.strip_prefix("git@") {
        let host = rest.split_once(':')?.0;
        return (!host.is_empty()).then(|| host.to_ascii_lowercase());
    }
    url::Url::parse(raw)
        .ok()?
        .host_str()
        .map(str::to_ascii_lowercase)
}

/// Signed HTTPS artifact links can authorize a download without an account token.
pub(crate) fn signed_asset_url(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && url.query_pairs().any(|(key, _)| {
            key.eq_ignore_ascii_case("x-amz-signature") || key.eq_ignore_ascii_case("sig")
        })
}

fn owner_from_clone_url(raw: &str) -> Option<String> {
    let path = if let Some((_, path)) = raw.split_once(':') {
        if raw.starts_with("git@") { path } else { raw }
    } else {
        raw
    };
    let parts: Vec<_> = path
        .trim_end_matches(".git")
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    (parts.len() >= 2).then(|| parts[parts.len() - 2].to_string())
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

#[cfg(test)]
mod tests {
    use super::{clone_host, provider_from_clone_url, signed_asset_url};

    #[test]
    fn clone_url_provider_uses_exact_hosts() {
        assert_eq!(
            clone_host("git@codeberg.org:team/app.git").as_deref(),
            Some("codeberg.org")
        );
        assert_eq!(
            provider_from_clone_url("https://codeberg.org/team/app.git"),
            Some("codeberg")
        );
        assert_eq!(
            provider_from_clone_url("https://github.com.evil.test/team/app"),
            None
        );
    }

    #[test]
    fn signed_artifact_must_be_https() {
        assert!(signed_asset_url(
            "https://bucket.example/app.dmg?X-Amz-Signature=abc"
        ));
        assert!(!signed_asset_url(
            "http://bucket.example/app.dmg?X-Amz-Signature=abc"
        ));
        assert!(!signed_asset_url("https://bucket.example/app.dmg"));
    }
}
