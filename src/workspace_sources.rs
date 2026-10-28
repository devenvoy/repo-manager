//! Source profiles and repository choices displayed within a project.

use release_desk_store::{FetchedProfile, parse_input_target};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Presentation preference; independent of the project's on-disk layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryListMode {
    #[default]
    Grouped,
    Flat,
}

/// A repository selectable for cloning and release management.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceRepository {
    pub name: String,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub stars: u32,
    #[serde(default)]
    pub forks: u32,
    #[serde(default)]
    pub primary_language: Option<String>,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub is_fork: bool,
    #[serde(default)]
    pub clone_url: String,
    #[serde(default)]
    pub default_branch: String,
    #[serde(default = "default_selected")]
    pub selected: bool,
}

const fn default_selected() -> bool {
    true
}

/// A provider user or organization and its discovered repositories.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSource {
    pub handle: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default = "default_provider")]
    pub provider: String,
    pub repositories: Vec<SourceRepository>,
}

fn default_provider() -> String {
    "github".to_string()
}

impl WorkspaceSource {
    /// Browsable HTTPS page corresponding to a provider clone URL.
    pub fn repository_browser_url(&self, repo: &SourceRepository) -> Option<String> {
        if let Ok(mut url) = url::Url::parse(&repo.clone_url) {
            if url.scheme() == "https" && url.host_str().is_some() {
                let _ = url.set_username("");
                let _ = url.set_password(None);
                let clean_path = url.path().trim_end_matches(".git").to_string();
                url.set_path(&clean_path);
                url.set_query(None);
                url.set_fragment(None);
                return Some(url.to_string());
            }
        }
        let target = if repo.full_name.contains('/') {
            repo.full_name.clone()
        } else {
            format!("{}/{}", self.handle, repo.name)
        };
        match self.provider.to_lowercase().as_str() {
            "github" => Some(format!("https://github.com/{target}")),
            "gitlab" => Some(format!("https://gitlab.com/{target}")),
            "bitbucket" => Some(format!("https://bitbucket.org/{target}")),
            "codeberg" => Some(format!("https://codeberg.org/{target}")),
            _ => None,
        }
    }

    /// User profile avatar URL, with deterministic fallback for popular providers.
    pub fn effective_avatar_url(&self) -> Option<String> {
        if let Some(ref url) = self.avatar_url {
            if !url.trim().is_empty() {
                return Some(url.clone());
            }
        }
        match self.provider.to_lowercase().as_str() {
            "github" if !self.handle.is_empty() => Some(format!(
                "https://avatars.githubusercontent.com/{}",
                self.handle
            )),
            "gitlab" if !self.handle.is_empty() => {
                Some(format!("https://gitlab.com/{}.png", self.handle))
            }
            _ => None,
        }
    }

    /// Local image path shared by the Sources and Repositories views.
    pub fn avatar_cache_request(&self) -> Option<(String, PathBuf)> {
        let url = self.effective_avatar_url()?;
        if !url.starts_with("https://") {
            return None;
        }
        let safe = |part: &str| {
            part.chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        };
        let directory = crate::settings::settings_path()
            .ok()?
            .parent()?
            .join("avatars");
        let path = directory.join(format!(
            "{}-{}.png",
            safe(&self.provider),
            safe(&self.handle)
        ));
        Some((url, path))
    }

    /// Convert from a fetched remote profile.
    pub fn from_fetched(profile: FetchedProfile, specific_repo: Option<&str>) -> Self {
        let repositories = profile
            .repositories
            .into_iter()
            .map(|r| {
                let is_match = specific_repo
                    .map(|target| {
                        if target.contains('/') {
                            target.eq_ignore_ascii_case(&r.full_name)
                        } else {
                            target.eq_ignore_ascii_case(&r.name)
                        }
                    })
                    .unwrap_or(false);
                SourceRepository {
                    name: r.name,
                    full_name: r.full_name,
                    description: r.description,
                    stars: r.stars,
                    forks: r.forks,
                    primary_language: r.primary_language,
                    is_private: r.is_private,
                    is_fork: r.is_fork,
                    clone_url: r.clone_url,
                    default_branch: r.default_branch,
                    selected: is_match,
                }
            })
            .collect();

        Self {
            handle: profile.handle,
            display_name: profile.display_name,
            avatar_url: profile.avatar_url,
            bio: profile.bio,
            provider: profile.provider,
            repositories,
        }
    }

