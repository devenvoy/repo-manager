//! Infrastructure service to fetch real user/organization profiles and repositories
//! from GitHub, GitLab, or Bitbucket APIs.

use release_desk_core::error::DomainError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Store a provider avatar locally so GPUI can render it without its HTTP image loader.
pub fn cache_avatar(url: &str, path: &std::path::Path) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Avatar URL must use HTTPS".into());
    }
    if path.is_file() {
        return Ok(());
    }
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("ReleaseDesk/1.0")
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    if response
        .content_length()
        .is_some_and(|size| size > 2_000_000)
    {
        return Err("Avatar image is too large".into());
    }
    let bytes = response.bytes().map_err(|error| error.to_string())?;
    if bytes.len() > 2_000_000 {
        return Err("Avatar image is too large".into());
    }
    let parent = path.parent().ok_or("Avatar cache path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("partial");
    std::fs::write(&temporary, &bytes).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

/// Metadata for an individual repository fetched from a remote Git provider.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FetchedRepo {
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub stars: u32,
    pub forks: u32,
    pub primary_language: Option<String>,
    pub is_private: bool,
    pub is_fork: bool,
    pub clone_url: String,
    pub default_branch: String,
}

/// Profile information and repository collection for a remote Git user or organization.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FetchedProfile {
    pub handle: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub provider: String,
    pub repositories: Vec<FetchedRepo>,
}

/// Parse a raw input string (which can be a full URL, owner/repo, or bare username)
/// into a structured provider, owner, and optional repository name.
pub fn parse_input_target(raw: &str, default_provider: &str) -> (String, String, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let without_proto = trimmed
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let mut parts = without_proto.split('/').filter(|s| !s.is_empty());
        let host = parts.next().unwrap_or("").to_lowercase();
        let owner = parts.next().unwrap_or("").to_string();
        let repo = parts.next().map(|r| r.trim_end_matches(".git").to_string());

        let provider = if host == "github.com" {
            "github".to_string()
        } else if host == "gitlab.com" {
            "gitlab".to_string()
        } else if host == "bitbucket.org" {
            "bitbucket".to_string()
        } else {
            "unsupported".to_string()
        };

        (provider, owner, repo)
    } else {
        let clean = trimmed.trim_start_matches('@').trim_matches('/');
        if let Some((owner, repo)) = clean.split_once('/') {
            (
                default_provider.to_string(),
                owner.to_string(),
                Some(repo.trim_end_matches(".git").to_string()),
            )
        } else {
            (default_provider.to_string(), clean.to_string(), None)
        }
    }
}

/// Resolve a source input, including GitLab paths that could name a subgroup or
/// a repository. For ambiguous paths, a real group lookup takes precedence;
/// only a not-found result falls back to a repository under its parent.
/// All network requests go to the provider's fixed API host.
pub fn fetch_source_input(
    raw: &str,
    default_provider: &str,
    token: Option<&str>,
) -> Result<(FetchedProfile, Option<String>), DomainError> {
    let (provider, owner, repo) = parse_input_target(raw, default_provider);
    if provider != "gitlab" {
        let profile = fetch_profile_and_repos(&owner, &provider, token)?;
        if let Some(name) = &repo {
            if !profile
                .repositories
                .iter()
                .any(|item| item.name.eq_ignore_ascii_case(name))
            {
                return Err(DomainError::NotFound {
                    entity: "repository",
                    id: format!("{owner}/{name}"),
                });
            }
        }
        return Ok((profile, repo));
    }
    let (path, explicit_repo) = gitlab_input_path(raw)?;
    if !explicit_repo {
        match fetch_profile_and_repos(&path, "gitlab", token) {
            Ok(profile) => return Ok((profile, None)),
            Err(DomainError::NotFound { .. }) if path.contains('/') => {}
            Err(error) => return Err(error),
        }
    }
    let (parent, name) = path.rsplit_once('/').ok_or({
        DomainError::Validation(release_desk_core::error::ValidationError::InvalidPath(
            "GitLab repository needs a namespace and name",
        ))
    })?;
    let profile = fetch_profile_and_repos(parent, "gitlab", token)?;
    let full_name = format!("{parent}/{name}");
    if !profile
        .repositories
        .iter()
        .any(|repo| repo.full_name.eq_ignore_ascii_case(&full_name))
    {
        return Err(DomainError::NotFound {
            entity: "GitLab repository",
            id: full_name,
        });
    }
    Ok((profile, Some(full_name)))
}

