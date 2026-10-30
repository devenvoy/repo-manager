//! Local App Store aggregator use case.
//!
//! Aggregates runnable desktop applications across all workspace projects,
//! matches them against on-disk installations in `~/Applications`, and derives
//! 3-state action buttons (`Install`, `Update`, `Open`).

use release_desk_core::app_catalog::{AppActionState, AppStoreItem, find_known_app};
use release_desk_core::project::ProjectIdentity;
use std::path::{Path, PathBuf};

/// Summary metrics for the Local App Store navigation badges.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AppStoreMetrics {
    pub total_apps: usize,
    pub updates_available: usize,
    pub up_to_date: usize,
    pub installable: usize,
}

/// Abstract port for probing installed desktop application bundles.
pub trait AppInstallationProbe: Send + Sync {
    /// Check whether an app binary (e.g. `PhotoCraft.app`) is installed in `target_dir`,
    /// returning its version string if present.
    fn probe_installed_version(&self, target_dir: &Path, binary_name: &str) -> Option<String>;
}

/// Filesystem-backed installation probe for macOS application bundles.
#[derive(Clone, Debug, Default)]
pub struct FsAppInstallationProbe;

impl AppInstallationProbe for FsAppInstallationProbe {
    fn probe_installed_version(&self, target_dir: &Path, binary_name: &str) -> Option<String> {
        let app_bundle = target_dir.join(binary_name);
        if !app_bundle.exists() {
            return None;
        }

        // On macOS, Info.plist holds CFBundleShortVersionString
        let plist_path = app_bundle.join("Contents/Info.plist");
        if plist_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&plist_path) {
                // Simple parser for <key>CFBundleShortVersionString</key><string>x.y.z</string>
                if let Some(pos) = content.find("CFBundleShortVersionString") {
                    let after = &content[pos..];
                    if let (Some(s_start), Some(s_end)) =
                        (after.find("<string>"), after.find("</string>"))
                    {
                        let ver = &after[s_start + 8..s_end];
                        return Some(ver.trim().to_string());
                    }
                }
            }
        }

        // If Info.plist is absent or cannot be parsed, treat as installed with generic indicator
        Some("installed".to_string())
    }
}

/// Use case that aggregates applications across projects and computes store state.
pub struct AppStoreAggregatorUseCase<P: AppInstallationProbe> {
    probe: P,
    install_target_dir: PathBuf,
}

impl<P: AppInstallationProbe> AppStoreAggregatorUseCase<P> {
    pub fn new(probe: P, install_target_dir: PathBuf) -> Self {
        Self {
            probe,
            install_target_dir,
        }
    }

    /// Aggregates all runnable apps from tracked repositories across all given projects.
    pub fn aggregate_apps(
        &self,
        projects: &[ProjectIdentity],
        repo_releases: &[(u128, String, Option<String>)], // (project_id, repo_name, latest_release_tag)
    ) -> (Vec<AppStoreItem>, AppStoreMetrics) {
        let mut items = Vec::new();

        // 1. Ingest apps linked to existing project repos
        for (project_id, repo_name, latest_release) in repo_releases {
            if let Some(meta) = find_known_app(repo_name) {
                let project_name = projects
                    .iter()
                    .find(|p| p.id.as_u128() == *project_id)
                    .map(|p| p.name.as_str().to_string())
                    .unwrap_or_else(|| "Workspace Fleet".to_string());

                let installed_version = self
                    .probe
                    .probe_installed_version(&self.install_target_dir, &meta.binary_name);

                let item = AppStoreItem::new(
                    meta,
                    release_desk_core::project::ProjectId::from_u128(*project_id),
                    project_name,
                    repo_name.clone(),
                    latest_release.clone(),
                    installed_version,
                );
                items.push(item);
            }
        }

        // Sort by category then name
        items.sort_by(|a, b| {
            a.metadata
                .category
                .cmp(&b.metadata.category)
                .then_with(|| a.metadata.name.cmp(&b.metadata.name))
        });

        // Compute metrics
        let mut metrics = AppStoreMetrics {
            total_apps: items.len(),
            updates_available: 0,
            up_to_date: 0,
            installable: 0,
        };

        for item in &items {
            match item.state {
                AppActionState::Installable => metrics.installable += 1,
                AppActionState::UpdateAvailable { .. } => metrics.updates_available += 1,
                AppActionState::UpToDate { .. } => metrics.up_to_date += 1,
            }
        }

        (items, metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use release_desk_core::project::{ProjectId, ProjectName};
    use std::collections::HashMap;

    #[derive(Default)]
    struct MockAppProbe {
        installed: HashMap<String, String>, // binary_name -> version
    }

    impl AppInstallationProbe for MockAppProbe {
        fn probe_installed_version(&self, _target_dir: &Path, binary_name: &str) -> Option<String> {
            self.installed.get(binary_name).cloned()
        }
    }

    #[test]
    fn empty_project_does_not_invent_apps() {
        let use_case = AppStoreAggregatorUseCase::new(
            MockAppProbe::default(),
            PathBuf::from("/example/applications"),
        );
        let project = ProjectIdentity::new(
            ProjectId::from_u128(1),
            ProjectName::new("Any project").unwrap(),
        );
        let (items, metrics) = use_case.aggregate_apps(&[project], &[]);
        assert!(items.is_empty());
        assert_eq!(metrics.total_apps, 0);
    }

    #[test]
    fn test_app_store_aggregation_and_metrics() {
        let mut installed = HashMap::new();
        // PhotoCraft is installed but has an update
        installed.insert("PhotoCraft.app".to_string(), "v2.3.9".to_string());
        // GridCraft is installed and up to date
        installed.insert("GridCraft.app".to_string(), "v1.9.0".to_string());

        let probe = MockAppProbe { installed };
        let target_dir = PathBuf::from("/Users/test/Applications");
        let use_case = AppStoreAggregatorUseCase::new(probe, target_dir);

        let proj = ProjectIdentity::new(
            ProjectId::from_u128(1),
            ProjectName::new("ArtCraft").unwrap(),
        );
        let projects = vec![proj];

        let repo_releases = vec![
            (1, "photocraft".to_string(), Some("v2.4.0".to_string())),
            (1, "gridcraft".to_string(), Some("v1.9.0".to_string())),
            (1, "filmcraft".to_string(), Some("v1.0.1".to_string())),
        ];

        let (items, metrics) = use_case.aggregate_apps(&projects, &repo_releases);
        assert_eq!(items.len(), 3);
        assert_eq!(metrics.total_apps, 3);
        assert_eq!(metrics.updates_available, 1); // PhotoCraft
        assert_eq!(metrics.up_to_date, 1); // GridCraft
        assert_eq!(metrics.installable, 1); // FilmCraft

        let pc = items
            .iter()
            .find(|i| i.metadata.id == "photocraft")
            .unwrap();
        assert!(matches!(pc.state, AppActionState::UpdateAvailable { .. }));

        let gc = items.iter().find(|i| i.metadata.id == "gridcraft").unwrap();
        assert!(matches!(gc.state, AppActionState::UpToDate { .. }));

        let fc = items.iter().find(|i| i.metadata.id == "filmcraft").unwrap();
        assert!(matches!(fc.state, AppActionState::Installable));
    }
}
