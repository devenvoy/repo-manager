//! Fetch and download release assets for the desktop workspace.
use crate::HttpGitHubGateway;
use release_desk_core::port::GitHubGateway;
use release_desk_core::project::AccountLogin;
use release_desk_core::release::Release;
use release_desk_core::repository::{GitHubRepositoryId, RepositoryIdentity, RepositoryName};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Provider data used by the workspace; unknown fields remain unknown.
#[derive(Clone, Debug)]
pub struct WorkspaceAsset {
    pub name: String,
    pub download_url: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceRelease {
    pub tag_name: String,
    pub description: Option<String>,
    pub is_draft: bool,
    pub is_prerelease: Option<bool>,
    pub published_at: Option<String>,
    pub assets: Vec<WorkspaceAsset>,
}

impl From<Release> for WorkspaceRelease {
    fn from(release: Release) -> Self {
        Self {
            tag_name: release.tag_name,
            description: release.description,
            is_draft: release.is_draft,
            is_prerelease: Some(release.is_prerelease),
            published_at: release.published_at,
            assets: release
                .release_assets
                .nodes
                .into_iter()
                .map(|asset| WorkspaceAsset {
                    name: asset.name,
                    download_url: asset.download_url,
                    size: Some(asset.size),
                })
                .collect(),
        }
    }
}

/// Fetch release history. Unsupported providers return an error, not an empty list.
pub fn fetch_releases(
    provider: &str,
    owner: &str,
    name: &str,
    token: Option<String>,
) -> Result<Vec<WorkspaceRelease>, String> {
    match provider {
        "github" | "local" => {
            if owner.is_empty() {
                return Err(format!("Owner is missing for repository {name}."));
            }
            let repo = RepositoryIdentity {
                id: GitHubRepositoryId::from_u64(0),
                owner: AccountLogin::new(owner).map_err(|e| e.to_string())?,
                name: RepositoryName::new(name).map_err(|e| e.to_string())?,
            };
            match HttpGitHubGateway::new(token).fetch_releases(&repo) {
                Ok(releases) => Ok(releases.into_iter().map(WorkspaceRelease::from).collect()),
                Err(e) => {
                    if provider == "local" {
                        Err(format!("No releases found for local repository {name}."))
                    } else {
                        Err(e.to_string())
                    }
                }
            }
        }
        "gitlab" => fetch_gitlab_releases(owner, name, token.as_deref()),
        _ => Err(format!(
            "Release fetching is not yet supported for {provider}."
        )),
    }
}

/// Try to load releases from local `.release-desk/data.json` or `data.json` if available.
pub fn load_snapshot_releases(
    project_dir: &Path,
    repo_name: &str,
) -> Option<Vec<WorkspaceRelease>> {
    let dot_rd = project_dir.join(".release-desk").join("data.json");
    let root_data = project_dir.join("data.json");
    let target = if dot_rd.is_file() {
        Some(dot_rd)
    } else if root_data.is_file() {
        Some(root_data)
    } else {
        None
    }?;

    let content = std::fs::read_to_string(target).ok()?;
    let snapshot = release_desk_core::parse_snapshot(&content).ok()?;
    let repo = snapshot.repos.into_iter().find(|r| {
        r.name.eq_ignore_ascii_case(repo_name)
            || repo_name.to_lowercase().contains(&r.name.to_lowercase())
    })?;

    let releases = repo.releases?;
    if releases.nodes.is_empty() {
        return None;
    }

    Some(
        releases
            .nodes
            .into_iter()
            .map(WorkspaceRelease::from)
            .collect(),
    )
}

/// Fetch release history with snapshot discovery fallback.
pub fn fetch_releases_with_project(
    provider: &str,
    owner: &str,
    name: &str,
    token: Option<String>,
    project_dir: Option<&Path>,
) -> Result<Vec<WorkspaceRelease>, String> {
    if let Some(dir) = project_dir {
        if let Some(snapshot_rels) = load_snapshot_releases(dir, name) {
            return Ok(snapshot_rels);
        }
    }
    fetch_releases(provider, owner, name, token)
}

#[derive(Deserialize)]
struct GitLabLink {
    name: String,
    url: String,
    #[serde(default)]
    direct_asset_url: Option<String>,
}
#[derive(Deserialize)]
struct GitLabAssets {
    #[serde(default)]
    links: Vec<GitLabLink>,
}
#[derive(Deserialize)]
struct GitLabRelease {
    tag_name: String,
    description: Option<String>,
    released_at: Option<String>,
    assets: GitLabAssets,
}

impl From<GitLabRelease> for WorkspaceRelease {
    fn from(release: GitLabRelease) -> Self {
        Self {
            tag_name: release.tag_name,
            description: release.description,
            is_draft: false,
            is_prerelease: None,
            published_at: release.released_at,
            assets: release
                .assets
                .links
                .into_iter()
                .map(|link| WorkspaceAsset {
                    name: link.name,
                    download_url: link.url,
                    size: None,
                })
                .collect(),
        }
    }
}

fn gitlab_releases_url(owner: &str, name: &str) -> Result<reqwest::Url, String> {
    if owner.trim().is_empty()
        || name.trim().is_empty()
        || owner.chars().chain(name.chars()).any(char::is_control)
    {
        return Err("Invalid GitLab project path.".into());
    }
    let mut url =
        reqwest::Url::parse("https://gitlab.com/api/v4/projects/").map_err(|e| e.to_string())?;
    url.path_segments_mut()
        .map_err(|_| "Invalid GitLab API URL.")?
        .pop_if_empty()
        .push(&format!("{owner}/{name}"))
        .push("releases");
    Ok(url)
}

fn fetch_gitlab_releases(
    owner: &str,
    name: &str,
    token: Option<&str>,
) -> Result<Vec<WorkspaceRelease>, String> {
    let url = gitlab_releases_url(owner, name)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Release-Desk/1.0")
        .build()
        .map_err(|e| format!("Cannot create GitLab client: {e}"))?;
    fetch_gitlab_pages(|page| {
        let mut request = client
            .get(url.clone())
            .query(&[("per_page", 100), ("page", page)]);
        if let Some(token) = token.filter(|token| !token.trim().is_empty()) {
            request = request.header("PRIVATE-TOKEN", token);
        }
        let response = request
            .send()
            .map_err(|e| format!("GitLab release request failed on page {page}: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "GitLab release request failed on page {page}: HTTP {}",
                response.status()
            ));
        }
        response
            .json()
            .map_err(|e| format!("Invalid GitLab release data on page {page}: {e}"))
    })
}