fn gitlab_input_path(raw: &str) -> Result<(String, bool), DomainError> {
    use release_desk_core::error::ValidationError;
    let invalid = || {
        DomainError::Validation(ValidationError::InvalidPath(
            "Enter a GitLab user, group, or repository path",
        ))
    };
    let raw = raw.trim();
    let path = if raw.starts_with("https://") || raw.starts_with("http://") {
        let url = reqwest::Url::parse(raw).map_err(|_| invalid())?;
        if url.host_str() != Some("gitlab.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        url.path().trim_matches('/').to_string()
    } else {
        raw.trim_start_matches('@').trim_matches('/').to_string()
    };
    let explicit_repo = path.ends_with(".git");
    let path = path.strip_suffix(".git").unwrap_or(&path);
    if path.is_empty()
        || path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part == "-"
                || part.contains(['?', '#', ' ', '\\', '%'])
                || part.chars().any(char::is_control)
        })
    {
        return Err(invalid());
    }
    Ok((path.to_string(), explicit_repo))
}

/// Fetch real profile and repository listings from the provider's REST API.
pub fn fetch_profile_and_repos(
    handle: &str,
    provider: &str,
    token: Option<&str>,
) -> Result<FetchedProfile, DomainError> {
    let clean_handle = handle.trim().trim_start_matches('@');
    if clean_handle.is_empty() {
        return Err(DomainError::Validation(
            release_desk_core::error::ValidationError::Empty("handle"),
        ));
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("Release-Desk-App/1.0")
        .build()
        .map_err(|e| DomainError::PortFailure {
            port: "source_discovery",
            message: format!("Could not initialize network client: {e}"),
        })?;

    match provider {
        "github"
            if !clean_handle
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') =>
        {
            Err(DomainError::Validation(
                release_desk_core::error::ValidationError::InvalidPath(
                    "Enter a valid GitHub user or organization name",
                ),
            ))
        }
        "gitlab" => fetch_gitlab_profile(&client, clean_handle, token),
        "github" => fetch_github_profile(&client, clean_handle, token),
        _ => Err(DomainError::PortFailure {
            port: "source_discovery",
            message: format!("Source discovery is not supported for {provider} yet"),
        }),
    }
}

/// Collect complete provider listings; an error never yields a partial catalogue.
fn collect_pages<T>(
    mut fetch: impl FnMut(u32) -> Result<Vec<T>, DomainError>,
) -> Result<Vec<T>, DomainError> {
    let mut items = Vec::new();
    let mut page = 1u32;
    loop {
        let batch = fetch(page)?;
        let last = batch.len() < 100;
        items.extend(batch);
        if last {
            return Ok(items);
        }
        page = page
            .checked_add(1)
            .ok_or_else(|| DomainError::PortFailure {
                port: "source_discovery",
                message: "Too many repository pages".into(),
            })?;
    }
}

#[derive(Deserialize)]
struct GhUserProfile {
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    bio: Option<String>,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct GhRepoItem {
    name: String,
    full_name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    stargazers_count: u32,
    #[serde(default)]
    forks_count: u32,
    language: Option<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    fork: bool,
    clone_url: Option<String>,
    default_branch: Option<String>,
}

fn fetch_github_profile(
    client: &reqwest::blocking::Client,
    handle: &str,
    token: Option<&str>,
) -> Result<FetchedProfile, DomainError> {
    let profile_url = format!("https://api.github.com/users/{handle}");
    let mut req = client
        .get(&profile_url)
        .header("Accept", "application/vnd.github.v3+json");
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        req = req.header("Authorization", format!("Bearer {t}"));
    }

    let profile_resp = req.send().map_err(|e| DomainError::PortFailure {
        port: "github_api",
        message: format!("Failed to reach GitHub API: {e}"),
    })?;

    if profile_resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(DomainError::NotFound {
            entity: "GitHub user or organization",
            id: handle.to_string(),
        });
    }

    if !profile_resp.status().is_success() {
        let status = profile_resp.status();
        let body = profile_resp.text().unwrap_or_default();
        return Err(DomainError::PortFailure {
            port: "github_api",
            message: format!("GitHub API returned {status}: {body}"),
        });
    }

    let profile: GhUserProfile = profile_resp.json().map_err(|e| DomainError::PortFailure {
        port: "github_api",
        message: format!("Failed to parse profile JSON: {e}"),
    })?;

    // Authenticated user listings include private repositories only on /user/repos.
    // Organization listings need /orgs/{org}/repos for accessible private repos.
    let mut listing = format!("users/{handle}/repos");
    if profile.kind == "Organization" {
        listing = format!("orgs/{handle}/repos");
    } else if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        let response = client
            .get("https://api.github.com/user")
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(t)
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "github_api",
                message: format!("Could not verify GitHub account: {e}"),
            })?;
        if !response.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "github_api",
                message: format!("Could not verify GitHub account: {}", response.status()),
            });
        }
        #[derive(Deserialize)]
        struct AuthenticatedUser {
            login: String,
        }
        let current: AuthenticatedUser = response.json().map_err(|e| DomainError::PortFailure {
            port: "github_api",
            message: format!("Could not read GitHub account: {e}"),
        })?;
        if current.login.eq_ignore_ascii_case(handle) {
            listing = "user/repos".into();
        }
    }

    let raw_repos = collect_pages(|page| {
        let repos_url =
            format!("https://api.github.com/{listing}?per_page=100&sort=updated&page={page}");
        let mut request = client
            .get(&repos_url)
            .header("Accept", "application/vnd.github+json");
        if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
            request = request.header("Authorization", format!("Bearer {t}"));
        }
        let response = request.send().map_err(|e| DomainError::PortFailure {
            port: "github_api",
            message: format!("Failed to fetch repositories on page {page}: {e}"),
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(DomainError::PortFailure {
                port: "github_api",
                message: format!("GitHub repository listing failed on page {page}: {status}"),
            });
        }
        response
            .json::<Vec<GhRepoItem>>()
            .map_err(|e| DomainError::PortFailure {
                port: "github_api",
                message: format!("Could not read repository listing on page {page}: {e}"),
            })
    })?;
    let raw_repos = raw_repos.into_iter().filter(|repo| {
        listing != "user/repos"
            || repo
                .full_name
                .as_deref()
                .and_then(|name| name.split_once('/'))
                .is_some_and(|(owner, _)| owner.eq_ignore_ascii_case(handle))
    });

    let repositories = raw_repos
        .map(|repo| map_github_repo(repo, handle))
        .collect();

    Ok(FetchedProfile {
        handle: profile.login,
        display_name: profile.name,
        avatar_url: profile.avatar_url,
        bio: profile.bio,
        provider: "github".to_string(),
        repositories,
    })
}

