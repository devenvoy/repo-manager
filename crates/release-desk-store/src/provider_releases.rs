//! Provider-specific release metadata adapters. The store consumes only WorkspaceRelease.

use crate::workspace_releases::{WorkspaceAsset, WorkspaceRelease};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize)]
struct ForgeAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
}

#[derive(Deserialize)]
struct ForgeRelease {
    tag_name: String,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<ForgeAsset>,
}

#[derive(Deserialize)]
struct DeclaredCatalog {
    #[serde(default)]
    releases: Vec<DeclaredRelease>,
}

#[derive(Deserialize)]
struct DeclaredRelease {
    tag: String,
    notes: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<DeclaredAsset>,
}

#[derive(Deserialize)]
struct DeclaredAsset {
    name: String,
    url: String,
    size: Option<u64>,
}

/// Read an optional repository-owned artifact map for providers whose build
/// artifacts are not attached to Git releases.
pub fn load_declared_releases(repo_dir: &Path) -> Result<Option<Vec<WorkspaceRelease>>, String> {
    let path = repo_dir.join("release-desk-store.json");
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot read repository store manifest: {error}")),
    };
    let declared: DeclaredCatalog = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid repository store manifest: {error}"))?;
    if declared.releases.is_empty() {
        return Ok(None);
    }
    let releases = declared
        .releases
        .into_iter()
        .map(|release| {
            if release.tag.trim().is_empty() {
                return Err("Declared release has no version tag.".to_string());
            }
            let assets = release
                .assets
                .into_iter()
                .map(|asset| {
                    if !safe_segment(&asset.name) {
                        return Err("Declared asset has an unsafe file name.".to_string());
                    }
                    let url = reqwest::Url::parse(&asset.url)
                        .map_err(|_| "Declared asset URL is invalid.")?;
                    if url.scheme() != "https"
                        || url.host_str().is_none()
                        || !url.username().is_empty()
                        || url.password().is_some()
                    {
                        return Err(
                            "Declared asset URL must use HTTPS without embedded credentials."
                                .into(),
                        );
                    }
                    Ok(WorkspaceAsset {
                        name: asset.name,
                        download_url: url.to_string(),
                        size: asset.size,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(WorkspaceRelease {
                tag_name: release.tag,
                description: release.notes,
                is_draft: false,
                is_prerelease: Some(false),
                published_at: release.published_at,
                assets,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Some(releases))
}

impl From<ForgeRelease> for WorkspaceRelease {
    fn from(item: ForgeRelease) -> Self {
        Self {
            tag_name: item.tag_name,
            description: item.body,
            is_draft: item.draft,
            is_prerelease: Some(item.prerelease),
            published_at: item.published_at,
            assets: item
                .assets
                .into_iter()
                .map(|asset| WorkspaceAsset {
                    name: asset.name,
                    download_url: asset.browser_download_url,
                    size: asset.size,
                })
                .collect(),
        }
    }
}

/// Gitea, Forgejo, and Codeberg expose the same release API shape.
pub fn fetch_forge_releases(
    base: &str,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Vec<WorkspaceRelease>, String> {
    let mut url = safe_base(base)?;
    if !safe_segment(owner) || !safe_segment(repo) {
        return Err("Invalid repository identity.".into());
    }
    url.path_segments_mut()
        .map_err(|_| "Invalid provider URL.")?
        .pop_if_empty()
        .extend(["api", "v1", "repos", owner, repo, "releases"]);
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Release-Desk/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let mut releases = Vec::new();
    for page in 1..=20 {
        let mut request = client
            .get(url.clone())
            .query(&[("limit", 50), ("page", page)]);
        if let Some(token) = token.filter(|value| !value.is_empty()) {
            request = request.header("Authorization", format!("token {token}"));
        }
        let response = request
            .send()
            .map_err(|e| format!("Release lookup failed: {}", e.without_url()))?;
        if !response.status().is_success() {
            return Err(format!("Release lookup failed: HTTP {}", response.status()));
        }
        let page_items: Vec<ForgeRelease> = response
            .json()
            .map_err(|e| format!("Invalid release data: {e}"))?;
        let done = page_items.is_empty();
        releases.extend(page_items.into_iter().map(WorkspaceRelease::from));
        if done {
            break;
        }
    }
    Ok(releases)
}

#[derive(Deserialize)]
struct BitbucketDownloads {
    #[serde(default)]
    values: Vec<BitbucketDownload>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct BitbucketDownload {
    name: String,
    size: Option<u64>,
    created_on: Option<String>,
}

/// Bitbucket Cloud stores release files in repository Downloads rather than GitHub-style releases.
pub fn fetch_bitbucket_downloads(
    workspace: &str,
    repo: &str,
    token: Option<&str>,
    username: Option<&str>,
) -> Result<Vec<WorkspaceRelease>, String> {
    if !safe_segment(workspace) || !safe_segment(repo) {
        return Err("Invalid repository identity.".into());
    }
    let mut url =
        reqwest::Url::parse("https://api.bitbucket.org/2.0/").map_err(|e| e.to_string())?;
    url.path_segments_mut()
        .map_err(|_| "Invalid Bitbucket URL.")?
        .pop_if_empty()
        .extend(["repositories", workspace, repo, "downloads"]);
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Release-Desk/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let mut releases = Vec::new();
    for _ in 0..20 {
        let mut request = client.get(url.clone());
        if let Some(token) = token.filter(|value| !value.is_empty()) {
            request = request.bearer_auth(token);
        }
        let mut response = request
            .send()
            .map_err(|e| format!("Download list failed: {}", e.without_url()))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            && let (Some(token), Some(username)) = (token, username)
        {
            response = client
                .get(url.clone())
                .basic_auth(username, Some(token))
                .send()
                .map_err(|e| format!("Download list failed: {}", e.without_url()))?;
        }
        if !response.status().is_success() {
            return Err(format!("Download list failed: HTTP {}", response.status()));
        }
        let page: BitbucketDownloads = response
            .json()
            .map_err(|e| format!("Invalid download list: {e}"))?;
        for asset in page.values {
            if !safe_segment(&asset.name) {
                continue;
            }
            let mut asset_url =
                reqwest::Url::parse("https://api.bitbucket.org/2.0/").map_err(|e| e.to_string())?;
            asset_url
                .path_segments_mut()
                .map_err(|_| "Invalid Bitbucket URL.")?
                .pop_if_empty()
                .extend(["repositories", workspace, repo, "downloads", &asset.name]);
            let tag = version_in_filename(&asset.name).unwrap_or_else(|| asset.name.clone());
            releases.push(WorkspaceRelease {
                tag_name: tag,
                description: None,
                is_draft: false,
                is_prerelease: None,
                published_at: asset.created_on,
                assets: vec![WorkspaceAsset {
                    name: asset.name,
                    download_url: asset_url.to_string(),
                    size: asset.size,
                }],
            });
        }
        let Some(next) = page.next else { break };
        let parsed = reqwest::Url::parse(&next).map_err(|_| "Invalid Bitbucket pagination URL.")?;
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("api.bitbucket.org")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("Unsafe Bitbucket pagination URL.".into());
        }
        url = parsed;
    }
    releases.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    Ok(releases)
}

pub(crate) fn safe_base(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "Invalid provider URL.")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Provider URL must be a clean HTTPS address.".into());
    }
    Ok(url)
}

fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
}

fn version_in_filename(name: &str) -> Option<String> {
    name.split(['-', '_', ' '])
        .find(|part| {
            part.trim_start_matches('v')
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit())
                && part.contains('.')
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_urls_and_versions_are_checked() {
        assert!(safe_base("https://codeberg.org/").is_ok());
        assert!(safe_base("http://codeberg.org/").is_err());
        assert!(safe_base("https://user:secret@codeberg.org/").is_err());
        assert_eq!(
            version_in_filename("MyApp-v1.2.3-macos.dmg"),
            Some("v1.2.3".into())
        );
    }

    #[test]
    fn forge_release_metadata_preserves_assets_and_notes() {
        let raw = r#"{"tag_name":"v2.0","body":"New export","draft":false,"prerelease":false,"published_at":"2026-10-01T00:00:00Z","assets":[{"name":"app.dmg","browser_download_url":"https://codeberg.org/attachments/42","size":1024}]}"#;
        let release: ForgeRelease = serde_json::from_str(raw).unwrap();
        let converted = WorkspaceRelease::from(release);
        assert_eq!(converted.tag_name, "v2.0");
        assert_eq!(converted.description.as_deref(), Some("New export"));
        assert_eq!(converted.assets[0].size, Some(1024));
    }

    #[test]
    fn bitbucket_download_list_reads_pagination_and_file_metadata() {
        let raw = r#"{"values":[{"name":"app-v1.2.3-macos.dmg","size":2048,"created_on":"2026-10-01T00:00:00Z"}],"next":"https://api.bitbucket.org/2.0/repositories/a/b/downloads?page=2"}"#;
        let page: BitbucketDownloads = serde_json::from_str(raw).unwrap();
        assert_eq!(page.values.len(), 1);
        assert_eq!(page.values[0].size, Some(2048));
        assert!(page.next.is_some());
    }

    #[test]
    fn declared_artifacts_support_private_signed_urls_without_network() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(
            repo.path().join("release-desk-store.json"),
            r#"{"name":"Example","releases":[{"tag":"v1.4.0","notes":"Better export","assets":[{"name":"example-v1.4.0-macos.dmg","url":"https://bucket.s3.amazonaws.com/example.dmg?X-Amz-Signature=abc","size":1234}]}]}"#,
        ).unwrap();
        let releases = load_declared_releases(repo.path()).unwrap().unwrap();
        assert_eq!(releases[0].tag_name, "v1.4.0");
        assert_eq!(releases[0].assets[0].size, Some(1234));
        assert!(
            releases[0].assets[0]
                .download_url
                .contains("X-Amz-Signature")
        );
    }
}