fn fetch_gitlab_pages(
    mut fetch: impl FnMut(u32) -> Result<Vec<GitLabRelease>, String>,
) -> Result<Vec<WorkspaceRelease>, String> {
    let mut releases = Vec::new();
    let mut page = 1u32;
    loop {
        let results = fetch(page)?;
        let done = results.len() < 100;
        releases.extend(results.into_iter().map(WorkspaceRelease::from));
        if done {
            break;
        }
        page = page
            .checked_add(1)
            .ok_or("Too many GitLab release pages.")?;
    }
    Ok(releases)
}

fn component(value: &str) -> Result<&str, String> {
    let mut parts = Path::new(value).components();
    if value.is_empty()
        || value.contains('\\')
        || !matches!(parts.next(), Some(Component::Normal(_)))
        || parts.next().is_some()
    {
        return Err("Invalid release path component.".into());
    }
    Ok(value)
}

// Encode provider namespaces and tags as single path components. Existing simple
// names retain their paths; escaping '%' keeps distinct inputs distinct.
fn storage_component(value: &str) -> Result<String, String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("Invalid release path component.".into());
    }
    let mut encoded = String::new();
    for ch in value.chars() {
        match ch {
            '/' => encoded.push_str("%2F"),
            '\\' => encoded.push_str("%5C"),
            '%' => encoded.push_str("%25"),
            _ => encoded.push(ch),
        }
    }
    if encoded == "." || encoded == ".." {
        return Err("Invalid release path component.".into());
    }
    component(&encoded)?;
    Ok(encoded)
}

/// Observed local file state. A size match is not proof of file identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAssetState {
    Missing,
    SizeMatches,
    SizeDiffers,
    PresentSizeUnknown,
    Unavailable,
}