fn map_github_repo(repo: GhRepoItem, handle: &str) -> FetchedRepo {
    let full_name = repo
        .full_name
        .unwrap_or_else(|| format!("{handle}/{}", repo.name));
    FetchedRepo {
        name: repo.name,
        full_name,
        description: repo.description,
        stars: repo.stargazers_count,
        forks: repo.forks_count,
        primary_language: repo.language,
        is_private: repo.private,
        is_fork: repo.fork,
        clone_url: repo.clone_url.unwrap_or_default(),
        default_branch: repo.default_branch.unwrap_or_default(),
    }
}

#[derive(Deserialize)]
struct GlUserItem {
    id: u64,
    username: String,
    name: Option<String>,
    avatar_url: Option<String>,
    bio: Option<String>,
}

#[derive(Deserialize)]
struct GlGroupItem {
    id: u64,
    full_path: String,
    name: Option<String>,
    avatar_url: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct GlProjectItem {
    path: String,
    path_with_namespace: String,
    description: Option<String>,
    #[serde(default)]
    star_count: u32,
    #[serde(default)]
    forks_count: u32,
    http_url_to_repo: Option<String>,
    default_branch: Option<String>,
    visibility: String,
    forked_from_project: Option<serde_json::Value>,
}

fn gitlab_request(
    client: &reqwest::blocking::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::blocking::RequestBuilder {
    let request = client.get(url);
    if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        request.header("PRIVATE-TOKEN", token)
    } else {
        request
    }
}

fn fetch_gitlab_profile(
    client: &reqwest::blocking::Client,
    handle: &str,
    token: Option<&str>,
) -> Result<FetchedProfile, DomainError> {
    let mut group_url = reqwest::Url::parse("https://gitlab.com/api/v4/groups/").map_err(|e| {
        DomainError::PortFailure {
            port: "gitlab_api",
            message: e.to_string(),
        }
    })?;
    group_url
        .path_segments_mut()
        .map_err(|_| DomainError::PortFailure {
            port: "gitlab_api",
            message: "Invalid GitLab API URL".into(),
        })?
        .pop_if_empty()
        .push(handle);
    let group_response = gitlab_request(client, group_url.as_str(), token)
        .send()
        .map_err(|e| DomainError::PortFailure {
            port: "gitlab_api",
            message: format!("Failed to reach GitLab: {e}"),
        })?;
    let (id, is_group, profile_handle, display_name, avatar_url, bio) =
        if group_response.status().is_success() {
            let group: GlGroupItem =
                group_response
                    .json()
                    .map_err(|e| DomainError::PortFailure {
                        port: "gitlab_api",
                        message: format!("Could not read GitLab group: {e}"),
                    })?;
            (
                group.id,
                true,
                group.full_path,
                group.name,
                group.avatar_url,
                group.description,
            )
        } else if group_response.status() == reqwest::StatusCode::NOT_FOUND {
            let response = gitlab_request(client, "https://gitlab.com/api/v4/users", token)
                .query(&[("username", handle)])
                .send()
                .map_err(|e| DomainError::PortFailure {
                    port: "gitlab_api",
                    message: format!("Failed to reach GitLab: {e}"),
                })?;
            if !response.status().is_success() {
                return Err(DomainError::PortFailure {
                    port: "gitlab_api",
                    message: format!("GitLab user lookup failed: {}", response.status()),
                });
            }
            let users: Vec<GlUserItem> = response.json().map_err(|e| DomainError::PortFailure {
                port: "gitlab_api",
                message: format!("Could not read GitLab users: {e}"),
            })?;
            let user = users
                .into_iter()
                .find(|u| u.username.eq_ignore_ascii_case(handle))
                .ok_or_else(|| DomainError::NotFound {
                    entity: "GitLab user or group",
                    id: handle.to_string(),
                })?;
            (
                user.id,
                false,
                user.username,
                user.name,
                user.avatar_url,
                user.bio,
            )
        } else {
            return Err(DomainError::PortFailure {
                port: "gitlab_api",
                message: format!("GitLab group lookup failed: {}", group_response.status()),
            });
        };

    let endpoint = if is_group {
        format!("https://gitlab.com/api/v4/groups/{id}/projects")
    } else {
        format!("https://gitlab.com/api/v4/users/{id}/projects")
    };
    let projects = collect_pages(|page| {
        let response = gitlab_request(client, &endpoint, token)
            .query(&[
                ("per_page", "100"),
                ("include_subgroups", "true"),
                ("page", &page.to_string()),
            ])
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "gitlab_api",
                message: format!("Failed to fetch projects on page {page}: {e}"),
            })?;
        if !response.status().is_success() {
            return Err(DomainError::PortFailure {
                port: "gitlab_api",
                message: format!(
                    "GitLab project listing failed on page {page}: {}",
                    response.status()
                ),
            });
        }
        response
            .json::<Vec<GlProjectItem>>()
            .map_err(|e| DomainError::PortFailure {
                port: "gitlab_api",
                message: format!("Could not read GitLab projects on page {page}: {e}"),
            })
    })?;

    let repositories = projects.into_iter().map(map_gitlab_repo).collect();

    Ok(FetchedProfile {
        handle: profile_handle,
        display_name,
        avatar_url,
        bio,
        provider: "gitlab".into(),
        repositories,
    })
}

