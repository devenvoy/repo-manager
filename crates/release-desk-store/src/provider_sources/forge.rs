use super::{
    SourceAdapter, SourceRequest, api_client, clean_clone_url, failure, input_path, safe_origin,
    safe_path,
};
use crate::source_fetcher::{FetchedProfile, FetchedRepo};
use release_desk_core::error::DomainError;
use serde::Deserialize;

pub(super) struct ForgeSource;

#[derive(Deserialize)]
struct ForgeIdentity {
    login: Option<String>,
    username: Option<String>,
    full_name: Option<String>,
    avatar_url: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct ForgeRepo {
    name: String,
    full_name: String,
    description: Option<String>,
    #[serde(default)]
    stars_count: u32,
    #[serde(default)]
    forks_count: u32,
    language: Option<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    fork: bool,
    clone_url: String,
    default_branch: Option<String>,
}

fn authorize(
    request: reqwest::blocking::RequestBuilder,
    credential: Option<&str>,
) -> reqwest::blocking::RequestBuilder {
    if let Some(token) = credential.filter(|token| !token.is_empty()) {
        request.header("Authorization", format!("token {token}"))
    } else {
        request
    }
}

impl SourceAdapter for ForgeSource {
    fn discover(
        &self,
        request: &SourceRequest<'_>,
    ) -> Result<(FetchedProfile, Option<String>), DomainError> {
        let base = match request.provider {
            "codeberg" => "https://codeberg.org/",
            "gitea" => "https://gitea.com/",
            "forgejo" | "custom" => request.endpoint.ok_or_else(|| {
                failure("Enter the HTTPS address of the Forgejo or custom Git server")
            })?,
            _ => return Err(failure("Unsupported forge provider")),
        };
        let origin = safe_origin(base)?;
        if origin.path() != "/" {
            return Err(failure("Forge address must be its HTTPS origin"));
        }
        let path = input_path(request.input, &origin)?;
        let parts = safe_path(&path)?;
        if parts.len() > 2 {
            return Err(failure("Use a forge user, organization, or repository URL"));
        }
        let account = parts[0];
        let selected = parts
            .get(1)
            .map(|value| value.trim_end_matches(".git").to_string());
        let client = api_client()?;
        let api_root = format!("{}api/v1", origin.as_str());
        let org_url = format!("{api_root}/orgs/{account}");
        let org_response = authorize(client.get(org_url), request.credential)
            .send()
            .map_err(|error| failure(error.to_string()))?;
        let (identity, list_path) = if org_response.status().is_success() {
            let identity = org_response
                .json::<ForgeIdentity>()
                .map_err(|error| failure(format!("Invalid forge organization: {error}")))?;
            (identity, format!("orgs/{account}/repos"))
        } else if org_response.status() == reqwest::StatusCode::NOT_FOUND {
            let user_url = format!("{api_root}/users/{account}");
            let user_response = authorize(client.get(user_url), request.credential)
                .send()
                .map_err(|error| failure(error.to_string()))?;
            if !user_response.status().is_success() {
                return Err(failure(format!(
                    "Forge user lookup returned HTTP {}",
                    user_response.status()
                )));
            }
            let identity = user_response
                .json::<ForgeIdentity>()
                .map_err(|error| failure(format!("Invalid forge user: {error}")))?;
            (identity, format!("users/{account}/repos"))
        } else {
            return Err(failure(format!(
                "Forge organization lookup returned HTTP {}",
                org_response.status()
            )));
        };

        let mut repositories = Vec::new();
        let mut finished = false;
        for page in 1..=1000 {
            let url = format!("{api_root}/{list_path}?limit=50&page={page}");
            let response = authorize(client.get(url), request.credential)
                .send()
                .map_err(|error| failure(error.to_string()))?;
            if !response.status().is_success() {
                return Err(failure(format!(
                    "Forge repository listing returned HTTP {}",
                    response.status()
                )));
            }
            let batch = response
                .json::<Vec<ForgeRepo>>()
                .map_err(|error| failure(format!("Invalid forge repositories: {error}")))?;
            if batch.is_empty() {
                finished = true;
                break;
            }
            for repo in batch {
                repositories.push(FetchedRepo {
                    name: repo.name,
                    full_name: repo.full_name,
                    description: repo.description,
                    stars: repo.stars_count,
                    forks: repo.forks_count,
                    primary_language: repo.language,
                    is_private: repo.private,
                    is_fork: repo.fork,
                    clone_url: clean_clone_url(&repo.clone_url, &origin, false)?,
                    default_branch: repo.default_branch.unwrap_or_default(),
                });
            }
        }
        if !finished {
            return Err(failure("Forge repository listing exceeded the page limit"));
        }
        if let Some(name) = &selected {
            if !repositories
                .iter()
                .any(|repo| repo.name.eq_ignore_ascii_case(name))
            {
                return Err(failure(format!(
                    "Repository {account}/{name} was not found"
                )));
            }
        }
        Ok((
            FetchedProfile {
                handle: identity
                    .login
                    .or(identity.username)
                    .unwrap_or_else(|| account.to_string()),
                display_name: identity.full_name,
                avatar_url: identity.avatar_url,
                bio: identity.description,
                provider: request.provider.to_string(),
                repositories,
            },
            selected,
        ))
    }
}
