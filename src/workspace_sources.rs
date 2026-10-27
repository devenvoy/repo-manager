//! Source profiles and repository choices displayed within a project.

use serde::{Deserialize, Serialize};

/// Presentation preference; independent of the project's on-disk layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryListMode {
    #[default]
    Grouped,
    Flat,
}

/// A repository shown in the sample selection until provider discovery is connected.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceRepository {
    pub name: String,
    pub selected: bool,
}

/// A provider user or organization and its sample repositories.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSource {
    pub handle: String,
    pub repositories: Vec<SourceRepository>,
}

impl WorkspaceSource {
    /// Create a source from a handle or profile URL. Repository names are examples,
    /// not results fetched from the provider.
    pub fn sample(raw: &str) -> Result<Self, String> {
        let input = raw.trim();
        let handle = if input.starts_with("https://") || input.starts_with("http://") {
            let path = input
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or_default();
            path.split_once('/')
                .map(|(_, path)| path.trim_matches('/'))
                .unwrap_or_default()
        } else {
            input.trim_start_matches('@').trim_matches('/')
        };
        if handle.is_empty()
            || handle.contains(['?', '#', ' ', '\\'])
            || handle.chars().any(char::is_control)
        {
            return Err("Enter a user or organization handle, or a profile URL.".into());
        }
        Ok(Self {
            handle: handle.to_string(),
            repositories: ["example-app", "example-library", "example-tools"]
                .into_iter()
                .map(|name| SourceRepository {
                    name: name.into(),
                    selected: false,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_handle_and_profile_url() {
        assert_eq!(WorkspaceSource::sample(" @team ").unwrap().handle, "team");
        assert_eq!(
            WorkspaceSource::sample("https://gitlab.com/group/team/")
                .unwrap()
                .handle,
            "group/team"
        );
        assert!(WorkspaceSource::sample("https://github.com/").is_err());
        assert!(WorkspaceSource::sample(" ").is_err());
    }
}
