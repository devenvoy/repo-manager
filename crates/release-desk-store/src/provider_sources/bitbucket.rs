use super::{
    SourceAdapter, SourceRequest, api_client, checked_next, clean_clone_url, failure, input_path,
    request_json, safe_path,
};
use crate::source_fetcher::{FetchedProfile, FetchedRepo};
use release_desk_core::error::DomainError;
use serde::Deserialize;

pub(super) struct BitbucketSource;

#[derive(Deserialize)]
struct Workspace {
    slug: String,
    name: Option<String>,
    links: Option<WorkspaceLinks>,
}

#[derive(Deserialize)]
struct WorkspaceLinks {
    avatar: Option<Href>,
}

#[derive(Deserialize)]
struct Href {
    href: String,
}

#[derive(Deserialize)]
struct RepoPage {
    values: Vec<Repository>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct Repository {
    slug: String,
    full_name: String,
    description: Option<String>,
    is_private: bool,
    language: Option<String>,
    mainbranch: Option<Branch>,
    links: RepoLinks,
    parent: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Branch {
    name: String,
}

#[derive(Deserialize)]
struct RepoLinks {
    clone: Vec<CloneLink>,
}

#[derive(Deserialize)]
struct CloneLink {
    href: String,
    name: String,
}

impl SourceAdapter for BitbucketSource {
    fn discover(
        &self,
        request: &SourceRequest<'_>,
    ) -> Result<(FetchedProfile, Option<String>), DomainError> {
        let origin = reqwest::Url::parse("https://bitbucket.org/").expect("fixed Bitbucket URL");
        let path = input_path(request.input, &origin)?;
        let parts = safe_path(&path)?;
        if parts.len() > 2 {
            return Err(failure(
                "Use a Bitbucket workspace or workspace/repository URL",
            ));
        }
        let workspace = parts[0];
        let selected = parts
            .get(1)
            .map(|value| value.trim_end_matches(".git").to_string());
        let client = api_client()?;
        let workspace_url = format!("https://api.bitbucket.org/2.0/workspaces/{workspace}");
        let mut profile_request = client.get(workspace_url);
        if let Some(token) = request.credential {
            profile_request = profile_request.bearer_auth(token);
        }
        let profile: Workspace = request_json(profile_request)?;

        let mut next = Some(
            reqwest::Url::parse(&format!(
                "https://api.bitbucket.org/2.0/repositories/{workspace}?pagelen=100"
            ))
            .map_err(|_| failure("Invalid Bitbucket workspace"))?,
        );
        let mut repositories = Vec::new();
        for _ in 0..1000 {
            let Some(url) = next.take() else { break };
            let mut call = client.get(url.clone());
            if let Some(token) = request.credential {
                call = call.bearer_auth(token);
            }
            let page: RepoPage = request_json(call)?;
            for repo in page.values {
                let clone_url = repo
                    .links
                    .clone
                    .into_iter()
                    .find(|link| link.name == "https")
                    .map(|link| link.href)
                    .ok_or_else(|| failure("Bitbucket repository is missing an HTTPS clone URL"))?;
                repositories.push(FetchedRepo {
                    name: repo.slug,
                    full_name: repo.full_name,
                    description: repo.description,
                    stars: 0,
                    forks: 0,
                    primary_language: repo.language,
                    is_private: repo.is_private,
                    is_fork: repo.parent.is_some(),
                    clone_url: clean_clone_url(&clone_url, &origin, true)?,
                    default_branch: repo
                        .mainbranch
                        .map_or_else(String::new, |branch| branch.name),
                });
            }
            next = page
                .next
                .as_deref()
                .map(|value| checked_next(&url, value))
                .transpose()?;
        }
        if next.is_some() {
            return Err(failure(
                "Bitbucket repository listing exceeded the page limit",
            ));
        }
        if let Some(name) = &selected {
            if !repositories
                .iter()
                .any(|repo| repo.name.eq_ignore_ascii_case(name))
            {
                return Err(failure(format!(
                    "Bitbucket repository {workspace}/{name} was not found"
                )));
            }
        }
        Ok((
            FetchedProfile {
                handle: profile.slug,
                display_name: profile.name,
                avatar_url: profile
                    .links
                    .and_then(|links| links.avatar)
                    .map(|avatar| avatar.href),
                bio: None,
                provider: "bitbucket".into(),
                repositories,
            },
            selected,
        ))
    }
}