/// Inspect the expected destination without following paths outside the project.
pub fn local_asset_state(
    project: &Path,
    owner: &str,
    repo: &str,
    tag: &str,
    name: &str,
    size: Option<u64>,
) -> LocalAssetState {
    let Ok(root) = project.canonicalize() else {
        return LocalAssetState::Unavailable;
    };
    let Ok(owner_comp) = storage_component(owner) else {
        return LocalAssetState::Unavailable;
    };
    let Ok(repo_comp) = component(repo) else {
        return LocalAssetState::Unavailable;
    };
    let Ok(tag_comp) = storage_component(tag) else {
        return LocalAssetState::Unavailable;
    };
    let Ok(name_comp) = component(name) else {
        return LocalAssetState::Unavailable;
    };

    let candidates = [
        root.join(".release-desk")
            .join("installers")
            .join(&owner_comp)
            .join(repo_comp)
            .join(&tag_comp)
            .join(name_comp),
        root.join("Release Dmgs").join(name_comp),
        root.join("releases").join(name_comp),
        root.join("installers").join(name_comp),
        root.join(name_comp),
    ];

    let mut found_state = LocalAssetState::Missing;

    for candidate in &candidates {
        if let Ok(actual) = candidate.canonicalize() {
            if actual.starts_with(&root) {
                if let Ok(metadata) = std::fs::metadata(&actual) {
                    if metadata.is_file() {
                        if size.is_none() {
                            return LocalAssetState::PresentSizeUnknown;
                        } else if Some(metadata.len()) == size {
                            return LocalAssetState::SizeMatches;
                        } else {
                            found_state = LocalAssetState::SizeDiffers;
                        }
                    }
                }
            }
        }
    }

    found_state
}

/// Inspect the project shelf for previous builds archived in `old/` subdirectories.
pub fn local_archived_files(project: &Path, repo_name: &str) -> Vec<(String, u64)> {
    let Ok(root) = project.canonicalize() else {
        return Vec::new();
    };
    let old_dirs = [
        root.join("Release Dmgs").join("old"),
        root.join("releases").join("old"),
        root.join(".release-desk").join("installers").join("old"),
    ];

    let mut results = Vec::new();
    let lower_repo = repo_name.to_lowercase();

    for dir in &old_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    let lower_file = file_name.to_lowercase();
                    if lower_file.contains(&lower_repo) {
                        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        results.push((file_name, len));
                    }
                }
            }
        }
    }

    results
}

fn comparable_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.last() != Some(&b'Z') {
        return false;
    }
    for (index, byte) in bytes.iter().take(19).enumerate() {
        let expected = match index {
            4 | 7 => Some(b'-'),
            10 => Some(b'T'),
            13 | 16 => Some(b':'),
            _ => None,
        };
        if expected.map_or(!byte.is_ascii_digit(), |expected| *byte != expected) {
            return false;
        }
    }
    bytes.len() == 20
        || (bytes[19] == b'.'
            && bytes.len() > 21
            && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit))
}

/// Identify a newer dated release when an older fetched release has a local file.
/// This only compares consistently formatted UTC timestamps and does not verify file contents.
pub fn newer_release_with_local_older<'a>(
    project: &Path,
    owner: &str,
    repo: &str,
    releases: &'a [WorkspaceRelease],
) -> Option<(&'a str, &'a str)> {
    let eligible: Vec<_> = releases
        .iter()
        .filter(|release| {
            !release.is_draft && release.is_prerelease != Some(true) && !release.assets.is_empty()
        })
        .collect();
    let dates: Vec<_> = eligible
        .iter()
        .map(|release| release.published_at.as_deref())
        .collect::<Option<_>>()?;
    let width = dates.first()?.len();
    if dates
        .iter()
        .any(|date| date.len() != width || !comparable_utc_timestamp(date))
    {
        return None;
    }
    let latest = eligible
        .iter()
        .max_by_key(|release| release.published_at.as_deref())?;
    if !latest.assets.iter().all(|asset| {
        local_asset_state(
            project,
            owner,
            repo,
            &latest.tag_name,
            &asset.name,
            asset.size,
        ) == LocalAssetState::Missing
    }) {
        return None;
    }
    let old = eligible
        .iter()
        .filter(|release| {
            release.published_at < latest.published_at
                && release.assets.iter().any(|asset| {
                    matches!(
                        local_asset_state(
                            project,
                            owner,
                            repo,
                            &release.tag_name,
                            &asset.name,
                            asset.size
                        ),
                        LocalAssetState::SizeMatches
                            | LocalAssetState::SizeDiffers
                            | LocalAssetState::PresentSizeUnknown
                    )
                })
        })
        .max_by_key(|release| release.published_at.as_deref())?;
    Some((&latest.tag_name, &old.tag_name))
}