    /// Replace discovered metadata while keeping the user's existing choices.
    /// Newly discovered repositories remain unselected until explicitly chosen.
    pub fn refresh_from_fetched(&mut self, profile: FetchedProfile) {
        let mut refreshed = Self::from_fetched(profile, None);
        for repo in &mut refreshed.repositories {
            if let Some(previous) = self.repositories.iter().find(|previous| {
                if !repo.full_name.is_empty() && !previous.full_name.is_empty() {
                    repo.full_name.eq_ignore_ascii_case(&previous.full_name)
                } else {
                    repo.name.eq_ignore_ascii_case(&previous.name)
                }
            }) {
                repo.selected = previous.selected;
            }
        }
        *self = refreshed;
    }

    /// Extract the handle from user input.
    pub fn extract_handle(raw: &str, default_provider: &str) -> Result<String, String> {
        let (_prov, owner, _repo) = parse_input_target(raw, default_provider);
        if owner.is_empty()
            || owner.contains(['?', '#', ' ', '\\'])
            || owner.chars().any(char::is_control)
        {
            return Err("Enter a user or organization handle, or a profile URL.".into());
        }
        Ok(owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_browses_before_selecting_clones() {
        let repo = release_desk_store::FetchedRepo {
            name: "one".into(),
            full_name: "team/one".into(),
            description: None,
            stars: 0,
            forks: 0,
            primary_language: None,
            is_private: false,
            is_fork: false,
            clone_url: "https://github.com/team/one.git".into(),
            default_branch: "main".into(),
        };
        let profile = FetchedProfile {
            handle: "team".into(),
            display_name: None,
            avatar_url: None,
            bio: None,
            provider: "github".into(),
            repositories: vec![repo],
        };
        assert!(!WorkspaceSource::from_fetched(profile.clone(), None).repositories[0].selected);
        assert!(WorkspaceSource::from_fetched(profile, Some("ONE")).repositories[0].selected);
    }

    #[test]
    fn explicit_nested_repository_selects_only_its_full_name() {
        let make_repo = |full_name: &str| release_desk_store::FetchedRepo {
            name: "app".into(),
            full_name: full_name.into(),
            description: None,
            stars: 0,
            forks: 0,
            primary_language: None,
            is_private: false,
            is_fork: false,
            clone_url: String::new(),
            default_branch: String::new(),
        };
        let profile = FetchedProfile {
            handle: "team".into(),
            display_name: None,
            avatar_url: None,
            bio: None,
            provider: "gitlab".into(),
            repositories: vec![make_repo("team/one/app"), make_repo("team/two/app")],
        };
        let source = WorkspaceSource::from_fetched(profile, Some("team/two/app"));
        assert!(!source.repositories[0].selected);
        assert!(source.repositories[1].selected);
    }

    #[test]
    fn refresh_preserves_choices_and_does_not_select_new_repositories() {
        fn repo(name: &str) -> release_desk_store::FetchedRepo {
            release_desk_store::FetchedRepo {
                name: name.into(),
                full_name: format!("team/{name}"),
                description: None,
                stars: 0,
                forks: 0,
                primary_language: None,
                is_private: false,
                is_fork: false,
                clone_url: format!("https://github.com/team/{name}.git"),
                default_branch: "main".into(),
            }
        }
        fn profile(repositories: Vec<release_desk_store::FetchedRepo>) -> FetchedProfile {
            FetchedProfile {
                handle: "team".into(),
                display_name: None,
                avatar_url: None,
                bio: None,
                provider: "github".into(),
                repositories,
            }
        }
        let mut source =
            WorkspaceSource::from_fetched(profile(vec![repo("one"), repo("two")]), None);
        source.repositories[0].selected = true;
        let mut changed = repo("ONE");
        changed.stars = 42;
        source.refresh_from_fetched(profile(vec![repo("two"), changed, repo("three")]));
        assert!(!source.repositories[0].selected);
        assert!(source.repositories[1].selected);
        assert_eq!(source.repositories[1].stars, 42);
        assert!(!source.repositories[2].selected);
    }

    #[test]
    fn accepts_handle_and_profile_url() {
        assert_eq!(
            WorkspaceSource::extract_handle(" @team ", "github").unwrap(),
            "team"
        );
        assert_eq!(
            WorkspaceSource::extract_handle("https://gitlab.com/group/team/", "gitlab").unwrap(),
            "group"
        );
        assert!(WorkspaceSource::extract_handle("https://github.com/", "github").is_err());
        assert!(WorkspaceSource::extract_handle(" ", "github").is_err());
    }
}
