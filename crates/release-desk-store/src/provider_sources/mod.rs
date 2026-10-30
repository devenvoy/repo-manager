//! Provider-specific source discovery behind one desktop-facing contract.

mod azure;
mod bitbucket;
mod codecommit;
pub mod connection;
mod forge;
mod github;
mod gitlab;

use crate::source_fetcher::FetchedProfile;
use release_desk_core::error::DomainError;

/// A selected host and optional instance coordinate for source discovery.
pub struct SourceRequest<'a> {
    pub provider: &'a str,
    pub input: &'a str,
    pub credential: Option<&'a str>,
    pub endpoint: Option<&'a str>,
}

/// One adapter owns parsing, account lookup, and full repository pagination.
pub trait SourceAdapter {
    fn discover(
        &self,
        request: &SourceRequest<'_>,
    ) -> Result<(FetchedProfile, Option<String>), DomainError>;
}

static GITHUB: github::GitHubSource = github::GitHubSource;
static GITLAB: gitlab::GitLabSource = gitlab::GitLabSource;
static BITBUCKET: bitbucket::BitbucketSource = bitbucket::BitbucketSource;
static FORGE: forge::ForgeSource = forge::ForgeSource;
static AZURE: azure::AzureSource = azure::AzureSource;
static CODECOMMIT: codecommit::CodeCommitSource = codecommit::CodeCommitSource;

fn adapter_for(provider: &str) -> Option<&'static dyn SourceAdapter> {
    match provider {
        "github" => Some(&GITHUB),
        "gitlab" => Some(&GITLAB),
        "bitbucket" => Some(&BITBUCKET),
        "gitea" | "forgejo" | "codeberg" | "custom" => Some(&FORGE),
        "azure_devops" => Some(&AZURE),
        "aws_codecommit" => Some(&CODECOMMIT),
        _ => None,
    }
}

/// Capability used by project setup; backed by an installed discovery adapter.
pub fn supports(provider: &str) -> bool {
    adapter_for(provider).is_some()
}

/// Identify the provider and source key before performing network I/O.
pub fn identify(
    request: &SourceRequest<'_>,
) -> Result<(String, String, Option<String>), DomainError> {
    if request.provider == "aws_codecommit" {
        return Ok((request.provider.into(), "all".into(), None));
    }
    if request.input.starts_with("https://") || request.input.starts_with("http://") {
        let url = safe_origin(request.input)?;
        let host = url
            .host_str()
            .ok_or_else(|| failure("Repository URL has no host"))?;
        let provider = match host {
            "github.com" => "github",
            "gitlab.com" => "gitlab",
            "bitbucket.org" => "bitbucket",
            "gitea.com" => "gitea",
            "codeberg.org" => "codeberg",
            "dev.azure.com" => "azure_devops",
            _ => {
                let configured = request
                    .endpoint
                    .ok_or_else(|| failure("Unknown provider host"))?;
                if safe_origin(configured)?.host_str() != Some(host) {
                    return Err(failure("Repository URL belongs to another provider host"));
                }
                request.provider
            }
        };
        let path = url.path().trim_matches('/');
        let parts = safe_path(path)?;
        let (owner, repo) = if provider == "azure_devops" {
            match parts.as_slice() {
                [org, project, "_git", repo] => {
                    (format!("{org}/{project}"), Some((*repo).to_string()))
                }
                [org, project] => (format!("{org}/{project}"), None),
                _ => return Err(failure("Enter an Azure project or repository URL")),
            }
        } else if provider == "gitlab" {
            (
                parts[0].to_string(),
                parts
                    .last()
                    .filter(|_| parts.len() > 1)
                    .map(|value| (*value).to_string()),
            )
        } else {
            (
                parts[0].to_string(),
                parts.get(1).map(|value| (*value).to_string()),
            )
        };
        Ok((
            provider.to_string(),
            owner,
            repo.map(|value| value.trim_end_matches(".git").to_string()),
        ))
    } else {
        let input = request.input.trim_start_matches('@').trim_matches('/');
        let parts = safe_path(input)?;
        let owner = parts[0].to_string();
        let repo = parts
            .get(1)
            .map(|value| value.trim_end_matches(".git").to_string());
        Ok((request.provider.to_string(), owner, repo))
    }
}

/// Resolve a configured provider to its discovery adapter.
pub fn discover(
    request: &SourceRequest<'_>,
) -> Result<(FetchedProfile, Option<String>), DomainError> {
    adapter_for(request.provider)
        .ok_or_else(|| failure(format!("Unknown Git provider: {}", request.provider)))?
        .discover(request)
}

pub(super) fn failure(message: impl Into<String>) -> DomainError {
    DomainError::PortFailure {
        port: "source_discovery",
        message: message.into(),
    }
}

pub(super) fn api_client() -> Result<reqwest::blocking::Client, DomainError> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Release-Desk-App/1.0")
        .build()
        .map_err(|error| failure(format!("Could not initialize API client: {error}")))
}