/// Download a public HTTPS asset into a project, without overwriting an existing file.
/// A temporary file is removed on failure and the supplied size is verified.
pub fn download_asset(
    project: &Path,
    owner: &str,
    repo: &str,
    tag: &str,
    name: &str,
    url: &str,
    expected_size: Option<u64>,
) -> Result<PathBuf, String> {
    let parsed = safe_download_url(url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .user_agent("Release-Desk/1.0")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("Too many redirects")
            } else if attempt.url().scheme() != "https"
                || !attempt.url().username().is_empty()
                || attempt.url().password().is_some()
            {
                attempt.error("Unsafe download redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(parsed)
        .send()
        .map_err(|e| format!("Download failed: {}", e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!(
            "Download failed: HTTP {}. Private assets may require authentication.",
            response.status()
        ));
    }
    save_asset(
        project,
        owner,
        repo,
        tag,
        name,
        expected_size,
        &mut response,
    )
}

/// Download a GitHub release asset with a token. The token is sent only to the
/// fixed GitHub API host, never to the redirected download host.
#[allow(clippy::too_many_arguments)]
pub fn download_github_asset(
    project: &Path,
    owner: &str,
    repo: &str,
    tag: &str,
    name: &str,
    expected_size: Option<u64>,
    token: &str,
) -> Result<PathBuf, String> {
    if token.trim().is_empty() {
        return Err("GitHub authentication is required.".into());
    }
    component(owner)?;
    component(repo)?;
    component(name)?;
    storage_component(tag)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .user_agent("Release-Desk/1.0")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut url = reqwest::Url::parse("https://api.github.com/").map_err(|e| e.to_string())?;
    url.path_segments_mut()
        .map_err(|_| "Invalid GitHub API URL.")?
        .pop_if_empty()
        .extend(["repos", owner, repo, "releases", "tags", tag]);
    let response = client
        .get(url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("GitHub release lookup failed: {}", e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub release lookup failed: HTTP {}",
            response.status()
        ));
    }
    let release: GitHubAssetList = response
        .json()
        .map_err(|e| format!("Could not read GitHub release: {e}"))?;
    let asset_id = find_github_asset(&release, name, expected_size)?;
    let mut asset_url =
        reqwest::Url::parse("https://api.github.com/").map_err(|e| e.to_string())?;
    asset_url
        .path_segments_mut()
        .map_err(|_| "Invalid GitHub API URL.")?
        .pop_if_empty()
        .extend([
            "repos",
            owner,
            repo,
            "releases",
            "assets",
            &asset_id.to_string(),
        ]);
    let mut response = client
        .get(asset_url)
        .bearer_auth(token)
        .header("Accept", "application/octet-stream")
        .send()
        .map_err(|e| format!("GitHub asset request failed: {}", e.without_url()))?;
    let redirected = response.status().is_redirection();
    if redirected {
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or("GitHub did not supply a download location.")?
            .to_str()
            .map_err(|_| "Invalid download location.")?;
        let url = safe_download_url(location)?;
        // A separate unauthenticated client follows storage redirects. It never receives the token.
        let public_client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .user_agent("Release-Desk/1.0")
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("Too many redirects")
                } else if attempt.url().scheme() != "https"
                    || !attempt.url().username().is_empty()
                    || attempt.url().password().is_some()
                {
                    attempt.error("Unsafe download redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|e| e.to_string())?;
        response = public_client
            .get(url)
            .send()
            .map_err(|e| format!("Download failed: {}", e.without_url()))?;
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub asset download failed: HTTP {}",
            response.status()
        ));
    }
    if !redirected
        && response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"))
    {
        return Err("GitHub returned asset metadata instead of a file.".into());
    }
    save_asset(
        project,
        owner,
        repo,
        tag,
        name,
        expected_size,
        &mut response,
    )
}

