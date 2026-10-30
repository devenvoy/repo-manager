use super::{SourceAdapter, SourceRequest, clean_clone_url, failure, safe_path};
use crate::source_fetcher::{FetchedProfile, FetchedRepo};
use release_desk_core::error::DomainError;
use serde::Deserialize;
use std::process::Command;

pub(super) struct CodeCommitSource;

#[derive(Deserialize)]
struct RepoList {
    repositories: Vec<RepoName>,
    #[serde(rename = "nextToken")]
    next_token: Option<String>,
}

#[derive(Deserialize)]
struct RepoName {
    #[serde(rename = "repositoryName")]
    name: String,
}

#[derive(Deserialize)]
struct RepoDetail {
    #[serde(rename = "repositoryMetadata")]
    metadata: RepoMetadata,
}

#[derive(Deserialize)]
struct RepoMetadata {
    #[serde(rename = "repositoryName")]
    name: String,
    #[serde(rename = "cloneUrlHttp")]
    clone_url: String,
    #[serde(rename = "defaultBranch")]
    default_branch: Option<String>,
    description: Option<String>,
}

fn aws_json<T: serde::de::DeserializeOwned>(args: &[&str], region: &str) -> Result<T, DomainError> {
    let output = Command::new("aws")
        .args(["codecommit"])
        .args(args)
        .args(["--region", region, "--output", "json", "--no-cli-pager"])
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .output()
        .map_err(|_| failure("AWS CLI is required to discover CodeCommit repositories"))?;
    if !output.status.success() {
        return Err(failure(
            "AWS CodeCommit request failed. Check the selected AWS profile, region, and IAM access",
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| failure(format!("Invalid AWS CodeCommit response: {error}")))
}

impl SourceAdapter for CodeCommitSource {
    fn discover(
        &self,
        request: &SourceRequest<'_>,
    ) -> Result<(FetchedProfile, Option<String>), DomainError> {
        let region = request
            .endpoint
            .ok_or_else(|| failure("Enter an AWS region"))?;
        if region.is_empty()
            || !region
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(failure("Enter a valid AWS region, such as us-east-1"));
        }
        let selected = match request.input.trim() {
            "" | "all" | "*" => None,
            value => {
                let parts = safe_path(value)?;
                if parts.len() != 1 {
                    return Err(failure("Enter a CodeCommit repository name or all"));
                }
                Some(parts[0].to_string())
            }
        };
        let mut names = Vec::new();
        let mut next_token = None;
        let mut seen_tokens = std::collections::HashSet::new();
        let mut finished = false;
        for _ in 0..1000 {
            let listing: RepoList = if let Some(token) = next_token.as_deref() {
                aws_json(&["list-repositories", "--next-token", token], region)?
            } else {
                aws_json(&["list-repositories"], region)?
            };
            names.extend(listing.repositories);
            next_token = listing.next_token;
            match next_token.as_deref() {
                Some(token) if !seen_tokens.insert(token.to_string()) => {
                    return Err(failure("CodeCommit pagination returned a repeated token"));
                }
                Some(_) => {}
                None => {
                    finished = true;
                    break;
                }
            }
        }
        if !finished {
            return Err(failure(
                "CodeCommit repository listing exceeded the page limit",
            ));
        }
        let mut repositories = Vec::with_capacity(names.len());
        for item in names {
            let detail: RepoDetail =
                aws_json(&["get-repository", "--repository-name", &item.name], region)?;
            let repo = detail.metadata;
            repositories.push(FetchedRepo {
                full_name: format!("{region}/{}", repo.name),
                name: repo.name,
                description: repo.description,
                stars: 0,
                forks: 0,
                primary_language: None,
                is_private: true,
                is_fork: false,
                clone_url: clean_clone_url(
                    &repo.clone_url,
                    &reqwest::Url::parse(&format!(
                        "https://git-codecommit.{region}.amazonaws.com/"
                    ))
                    .map_err(|_| failure("Invalid CodeCommit region"))?,
                    false,
                )?,
                default_branch: repo.default_branch.unwrap_or_default(),
            });
        }
        if let Some(name) = &selected {
            if !repositories.iter().any(|repo| repo.name == *name) {
                return Err(failure(format!(
                    "CodeCommit repository {name} was not found in {region}"
                )));
            }
        }
        Ok((
            FetchedProfile {
                handle: "all".into(),
                display_name: Some(format!("AWS CodeCommit ({region})")),
                avatar_url: None,
                bio: None,
                provider: "aws_codecommit".into(),
                repositories,
            },
            selected,
        ))
    }
}
