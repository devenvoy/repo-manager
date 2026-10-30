//! Profile ingestion use case for fetching and selecting repositories to clone into a project.
//!
//! Extracts usernames/orgs from raw handles or provider profile URLs (e.g. `https://github.com/storytold`),
//! builds selectable repository lists, and calculates target on-disk paths based on the project's
//! `DiskLayout` preference (`Grouped` vs `Flat`).

use release_desk_core::error::{DomainError, ValidationError};
use release_desk_core::project::{
    AccountKind, AccountLogin, ProfileSource, ProjectIdentity, SelectableRepo,
};
use release_desk_core::provider::GitProviderKind;
use std::path::PathBuf;

/// Parsed profile reference identifying a provider and account handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedProfileRef {
    pub provider: GitProviderKind,
    pub login: AccountLogin,
}

/// Helper to parse a raw string which could be a full URL or a bare username.
pub fn parse_profile_input(
    raw: &str,
    default_provider: GitProviderKind,
) -> Result<ParsedProfileRef, ValidationError> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(ValidationError::Empty("profile input"));
    }

    // Check if it's a URL
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let without_proto = trimmed
            .trim_start_matches("https://")
            .trim_start_matches("http://");

        let mut parts = without_proto.split('/');
        let host = parts.next().unwrap_or("").to_lowercase();
        let handle = parts.next().unwrap_or("");

        if handle.is_empty() {
            return Err(ValidationError::InvalidPath(
                "profile URL must contain a username or organization path",
            ));
        }

        let provider = if host.contains("github.com") {
            GitProviderKind::GitHub
        } else if host.contains("gitlab.com") {
            GitProviderKind::GitLab
        } else if host.contains("bitbucket.org") {
            GitProviderKind::Bitbucket
        } else {
            GitProviderKind::Custom(host)
        };

        let login = AccountLogin::new(handle)?;
        Ok(ParsedProfileRef { provider, login })
    } else {
        // Bare username, with optional leading '@'
        let clean = trimmed.trim_start_matches('@');
        let login = AccountLogin::new(clean)?;
        Ok(ParsedProfileRef {
            provider: default_provider,
            login,
        })
    }
}

/// Raw discovered repository information prior to selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredRepoItem {
    pub name: String,
    pub clone_url: String,
    pub description: Option<String>,
    pub is_private: bool,
    pub stars: u32,
    pub default_branch: String,
    pub has_release_asset: bool,
}

/// Plan for cloning an individual selected repository into the project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepoClonePlan {
    pub repo_name: String,
    pub clone_url: String,
    pub target_path: PathBuf,
}

/// Use case orchestrating profile ingestion and repository selection.
#[derive(Clone, Debug, Default)]
pub struct IngestProfileUseCase;

impl IngestProfileUseCase {
    pub fn new() -> Self {
        Self
    }

    /// Parse user input into a validated profile reference.
    pub fn parse_target(
        &self,
        raw: &str,
        default_provider: GitProviderKind,
    ) -> Result<ParsedProfileRef, DomainError> {
        parse_profile_input(raw, default_provider).map_err(DomainError::from)
    }

    /// Transform repository listings into selectable items with defaults.
    pub fn prepare_selectable_repos(&self, items: Vec<DiscoveredRepoItem>) -> Vec<SelectableRepo> {
        items
            .into_iter()
            .map(|item| SelectableRepo {
                name: item.name,
                clone_url: item.clone_url,
                description: item.description,
                is_private: item.is_private,
                stars: item.stars,
                default_branch: item.default_branch,
                has_release_asset: item.has_release_asset,
                selected: true, // Default to selected
            })
            .collect()
    }