/// Download a private GitLab asset through its authenticated direct release link.
/// The token is sent only to gitlab.com; redirects to external hosts never receive it.
/// Links without GitLab direct asset URLs cannot be downloaded with this method.
#[allow(clippy::too_many_arguments)]
pub fn download_gitlab_asset(
    project: &Path,
    owner: &str,
    repo: &str,
    tag: &str,
    name: &str,
    original_url: &str,
    expected_size: Option<u64>,
    token: &str,
) -> Result<PathBuf, String> {
    if token.trim().is_empty() {
        return Err("GitLab authentication is required.".into());
    }
    component(repo)?;
    component(name)?;
    storage_component(owner)?;
    storage_component(tag)?;
    let mut lookup = gitlab_releases_url(owner, repo)?;
    lookup
        .path_segments_mut()
        .map_err(|_| "Invalid GitLab API URL.")?
        .push(tag);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .user_agent("Release-Desk/1.0")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(lookup)
        .header("PRIVATE-TOKEN", token)
        .send()
        .map_err(|e| format!("GitLab release lookup failed: {}", e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!(
            "GitLab release lookup failed: HTTP {}",
            response.status()
        ));
    }
    let release: GitLabRelease = response
        .json()
        .map_err(|e| format!("Could not read GitLab release: {e}"))?;
    let mut url = gitlab_direct_url(&release.assets.links, name, original_url)?;
    let mut authenticated = true;
    for _ in 0..10 {
        let mut request = client.get(url.clone());
        if authenticated && trusted_gitlab_host(&url) {
            request = request.header("PRIVATE-TOKEN", token);
        } else {
            authenticated = false;
        }
        let mut response = request
            .send()
            .map_err(|e| format!("GitLab asset request failed: {}", e.without_url()))?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .ok_or("GitLab did not supply a download location.")?
                .to_str()
                .map_err(|_| "Invalid download location.")?;
            let next = url
                .join(location)
                .map_err(|_| "Invalid download location.")?;
            url = safe_download_url(next.as_str())?;
            authenticated = redirect_keeps_gitlab_auth(authenticated, &url);
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "GitLab asset download failed: HTTP {}",
                response.status()
            ));
        }
        if response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/html") || v.starts_with("application/json"))
        {
            return Err("GitLab returned a page or metadata instead of a file.".into());
        }
        return save_asset(
            project,
            owner,
            repo,
            tag,
            name,
            expected_size,
            &mut response,
        );
    }
    Err("Too many download redirects.".into())
}

fn trusted_gitlab_host(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("gitlab.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
}

fn redirect_keeps_gitlab_auth(authenticated: bool, next: &reqwest::Url) -> bool {
    authenticated && trusted_gitlab_host(next)
}

fn gitlab_direct_url(
    links: &[GitLabLink],
    name: &str,
    original_url: &str,
) -> Result<reqwest::Url, String> {
    let mut matches = links.iter().filter(|link| link.name == name);
    let link = matches
        .next()
        .ok_or("The asset is no longer present in this release.")?;
    if matches.next().is_some() {
        return Err("Release contains duplicate asset names.".into());
    }
    if link.url != original_url {
        return Err(
            "The asset changed since the release was loaded. Refresh and try again.".into(),
        );
    }
    let direct = link
        .direct_asset_url
        .as_deref()
        .ok_or("This asset has no GitLab direct download link; private download is unavailable.")?;
    let url = safe_download_url(direct)?;
    if !trusted_gitlab_host(&url) {
        return Err("GitLab returned an untrusted direct download location.".into());
    }
    Ok(url)
}

#[derive(Deserialize)]
struct GitHubAssetList {
    assets: Vec<GitHubAssetInfo>,
}
#[derive(Deserialize)]
struct GitHubAssetInfo {
    id: u64,
    name: String,
    size: u64,
}

fn find_github_asset(
    release: &GitHubAssetList,
    name: &str,
    expected: Option<u64>,
) -> Result<u64, String> {
    let mut matching = release.assets.iter().filter(|asset| asset.name == name);
    let asset = matching
        .next()
        .ok_or("The asset is no longer present in this release.")?;
    if matching.next().is_some() {
        return Err("Release contains duplicate asset names.".into());
    }
    if expected.is_some_and(|size| size != asset.size) {
        return Err(
            "The asset changed since the release was loaded. Refresh and try again.".into(),
        );
    }
    Ok(asset.id)
}

fn safe_download_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "Invalid asset URL.")?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err("Only HTTPS asset URLs without embedded credentials are supported.".into());
    }
    Ok(url)
}

