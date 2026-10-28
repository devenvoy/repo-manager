//! Integration tests for Local App Store catalog aggregation, version resolution,
//! and 3-state action button transitions (Install, Update, Open).

use release_desk_application::use_case::app_store::{
    AppInstallationProbe, AppStoreAggregatorUseCase,
};
use release_desk_core::app_catalog::{AppActionState, default_artcraft_catalog, find_known_app};
use release_desk_core::project::{ProjectId, ProjectIdentity, ProjectName};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct TestInstallationProbe {
    installed: HashMap<String, String>, // binary_name -> installed version
}

impl AppInstallationProbe for TestInstallationProbe {
    fn probe_installed_version(&self, _target_dir: &Path, binary_name: &str) -> Option<String> {
        self.installed.get(binary_name).cloned()
    }
}

#[test]
fn test_app_catalog_builtin_completeness() {
    let catalog = default_artcraft_catalog();
    assert_eq!(
        catalog.len(),
        8,
        "ArtCraft suite consists of 8 core applications"
    );

    let ids: Vec<_> = catalog.iter().map(|a| a.id.as_str()).collect();
    assert!(ids.contains(&"artcraft"));
    assert!(ids.contains(&"photocraft"));
    assert!(ids.contains(&"gridcraft"));
    assert!(ids.contains(&"filmcraft"));
    assert!(ids.contains(&"vectorcraft"));
    assert!(ids.contains(&"pdfcraft"));
    assert!(ids.contains(&"soundcraft"));
    assert!(ids.contains(&"cadcraft"));

    for app in &catalog {
        assert!(!app.name.is_empty());
        assert!(!app.description.is_empty());
        assert!(!app.changelog.is_empty(), "every app has release notes");
        assert!(app.binary_name.ends_with(".app"));
    }
}

#[test]
fn test_find_known_app_matching() {
    assert_eq!(find_known_app("PhotoCraft").unwrap().id, "photocraft");
    assert_eq!(
        find_known_app("storytold/gridcraft").unwrap().id,
        "gridcraft"
    );
    assert_eq!(find_known_app("cadcraft-engine").unwrap().id, "cadcraft");
    assert!(find_known_app("unknown-repo").is_none());
}

#[test]
fn test_app_store_3_state_actions_and_metrics() {
    let mut installed = HashMap::new();
    // 1. GridCraft is up to date: installed v1.9.0, latest v1.9.0
    installed.insert("GridCraft.app".to_string(), "v1.9.0".to_string());
    // 2. PhotoCraft has an update: installed v2.3.9, latest v2.4.0
    installed.insert("PhotoCraft.app".to_string(), "v2.3.9".to_string());
    // 3. FilmCraft is not installed

    let probe = TestInstallationProbe { installed };
    let aggregator = AppStoreAggregatorUseCase::new(probe, PathBuf::from("~/Applications"));

    let project = ProjectIdentity::new(
        ProjectId::from_u128(10),
        ProjectName::new("ArtCraft Fleet").unwrap(),
    );
    let projects = vec![project];

    let repo_releases = vec![
        (10, "gridcraft".to_string(), Some("v1.9.0".to_string())),
        (10, "photocraft".to_string(), Some("v2.4.0".to_string())),
        (10, "filmcraft".to_string(), Some("v1.0.1".to_string())),
    ];

    let (items, metrics) = aggregator.aggregate_apps(&projects, &repo_releases);

    assert_eq!(items.len(), 3);
    assert_eq!(metrics.total_apps, 3);
    assert_eq!(metrics.up_to_date, 1, "GridCraft is up-to-date");
    assert_eq!(metrics.updates_available, 1, "PhotoCraft has 1 update");
    assert_eq!(metrics.installable, 1, "FilmCraft is installable");

    let gc = items.iter().find(|i| i.metadata.id == "gridcraft").unwrap();
    assert_eq!(
        gc.state,
        AppActionState::UpToDate {
            version: "v1.9.0".to_string()
        }
    );

    let pc = items
        .iter()
        .find(|i| i.metadata.id == "photocraft")
        .unwrap();
    assert_eq!(
        pc.state,
        AppActionState::UpdateAvailable {
            installed_version: "v2.3.9".to_string(),
            latest_version: "v2.4.0".to_string()
        }
    );

    let fc = items.iter().find(|i| i.metadata.id == "filmcraft").unwrap();
    assert_eq!(fc.state, AppActionState::Installable);
}

#[test]
fn test_dynamic_repo_icon_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let repo_dir = temp.path().join("PhotoCraft");
    let icon_dir = repo_dir.join("assets").join("app-icon");
    std::fs::create_dir_all(&icon_dir).unwrap();

    let icon_file = icon_dir.join("photocraft-1024.png");
    std::fs::write(&icon_file, b"fake png data").unwrap();

    // Verify local icon discovery finds the 1024 png
    let found = repo_manager::components::app_icon::find_local_repo_icon(&repo_dir, "photocraft");
    assert_eq!(found, Some(icon_file));

    // Verify cache request for ArtCraft apps points to WebP on getartcraft.com
    let cache_req = repo_manager::components::app_icon::repo_icon_cache_request(
        "photocraft",
        Some("storytold"),
    );
    assert!(cache_req.is_some());
    let (url, path) = cache_req.unwrap();
    assert_eq!(
        url,
        "https://getartcraft.com/images/apps/photocraft/icon.webp"
    );
    assert!(path.to_string_lossy().contains("artcraft-photocraft.webp"));

    // Verify cache request for non-ArtCraft generic repo points to GitHub owner avatar
    let generic_req = repo_manager::components::app_icon::repo_icon_cache_request(
        "test-generic-app-xyz",
        Some("genericowner"),
    );
    assert!(generic_req.is_some());
    let (gen_url, gen_path) = generic_req.unwrap();
    assert_eq!(gen_url, "https://github.com/genericowner.png");
    assert!(
        gen_path
            .to_string_lossy()
            .contains("genericowner-test-generic-app-xyz.png")
    );
}

#[test]
fn test_archived_files_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let old_dir = temp.path().join("Release Dmgs").join("old");
    std::fs::create_dir_all(&old_dir).unwrap();

    let old_dmg = old_dir.join("PhotoCraft-0.9.0-aarch64.dmg");
    std::fs::write(&old_dmg, b"dummy old dmg bytes").unwrap();

    let archived = release_desk_store::workspace_releases::local_archived_files(
        temp.path(),
        "photocraft",
        "dmg",
    );
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].0, "PhotoCraft-0.9.0-aarch64.dmg");
    assert_eq!(archived[0].1, 19);
}