    /// Compute clone target paths for selected repositories according to project layout.
    pub fn plan_clones(
        &self,
        project: &ProjectIdentity,
        user_login: &str,
        repos: &[SelectableRepo],
    ) -> Result<Vec<RepoClonePlan>, DomainError> {
        let root = project
            .clone_location
            .as_deref()
            .ok_or(DomainError::Validation(ValidationError::InvalidPath(
                "project has no bound clone location",
            )))?;

        let plans = repos
            .iter()
            .filter(|r| r.selected)
            .map(|r| {
                let target_path = project
                    .disk_layout
                    .compute_clone_path(root, user_login, &r.name);
                RepoClonePlan {
                    repo_name: r.name.clone(),
                    clone_url: r.clone_url.clone(),
                    target_path,
                }
            })
            .collect();

        Ok(plans)
    }

    /// Create a profile source object from fetched metadata.
    pub fn create_profile_source(
        &self,
        login: AccountLogin,
        kind: AccountKind,
        display_name: Option<String>,
        avatar_url: Option<String>,
        bio: Option<String>,
        public_repos_count: u32,
    ) -> ProfileSource {
        ProfileSource {
            identity: release_desk_core::project::AccountIdentity { login, kind },
            display_name,
            avatar_url,
            bio,
            public_repos_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use release_desk_core::project::{DiskLayout, ProjectId, ProjectName};

    #[test]
    fn test_parse_profile_urls_and_handles() {
        let gh =
            parse_profile_input("https://github.com/storytold", GitProviderKind::GitHub).unwrap();
        assert_eq!(gh.provider, GitProviderKind::GitHub);
        assert_eq!(gh.login.as_str(), "storytold");

        let gh_slash =
            parse_profile_input("https://github.com/storytold/", GitProviderKind::GitHub).unwrap();
        assert_eq!(gh_slash.login.as_str(), "storytold");

        let gl =
            parse_profile_input("https://gitlab.com/cloud-ops", GitProviderKind::GitHub).unwrap();
        assert_eq!(gl.provider, GitProviderKind::GitLab);
        assert_eq!(gl.login.as_str(), "cloud-ops");

        let handle = parse_profile_input("@example-user", GitProviderKind::GitHub).unwrap();
        assert_eq!(handle.provider, GitProviderKind::GitHub);
        assert_eq!(handle.login.as_str(), "example-user");

        assert!(parse_profile_input("", GitProviderKind::GitHub).is_err());
        assert!(parse_profile_input("https://github.com/", GitProviderKind::GitHub).is_err());
    }

    #[test]
    fn test_clone_plans_grouped_and_flat() {
        let use_case = IngestProfileUseCase::new();
        let id = ProjectId::from_u128(100);
        let name = ProjectName::new("Fleet").unwrap();
        let root = PathBuf::from("/Users/example/Codes");

        let grouped_proj =
            ProjectIdentity::with_clone_location(id, name.clone(), Some(root.clone()))
                .with_layout(DiskLayout::Grouped);

        let repos = vec![
            SelectableRepo {
                name: "photocraft".to_string(),
                clone_url: "https://github.com/storytold/photocraft.git".to_string(),
                description: Some("RAW Editor".to_string()),
                is_private: false,
                stars: 120,
                default_branch: "main".to_string(),
                has_release_asset: true,
                selected: true,
            },
            SelectableRepo {
                name: "gridcraft".to_string(),
                clone_url: "https://github.com/storytold/gridcraft.git".to_string(),
                description: Some("Spreadsheet".to_string()),
                is_private: false,
                stars: 45,
                default_branch: "main".to_string(),
                has_release_asset: true,
                selected: false, // Unselected
            },
        ];

        let plans = use_case
            .plan_clones(&grouped_proj, "storytold", &repos)
            .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(
            plans[0].target_path,
            PathBuf::from("/Users/example/Codes/storytold/photocraft")
        );

        let flat_proj = ProjectIdentity::with_clone_location(id, name, Some(root))
            .with_layout(DiskLayout::Flat);
        let flat_plans = use_case
            .plan_clones(&flat_proj, "storytold", &repos)
            .unwrap();
        assert_eq!(flat_plans.len(), 1);
        assert_eq!(
            flat_plans[0].target_path,
            PathBuf::from("/Users/example/Codes/photocraft")
        );
    }
}
