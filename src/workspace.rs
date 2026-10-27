//! Desktop project records and provider choices used by the workspace UI.

use crate::workspace_sources::{RepositoryListMode, WorkspaceSource};
use release_desk_core::project::{DiskLayout, ProjectId, ProjectIdentity, ProjectName};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderChoice {
    #[default]
    GitHub,
    GitLab,
    Bitbucket,
    AzureDevOps,
    Gitea,
    Forgejo,
    Codeberg,
    AwsCodeCommit,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProjectSection {
    #[default]
    Overview,
    Sources,
    Repositories,
    Releases,
    Settings,
}

impl ProjectSection {
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Sources,
        Self::Repositories,
        Self::Releases,
        Self::Settings,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Sources => "Sources",
            Self::Repositories => "Repositories",
            Self::Releases => "Releases",
            Self::Settings => "Settings",
        }
    }
}

impl ProviderChoice {
    pub const ALL: [Self; 9] = [
        Self::GitHub,
        Self::GitLab,
        Self::Bitbucket,
        Self::AzureDevOps,
        Self::Gitea,
        Self::Forgejo,
        Self::Codeberg,
        Self::AwsCodeCommit,
        Self::Custom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
            Self::Bitbucket => "Bitbucket",
            Self::AzureDevOps => "Azure DevOps",
            Self::Gitea => "Gitea",
            Self::Forgejo => "Forgejo",
            Self::Codeberg => "Codeberg",
            Self::AwsCodeCommit => "AWS CodeCommit",
            Self::Custom => "Custom Git server",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
            Self::Bitbucket => "bitbucket",
            Self::AzureDevOps => "azure_devops",
            Self::Gitea => "gitea",
            Self::Forgejo => "forgejo",
            Self::Codeberg => "codeberg",
            Self::AwsCodeCommit => "aws_codecommit",
            Self::Custom => "custom",
        }
    }

    pub const fn default_host(self) -> &'static str {
        match self {
            Self::GitHub => "github.com",
            Self::GitLab => "gitlab.com",
            Self::Bitbucket => "bitbucket.org",
            Self::AzureDevOps => "dev.azure.com",
            Self::Gitea => "gitea.com",
            Self::Forgejo => "codeberg.org",
            Self::Codeberg => "codeberg.org",
            Self::AwsCodeCommit => "amazonaws.com",
            Self::Custom => "localhost",
        }
    }

    pub const fn token_creation_url(self) -> &'static str {
        match self {
            Self::GitLab => {
                "https://gitlab.com/-/profile/personal_access_tokens?name=Release%20Desk&scopes=api,read_user,read_repository"
            }
            Self::Bitbucket => "https://bitbucket.org/account/settings/app-passwords/",
            _ => {
                "https://github.com/settings/tokens/new?description=Release%20Desk&scopes=repo,read:user"
            }
        }
    }

    pub fn oauth_authorize_url(self, client_id: &str, redirect_uri: &str) -> String {
        let cid = if client_id.trim().is_empty() {
            "release-desk-app"
        } else {
            client_id.trim()
        };
        match self {
            Self::GitHub => format!(
                "https://github.com/login/oauth/authorize?client_id={cid}&redirect_uri={redirect_uri}&scope=repo%20read:user&response_type=code"
            ),
            Self::GitLab => format!(
                "https://gitlab.com/oauth/authorize?client_id={cid}&redirect_uri={redirect_uri}&response_type=code&scope=read_user%20read_repository"
            ),
            Self::Bitbucket => format!(
                "https://bitbucket.org/site/oauth2/authorize?client_id={cid}&response_type=code"
            ),
            Self::Codeberg | Self::Forgejo => format!(
                "https://codeberg.org/login/oauth/authorize?client_id={cid}&redirect_uri={redirect_uri}&response_type=code"
            ),
            Self::Gitea => format!(
                "https://gitea.com/login/oauth/authorize?client_id={cid}&redirect_uri={redirect_uri}&response_type=code"
            ),
            _ => format!(
                "https://{}/login/oauth/authorize?client_id={cid}&redirect_uri={redirect_uri}&response_type=code",
                self.default_host()
            ),
        }
    }
}