fn map_gitlab_repo(project: GlProjectItem) -> FetchedRepo {
    FetchedRepo {
        name: project.path,
        full_name: project.path_with_namespace,
        description: project.description,
        stars: project.star_count,
        forks: project.forks_count,
        primary_language: None,
        is_private: project.visibility != "public",
        is_fork: project.forked_from_project.is_some(),
        clone_url: project.http_url_to_repo.unwrap_or_default(),
        default_branch: project.default_branch.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_fetches_every_page_and_never_returns_partial_results_on_error() {
        let mut calls = Vec::new();
        let items = collect_pages(|page| {
            calls.push(page);
            Ok(if page == 1 { vec![1; 100] } else { vec![2; 3] })
        })
        .unwrap();
        assert_eq!(calls, [1, 2]);
        assert_eq!(items.len(), 103);
        assert_eq!(items[102], 2);
        let result = collect_pages(|page| {
            if page == 1 {
                Ok(vec![1; 100])
            } else {
                Err(DomainError::PortFailure {
                    port: "fixture",
                    message: "page 2 failed".into(),
                })
            }
        });
        assert!(matches!(
            result,
            Err(DomainError::PortFailure {
                port: "fixture",
                ..
            })
        ));
    }

    #[test]
    fn provider_metadata_is_preserved_and_missing_fields_are_not_invented() {
        let gh: GhRepoItem = serde_json::from_str(r#"{"name":"app","full_name":"team/app","description":"Tool","stargazers_count":12,"forks_count":4,"language":"Rust","private":true,"fork":true,"clone_url":"https://github.com/team/app.git","default_branch":"trunk"}"#).unwrap();
        let repo = map_github_repo(gh, "team");
        assert_eq!(
            (
                repo.full_name.as_str(),
                repo.stars,
                repo.forks,
                repo.primary_language.as_deref()
            ),
            ("team/app", 12, 4, Some("Rust"))
        );
        assert!(repo.is_private && repo.is_fork);
        assert_eq!(repo.default_branch, "trunk");
        let gl: GlProjectItem = serde_json::from_str(r#"{"path":"app","path_with_namespace":"team/sub/app","visibility":"internal","forked_from_project":{}}"#).unwrap();
        let repo = map_gitlab_repo(gl);
        assert_eq!(repo.full_name, "team/sub/app");
        assert!(repo.is_private && repo.is_fork);
        assert_eq!(repo.clone_url, "");
        assert_eq!(repo.default_branch, "");
        assert_eq!(repo.primary_language, None);
    }

    #[test]
    fn invalid_github_handles_fail_before_network_access() {
        for handle in ["team?x=y", "../owner", "team%2Fother", "bad name"] {
            assert!(
                matches!(
                    fetch_profile_and_repos(handle, "github", None),
                    Err(DomainError::Validation(_))
                ),
                "{handle}"
            );
        }
    }

    #[test]
    fn test_parse_input_target_variations() {
        assert_eq!(
            parse_input_target("https://github.com/braydennilsen", "github"),
            ("github".into(), "braydennilsen".into(), None)
        );
        assert_eq!(
            parse_input_target("https://github.com/braydennilsen/PaliCAD", "github"),
            (
                "github".into(),
                "braydennilsen".into(),
                Some("PaliCAD".into())
            )
        );
        assert_eq!(
            parse_input_target("@tokio-rs", "github"),
            ("github".into(), "tokio-rs".into(), None)
        );
        assert_eq!(
            parse_input_target("https://gitlab.com/inkscape/inkscape.git", "github"),
            ("gitlab".into(), "inkscape".into(), Some("inkscape".into()))
        );
    }

    #[test]
    fn gitlab_paths_preserve_subgroups_and_reject_unsafe_inputs() {
        assert_eq!(
            gitlab_input_path("https://gitlab.com/team/subgroup").unwrap(),
            ("team/subgroup".into(), false)
        );
        assert_eq!(
            gitlab_input_path("https://gitlab.com/team/subgroup/app.git").unwrap(),
            ("team/subgroup/app".into(), true)
        );
        for input in [
            "https://gitlab.com.attacker.test/team",
            "https://gitlab.com/team?a=b",
            "team/../app",
            "team//app",
            "team/%2fapp",
        ] {
            assert!(gitlab_input_path(input).is_err(), "{input}");
        }
    }

    #[test]
    fn rejects_lookalike_provider_hosts() {
        let (provider, _, _) =
            parse_input_target("https://github.com.attacker.example/team", "github");
        assert_eq!(provider, "unsupported");
    }

    #[test]
    #[ignore]
    fn test_live_github_fetch_braydennilsen() {
        let profile =
            fetch_profile_and_repos("braydennilsen", "github", None).expect("failed live fetch");
        assert_eq!(profile.handle, "braydennilsen");
        assert!(profile.repositories.iter().any(|r| r.name == "PaliCAD"));
    }
}
