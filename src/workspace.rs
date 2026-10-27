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
        if self.account.trim().is_empty() {
            return Err("Enter the account you will use for this project.".into());
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
}