impl From<ProviderChoice> for release_desk_core::provider::GitProviderKind {
    fn from(choice: ProviderChoice) -> Self {
        match choice {
            ProviderChoice::GitHub => release_desk_core::provider::GitProviderKind::GitHub,
            ProviderChoice::GitLab => release_desk_core::provider::GitProviderKind::GitLab,
            ProviderChoice::Bitbucket => release_desk_core::provider::GitProviderKind::Bitbucket,
            other => {
                release_desk_core::provider::GitProviderKind::Custom(other.as_str().to_string())
            }
        }
    }
}

/// Account credentials and connection info previously saved for a provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedAccountInfo {
    pub handle: String,
    pub credential_ref: Option<String>,
    pub has_keychain_secret: bool,
}

/// Selected authentication workflow mode in Project Setup.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AuthModeChoice {
    #[default]
    OAuthBrowser,
    OAuthWebview,
    Token,
    SavedAccount,
}

/// Find unique saved accounts for a given provider across projects.
pub fn find_saved_accounts_for_provider(
    projects: &[WorkspaceProject],
    provider: ProviderChoice,
) -> Vec<SavedAccountInfo> {
    let mut accounts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in projects {
        if p.provider == provider && !p.account.trim().is_empty() {
            let handle = p.account.trim().to_string();
            if seen.insert(handle.clone()) {
                let cred_key = p
                    .credential_ref
                    .clone()
                    .unwrap_or_else(|| format!("release_desk_{}_{}", provider.as_str(), handle));
                accounts.push(SavedAccountInfo {
                    handle,
                    credential_ref: Some(cred_key),
                    has_keychain_secret: true,
                });
            }
        }
    }
    accounts
}

/// One project always maps to one on-disk directory and one provider account.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceProject {
    pub id: u128,
    pub name: String,
    pub directory: PathBuf,
    pub provider: ProviderChoice,
    pub account: String,
    pub custom_host: Option<String>,
    #[serde(default)]
    pub credential_ref: Option<String>,
    pub disk_layout: DiskLayout,
    #[serde(default)]
    pub sources: Vec<WorkspaceSource>,
    #[serde(default)]
    pub repository_list_mode: RepositoryListMode,
}

impl WorkspaceProject {
    pub fn validate(&self) -> Result<(), String> {
        ProjectName::new(&self.name).map_err(|error| error.to_string())?;
        if !self.directory.is_absolute() || !self.directory.is_dir() {
            return Err("Choose an existing directory on disk for this project.".into());
        }
        if self.provider == ProviderChoice::Custom {
            let host = self.custom_host.as_deref().unwrap_or_default();
            if !valid_custom_host(host) {
                return Err("Enter a valid HTTPS URL for the custom Git server.".into());
            }
        }
        Ok(())
    }

    pub fn identity(&self) -> ProjectIdentity {
        ProjectIdentity::with_clone_location(
            ProjectId::from_u128(self.id),
            ProjectName::new(&self.name).expect("validated project name"),
            Some(self.directory.clone()),
        )
        .with_layout(self.disk_layout)
    }
}

pub fn expand_directory(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(trimmed)
}

fn valid_custom_host(raw: &str) -> bool {
    let Some(host) = raw.strip_prefix("https://") else {
        return false;
    };
    let authority = host.split('/').next().unwrap_or_default();
    !authority.is_empty()
        && !authority.contains('@')
        && !authority.contains(' ')
        && authority.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_host_is_required_only_for_custom_provider() {
        let directory = std::env::temp_dir();
        let mut project = WorkspaceProject {
            id: 1,
            name: "Demo".into(),
            directory,
            provider: ProviderChoice::GitHub,
            account: "demo".into(),
            custom_host: None,
            credential_ref: None,
            disk_layout: DiskLayout::Grouped,
            sources: Vec::new(),
            repository_list_mode: RepositoryListMode::Grouped,
        };
        assert!(project.validate().is_ok());
        project.provider = ProviderChoice::Custom;
        assert!(project.validate().is_err());
        project.custom_host = Some("https://git.example.com".into());
        assert!(project.validate().is_ok());
    }

    #[test]
    fn account_is_optional_on_project() {
        let directory = std::env::temp_dir();
        let project = WorkspaceProject {
            id: 1,
            name: "Fleet".into(),
            directory,
            provider: ProviderChoice::GitHub,
            account: String::new(),
            custom_host: None,
            credential_ref: None,
            disk_layout: DiskLayout::Grouped,
            sources: Vec::new(),
            repository_list_mode: RepositoryListMode::Grouped,
        };
        assert!(project.validate().is_ok());
    }
}