fn save_asset(
    project: &Path,
    owner: &str,
    repo: &str,
    tag: &str,
    name: &str,
    expected_size: Option<u64>,
    input: &mut impl Read,
) -> Result<PathBuf, String> {
    let root = project
        .canonicalize()
        .map_err(|e| format!("Cannot access project folder: {e}"))?;
    let owner = storage_component(owner)?;
    let tag = storage_component(tag)?;
    let components = [
        ".release-desk",
        "installers",
        owner.as_str(),
        component(repo)?,
        tag.as_str(),
    ];
    component(name)?;
    let mut folder = root.clone();
    for part in components {
        let next = folder.join(part);
        if !next.exists() {
            std::fs::create_dir(&next)
                .map_err(|e| format!("Cannot create download folder: {e}"))?;
        }
        folder = next
            .canonicalize()
            .map_err(|e| format!("Cannot access download folder: {e}"))?;
        if !folder.starts_with(&root) {
            return Err("Download folder resolves outside the project.".into());
        }
    }
    let destination = folder.join(component(name)?);
    let mut temp = tempfile::NamedTempFile::new_in(&folder)
        .map_err(|e| format!("Cannot create temporary download: {e}"))?;
    copy_verified(input, &mut temp, expected_size)?;
    temp.flush()
        .map_err(|e| format!("Cannot flush download: {e}"))?;
    temp.persist_noclobber(&destination).map_err(|e| {
        format!(
            "Cannot save download (the file may already exist): {}",
            e.error
        )
    })?;
    Ok(destination)
}