pub(super) fn safe_origin(raw: &str) -> Result<reqwest::Url, DomainError> {
    let url =
        reqwest::Url::parse(raw).map_err(|_| failure("Enter a valid HTTPS provider address"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(failure(
            "Provider address must be an HTTPS URL without credentials",
        ));
    }
    Ok(url)
}

pub(super) fn safe_path(input: &str) -> Result<Vec<&str>, DomainError> {
    let parts: Vec<_> = input.trim_matches('/').split('/').collect();
    if parts.is_empty()
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.contains(['?', '#', '%', '\\', ' ', '@'])
                || part.chars().any(char::is_control)
        })
    {
        return Err(failure("Enter a valid account or repository path"));
    }
    Ok(parts)
}

pub(super) fn input_path(input: &str, origin: &reqwest::Url) -> Result<String, DomainError> {
    if input.starts_with("https://") || input.starts_with("http://") {
        let url = safe_origin(input)?;
        if url.origin() != origin.origin() {
            return Err(failure(
                "Repository URL belongs to a different provider host",
            ));
        }
        Ok(url.path().trim_start_matches('/').to_string())
    } else {
        Ok(input.trim_start_matches('@').to_string())
    }
}

pub(super) fn request_json<T: serde::de::DeserializeOwned>(
    request: reqwest::blocking::RequestBuilder,
) -> Result<T, DomainError> {
    let response = request.send().map_err(|error| failure(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(failure(format!("Provider API returned HTTP {status}")));
    }
    response
        .json()
        .map_err(|error| failure(format!("Invalid provider response: {error}")))
}

pub(super) fn checked_next(
    current: &reqwest::Url,
    next: &str,
) -> Result<reqwest::Url, DomainError> {
    let url = current
        .join(next)
        .map_err(|_| failure("Invalid provider pagination URL"))?;
    if url.scheme() != "https" || url.origin() != current.origin() {
        return Err(failure("Provider pagination moved to an untrusted host"));
    }
    Ok(url)
}

pub(super) fn clean_clone_url(
    raw: &str,
    expected_origin: &reqwest::Url,
    strip_username: bool,
) -> Result<String, DomainError> {
    let mut url =
        reqwest::Url::parse(raw).map_err(|_| failure("Provider returned an invalid clone URL"))?;
    if url.scheme() != "https"
        || url.origin() != expected_origin.origin()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.password().is_some()
        || (!strip_username && !url.username().is_empty())
    {
        return Err(failure("Provider returned an untrusted HTTPS clone URL"));
    }
    safe_path(url.path())?;
    if strip_username {
        url.set_username("")
            .map_err(|_| failure("Invalid clone URL username"))?;
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_provider_urls_without_trusting_similar_hosts() {
        for provider in [
            "github",
            "gitlab",
            "bitbucket",
            "gitea",
            "forgejo",
            "codeberg",
            "azure_devops",
            "aws_codecommit",
            "custom",
        ] {
            assert!(supports(provider), "missing adapter for {provider}");
        }
        for (input, provider, owner, repo) in [
            (
                "https://bitbucket.org/team/app.git",
                "bitbucket",
                "team",
                Some("app"),
            ),
            (
                "https://codeberg.org/team/app",
                "codeberg",
                "team",
                Some("app"),
            ),
            ("https://gitea.com/team", "gitea", "team", None),
            (
                "https://dev.azure.com/org/project/_git/app",
                "azure_devops",
                "org/project",
                Some("app"),
            ),
        ] {
            let request = SourceRequest {
                provider: "github",
                input,
                credential: None,
                endpoint: None,
            };
            let actual = identify(&request).expect("known provider URL");
            assert_eq!(
                actual,
                (provider.into(), owner.into(), repo.map(str::to_string))
            );
        }
        let lookalike = SourceRequest {
            provider: "github",
            input: "https://github.com.attacker.test/team/app",
            credential: None,
            endpoint: None,
        };
        assert!(identify(&lookalike).is_err());
    }

    #[test]
    fn rejects_cross_origin_pagination_and_unsafe_paths() {
        let current = reqwest::Url::parse("https://api.bitbucket.org/2.0/repositories/team?page=1")
            .expect("fixed URL");
        assert!(checked_next(&current, "?page=2").is_ok());
        assert!(checked_next(&current, "https://evil.test/next").is_err());
        assert!(safe_path("team/../repo").is_err());
        assert!(safe_path("team/%2Frepo").is_err());
        assert!(safe_origin("https://user:secret@git.example.com").is_err());
        let bitbucket = reqwest::Url::parse("https://bitbucket.org/").expect("fixed URL");
        assert_eq!(
            clean_clone_url(
                "https://person@bitbucket.org/team/app.git",
                &bitbucket,
                true
            )
            .expect("clean clone URL"),
            "https://bitbucket.org/team/app.git"
        );
    }
}
