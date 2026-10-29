//! Release data shown in a project workspace.
use release_desk_store::workspace_releases::{
    LocalAssetState, WorkspaceAsset, WorkspaceRelease, local_archived_files, local_asset_state,
    newer_release_with_local_older, release_asset_type, release_asset_type_for_extension,
};
use std::path::{Path, PathBuf};

/// A repository's most recent release fetch and its result.
#[derive(Clone, Debug)]
pub struct RepositoryReleases {
    pub provider: String,
    pub owner: String,
    pub repository: String,
    pub result: Result<Vec<WorkspaceRelease>, String>,
}

/// Filesystem-backed release data collected once, outside of the render path.
#[derive(Clone, Debug)]
pub struct ReleasePresentation {
    pub icon_path: Option<PathBuf>,
    pub latest_tag: Option<String>,
    pub published_at: Option<String>,
    pub asset: Option<WorkspaceAsset>,
    pub pkg_kind: String,
    pub local_state: LocalAssetState,
    pub newer_update: Option<(String, String)>,
    pub archived_file: Option<(String, u64)>,
}

impl Default for ReleasePresentation {
    fn default() -> Self {
        Self {
            icon_path: None,
            latest_tag: None,
            published_at: None,
            asset: None,
            pkg_kind: String::new(),
            local_state: LocalAssetState::Missing,
            newer_update: None,
            archived_file: None,
        }
    }
}

/// Build the filesystem-backed portion of the release shelf in one background pass.
pub fn build_release_presentations(
    project_directory: &Path,
    repositories: &[RepositoryReleases],
    extension: &str,
) -> Vec<ReleasePresentation> {
    let extension = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let selected_kind = release_asset_type_for_extension(&extension);
    let selected_extension = selected_kind.map(|kind| kind.extension);
    let pkg_kind = selected_kind
        .map(|kind| kind.label.to_string())
        .unwrap_or_else(|| "Package".to_string());

    crate::modules::parallel::map_bounded(
        repositories,
        crate::modules::parallel::IO_WORKERS,
        |repository| {
            let icon_path = crate::modules::icon_agent::resolve_icon_for(
                Some(project_directory),
                &repository.provider,
                &repository.owner,
                &repository.repository,
            );

            let Ok(releases) = &repository.result else {
                return ReleasePresentation {
                    icon_path,
                    pkg_kind: pkg_kind.clone(),
                    ..Default::default()
                };
            };

            let latest_release = releases.iter().find(|release| !release.is_draft);
            let target_asset = latest_release.and_then(|release| {
                release
                    .assets
                    .iter()
                    .find(|asset| match selected_extension {
                        Some(expected) => release_asset_type(&asset.name)
                            .is_some_and(|kind| kind.extension == expected),
                        None => asset
                            .name
                            .to_ascii_lowercase()
                            .ends_with(&format!(".{extension}")),
                    })
            });
            let local_state = match (latest_release, target_asset) {
                (Some(release), Some(asset)) => local_asset_state(
                    project_directory,
                    &repository.owner,
                    &repository.repository,
                    &release.tag_name,
                    &asset.name,
                    asset.size,
                ),
                _ => LocalAssetState::Missing,
            };
            let newer_update = latest_release.and_then(|_| {
                newer_release_with_local_older(
                    project_directory,
                    &repository.owner,
                    &repository.repository,
                    releases,
                )
            });
            let archived_file =
                local_archived_files(project_directory, &repository.repository, &extension)
                    .into_iter()
                    .next();

            ReleasePresentation {
                icon_path,
                latest_tag: latest_release.map(|release| release.tag_name.clone()),
                published_at: latest_release.and_then(|release| release.published_at.clone()),
                asset: target_asset.cloned(),
                pkg_kind: pkg_kind.clone(),
                local_state,
                newer_update: newer_update
                    .map(|(latest, older)| (latest.to_string(), older.to_string())),
                archived_file,
            }
        },
    )
}

/// Data exists only after a real fetch; errors remain attached to their repository.
#[derive(Clone, Debug, Default)]
pub enum ReleaseViewState {
    #[default]
    NotLoaded,
    Loading,
    Loaded {
        repositories: Vec<RepositoryReleases>,
        presentations: Vec<ReleasePresentation>,
        extension: String,
    },
}

impl ReleaseViewState {
    pub fn repositories(&self) -> Option<&[RepositoryReleases]> {
        match self {
            Self::Loaded { repositories, .. } => Some(repositories),
            Self::NotLoaded | Self::Loading => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, size: u64) -> WorkspaceAsset {
        WorkspaceAsset {
            name: name.to_string(),
            download_url: format!("https://example.com/{name}"),
            size: Some(size),
        }
    }

    fn release(tag: &str, published_at: &str, assets: Vec<WorkspaceAsset>) -> WorkspaceRelease {
        WorkspaceRelease {
            tag_name: tag.to_string(),
            description: None,
            is_draft: false,
            is_prerelease: None,
            published_at: Some(published_at.to_string()),
            assets,
        }
    }

    fn repository(result: Result<Vec<WorkspaceRelease>, String>) -> RepositoryReleases {
        RepositoryReleases {
            provider: "github".to_string(),
            owner: "owner".to_string(),
            repository: "repo".to_string(),
            result,
        }
    }

    #[test]
    fn presentations_follow_selected_file_type() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("Release Zips/old")).unwrap();
        std::fs::write(directory.path().join("Release Zips/repo-v2.zip"), b"new").unwrap();
        std::fs::write(
            directory.path().join("Release Zips/old/repo-v1.zip"),
            b"old",
        )
        .unwrap();
        let repositories = vec![repository(Ok(vec![
            release(
                "v2",
                "2026-01-02T00:00:00Z",
                vec![asset("repo-v2.dmg", 3), asset("repo-v2.zip", 3)],
            ),
            release("v1", "2026-01-01T00:00:00Z", vec![asset("repo-v1.zip", 3)]),
        ]))];

        let presentations = build_release_presentations(directory.path(), &repositories, "zip");

        assert_eq!(presentations.len(), 1);
        let presentation = &presentations[0];
        assert_eq!(presentation.latest_tag.as_deref(), Some("v2"));
        assert_eq!(presentation.pkg_kind, "ZIP");
        assert_eq!(
            presentation.asset.as_ref().map(|asset| asset.name.as_str()),
            Some("repo-v2.zip")
        );
        assert_eq!(presentation.local_state, LocalAssetState::SizeMatches);
        assert_eq!(
            presentation
                .archived_file
                .as_ref()
                .map(|(name, _)| name.as_str()),
            Some("repo-v1.zip")
        );
    }

    #[test]
    fn failed_repository_still_has_a_shelf_presentation() {
        let directory = tempfile::tempdir().unwrap();
        let presentations = build_release_presentations(
            directory.path(),
            &[repository(Err("network failed".to_string()))],
            "dmg",
        );

        assert_eq!(presentations.len(), 1);
        assert_eq!(presentations[0].pkg_kind, "DMG");
        assert!(presentations[0].latest_tag.is_none());
        assert!(presentations[0].asset.is_none());
        assert_eq!(presentations[0].local_state, LocalAssetState::Missing);
    }
}