fn copy_verified(
    input: &mut impl Read,
    output: &mut impl Write,
    expected_size: Option<u64>,
) -> Result<(), String> {
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|e| format!("Download interrupted: {e}"))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or("Download size overflow.")?;
        if expected_size.is_some_and(|size| total > size) {
            return Err("Download exceeded the expected asset size.".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| format!("Cannot save download: {e}"))?;
    }
    if let Some(size) = expected_size {
        if total != size {
            return Err(format!(
                "Download size mismatch: expected {size} bytes, received {total}."
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_release(tag: &str) -> GitLabRelease {
        serde_json::from_value(serde_json::json!({
            "tag_name": tag, "description": null, "released_at": null,
            "assets": {"links": []}
        }))
        .unwrap()
    }

    #[test]
    fn gitlab_private_download_checks_identity_and_direct_host() {
        let fixture = |direct: Option<&str>| GitLabLink {
            name: "asset.zip".into(),
            url: "https://storage.example/asset.zip".into(),
            direct_asset_url: direct.map(str::to_string),
        };
        let good = fixture(Some(
            "https://gitlab.com/group/repo/-/releases/v1/downloads/asset.zip",
        ));
        assert!(
            gitlab_direct_url(&[good], "asset.zip", "https://storage.example/asset.zip").is_ok()
        );
        assert!(
            gitlab_direct_url(
                &[fixture(None)],
                "asset.zip",
                "https://storage.example/asset.zip"
            )
            .is_err()
        );
        assert!(
            gitlab_direct_url(
                &[fixture(Some("https://gitlab.com.evil.example/file"))],
                "asset.zip",
                "https://storage.example/asset.zip"
            )
            .is_err()
        );
        assert!(
            gitlab_direct_url(
                &[fixture(Some("https://gitlab.com/file"))],
                "asset.zip",
                "https://changed.example/file"
            )
            .is_err()
        );
        assert!(
            gitlab_direct_url(
                &[
                    fixture(Some("https://gitlab.com/file")),
                    fixture(Some("https://gitlab.com/file"))
                ],
                "asset.zip",
                "https://storage.example/asset.zip"
            )
            .is_err()
        );
    }

    #[test]
    fn gitlab_redirect_never_restores_token_after_leaving_host() {
        let gitlab = reqwest::Url::parse("https://gitlab.com/file").unwrap();
        let other = reqwest::Url::parse("https://storage.example/file").unwrap();
        let bad_port = reqwest::Url::parse("https://gitlab.com:444/file").unwrap();
        assert!(redirect_keeps_gitlab_auth(true, &gitlab));
        assert!(!redirect_keeps_gitlab_auth(true, &other));
        assert!(!redirect_keeps_gitlab_auth(true, &bad_port));
        assert!(!redirect_keeps_gitlab_auth(false, &gitlab));
    }

    #[test]
    fn gitlab_pagination_keeps_every_page_and_stops_after_short_page() {
        let mut pages = Vec::new();
        let releases = fetch_gitlab_pages(|page| {
            pages.push(page);
            Ok(if page == 1 {
                (0..100)
                    .map(|i| fixture_release(&format!("v{i}")))
                    .collect()
            } else {
                vec![fixture_release("last")]
            })
        })
        .unwrap();
        assert_eq!(pages, vec![1, 2]);
        assert_eq!(releases.len(), 101);
        assert_eq!(releases[100].tag_name, "last");
    }

    #[test]
    fn gitlab_page_failure_is_not_reported_as_partial_success() {
        let result = fetch_gitlab_pages(|page| {
            if page == 1 {
                Ok((0..100).map(|_| fixture_release("v1")).collect())
            } else {
                Err("HTTP 429 on page 2".into())
            }
        });
        assert_eq!(result.unwrap_err(), "HTTP 429 on page 2");
    }

    #[test]
    fn github_asset_selection_checks_identity_and_size() {
        let release: GitHubAssetList =
            serde_json::from_str(r#"{"assets":[{"id":42,"name":"app.zip","size":10}]}"#).unwrap();
        assert_eq!(
            find_github_asset(&release, "app.zip", Some(10)).unwrap(),
            42
        );
        assert!(find_github_asset(&release, "app.zip", Some(11)).is_err());
        assert!(find_github_asset(&release, "other.zip", None).is_err());
        let duplicate: GitHubAssetList = serde_json::from_str(
            r#"{"assets":[{"id":1,"name":"a","size":1},{"id":2,"name":"a","size":1}]}"#,
        )
        .unwrap();
        assert!(find_github_asset(&duplicate, "a", None).is_err());
        assert!(safe_download_url("http://example.com/file").is_err());
        assert!(safe_download_url("https://user:pass@example.com/file").is_err());
    }

    #[test]
    fn asset_stream_checks_size_and_handles_unknown_size() {
        let mut output = Vec::new();
        copy_verified(&mut &b"asset data"[..], &mut output, Some(10)).unwrap();
        assert_eq!(output, b"asset data");
        let mut output = Vec::new();
        assert!(
            copy_verified(&mut &b"asset data"[..], &mut output, Some(11))
                .unwrap_err()
                .contains("size mismatch")
        );
        let mut oversized = Vec::new();
        assert!(
            copy_verified(&mut &b"asset data"[..], &mut oversized, Some(4))
                .unwrap_err()
                .contains("exceeded")
        );
        assert!(oversized.is_empty());
        copy_verified(&mut &b"asset data"[..], &mut Vec::new(), None).unwrap();
    }

    #[test]
    fn asset_stream_reports_interrupted_reads() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("connection reset"))
            }
        }
        assert!(
            copy_verified(&mut Broken, &mut Vec::new(), None)
                .unwrap_err()
                .contains("connection reset")
        );
    }

    #[test]
    fn rejects_insecure_or_credential_bearing_asset_urls() {
        let root = tempfile::tempdir().unwrap();
        for url in [
            "http://example.com/file",
            "https://user:pass@example.com/file",
        ] {
            let result = download_asset(root.path(), "o", "r", "v1", "file", url, None);
            assert!(result.is_err());
        }
        assert!(!root.path().join(".release-desk").exists());
    }

    #[test]
    fn gitlab_project_path_is_url_encoded() {
        let url = gitlab_releases_url("group/subgroup", "project").unwrap();
        assert_eq!(
            url.as_str(),
            "https://gitlab.com/api/v4/projects/group%2Fsubgroup%2Fproject/releases"
        );
    }
    #[test]
    fn gitlab_assets_have_unknown_size_and_prerelease_state() {
        let raw = r#"{"tag_name":"v1","description":"notes","released_at":"2025-01-01T00:00:00Z","assets":{"links":[{"name":"build.zip","url":"https://example.com/build.zip"}]}}"#;
        let release: WorkspaceRelease = serde_json::from_str::<GitLabRelease>(raw).unwrap().into();
        assert_eq!(release.is_prerelease, None);
        assert_eq!(release.assets[0].size, None);
        assert_eq!(release.assets[0].name, "build.zip");
    }
    #[test]
    fn newer_release_requires_comparable_dates_and_an_older_local_file() {
        let root = tempfile::tempdir().unwrap();
        let asset = WorkspaceAsset {
            name: "app.zip".into(),
            download_url: "https://example.com/app.zip".into(),
            size: Some(2),
        };
        let make = |tag: &str, date: &str| WorkspaceRelease {
            tag_name: tag.into(),
            description: None,
            is_draft: false,
            is_prerelease: Some(false),
            published_at: Some(date.into()),
            assets: vec![asset.clone()],
        };
        let mut releases = vec![
            make("v1", "2025-01-01T00:00:00Z"),
            make("v2", "2025-02-01T00:00:00Z"),
        ];
        assert_eq!(
            newer_release_with_local_older(root.path(), "o", "r", &releases),
            None
        );
        let folder = root.path().join(".release-desk/installers/o/r/v1");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("app.zip"), b"ok").unwrap();
        assert_eq!(
            newer_release_with_local_older(root.path(), "o", "r", &releases),
            Some(("v2", "v1"))
        );
        releases[1].published_at = Some("xxxx-02-01T00:00:00Z".into());
        assert_eq!(
            newer_release_with_local_older(root.path(), "o", "r", &releases),
            None
        );
        releases[1].published_at = None;
        assert_eq!(
            newer_release_with_local_older(root.path(), "o", "r", &releases),
            None
        );
    }

    #[test]
    fn namespaces_and_tags_are_encoded_without_collisions() {
        assert_eq!(
            storage_component("group/subgroup").unwrap(),
            "group%2Fsubgroup"
        );
        assert_eq!(
            storage_component("group%2Fsubgroup").unwrap(),
            "group%252Fsubgroup"
        );
        assert_eq!(storage_component("release/1").unwrap(), "release%2F1");
        assert!(storage_component("..").is_err());
        let root = tempfile::tempdir().unwrap();
        let folder = root
            .path()
            .join(".release-desk/installers/group%2Fsubgroup/repo/release%2F1");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("asset.zip"), b"ok").unwrap();
        assert_eq!(
            local_asset_state(
                root.path(),
                "group/subgroup",
                "repo",
                "release/1",
                "asset.zip",
                Some(2)
            ),
            LocalAssetState::SizeMatches
        );
    }

    #[test]
    fn path_components_reject_traversal() {
        for name in ["", ".", "..", "a/b", "a\\b", "/tmp"] {
            assert!(component(name).is_err(), "{name}");
        }
        assert_eq!(component("v1.0-rc1").unwrap(), "v1.0-rc1");
    }
    #[test]
    fn unsupported_provider_is_not_empty_success() {
        assert!(fetch_releases("bitbucket", "example", "repo", None).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_cannot_create_download_folders_outside_project() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join(".release-desk")).unwrap();
        let result = download_asset(
            root.path(),
            "owner",
            "repo",
            "v1",
            "file.zip",
            "https://example.com/file.zip",
            Some(0),
        );
        assert!(result.is_err());
        assert!(!outside.path().join("installers").exists());
    }
    #[test]
    fn local_state_reports_size_without_claiming_identity() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join(".release-desk/installers/o/r/v1");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.zip"), b"abc").unwrap();
        assert_eq!(
            local_asset_state(root.path(), "o", "r", "v1", "a.zip", Some(3)),
            LocalAssetState::SizeMatches
        );
        assert_eq!(
            local_asset_state(root.path(), "o", "r", "v1", "a.zip", Some(5)),
            LocalAssetState::SizeDiffers
        );
        assert_eq!(
            local_asset_state(root.path(), "o", "r", "v1", "missing", Some(3)),
            LocalAssetState::Missing
        );
    }
}
