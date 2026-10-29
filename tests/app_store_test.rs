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
    image::RgbaImage::new(512, 512).save(&icon_file).unwrap();

    // Verify local icon discovery finds the 1024 png
    let found = repo_manager::modules::artwork::find_local_repo_icon(&repo_dir, "photocraft");
    assert_eq!(found, Some(icon_file.canonicalize().unwrap()));

    let custom_repo = temp.path().join("IndependentApp");
    let custom_icons = custom_repo.join("src-tauri/icons");
    std::fs::create_dir_all(&custom_icons).unwrap();
    image::RgbaImage::new(64, 64)
        .save(custom_icons.join("icon-64.png"))
        .unwrap();
    let full_icon = custom_icons.join("icon-512.png");
    image::RgbaImage::new(512, 512).save(&full_icon).unwrap();
    assert_eq!(
        repo_manager::modules::artwork::find_local_repo_icon(&custom_repo, "IndependentApp"),
        Some(full_icon.canonicalize().unwrap())
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

#[test]
fn test_find_icon_engine_manifest_and_sniffing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("my-web-app");
    std::fs::create_dir_all(root.join("public")).unwrap();

    let logo_file = root.join("public/logo.png");
    image::RgbaImage::new(256, 256).save(&logo_file).unwrap();

    let banner_file = root.join("public/banner.png");
    image::RgbaImage::new(1200, 300).save(&banner_file).unwrap();

    // Write package.json pointing to public/logo.png
    let pkg_json = r#"{"name": "my-web-app", "icon": "public/logo.png"}"#;
    std::fs::write(root.join("package.json"), pkg_json).unwrap();

    // Write README.md with an image
    let readme = "# My Web App\n\n![Logo](public/logo.png)\n";
    std::fs::write(root.join("README.md"), readme).unwrap();

    let icon = repo_manager::modules::artwork::find_local_repo_icon(&root, "my-web-app").unwrap();

    assert_eq!(icon, logo_file.canonicalize().unwrap());
}

#[test]
fn test_artcraft_photocraft_picks_square_icon_over_suite_banner() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("photocraft");
    std::fs::create_dir_all(root.join("assets/app-icon")).unwrap();

    // 1. Suite umbrella banner (1200x300 horizontal text banner "ARTCRAFT")
    let banner_file = root.join("assets/logo.svg");
    std::fs::write(&banner_file, r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 300"><text>ARTCRAFT</text></svg>"#).unwrap();

    // 2. Real square app icon (512x512 PNG)
    let icon_file = root.join("assets/app-icon/photocraft-1024.png");
    image::RgbaImage::new(512, 512).save(&icon_file).unwrap();

    // 3. Resolve icon for photocraft
    let resolved = repo_manager::modules::artwork::find_local_repo_icon(&root, "photocraft");
    assert!(resolved.is_some(), "Must resolve an icon for photocraft");
    assert_eq!(
        resolved.unwrap(),
        icon_file.canonicalize().unwrap(),
        "Must pick authentic photocraft-1024.png, not the ARTCRAFT banner"
    );
}

#[test]
fn test_tauri_app_resolves_from_tauri_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("figcraft");
    std::fs::create_dir_all(root.join("src-tauri/icons")).unwrap();

    let icon_32 = root.join("src-tauri/icons/32x32.png");
    image::RgbaImage::new(32, 32).save(&icon_32).unwrap();

    let icon_128 = root.join("src-tauri/icons/128x128.png");
    image::RgbaImage::new(128, 128).save(&icon_128).unwrap();

    let icon_file = root.join("src-tauri/icons/icon-512.png");
    image::RgbaImage::new(512, 512).save(&icon_file).unwrap();

    let tauri_json = r#"{
        "bundle": {
            "icon": ["icons/32x32.png", "icons/128x128.png", "icons/icon-512.png"]
        }
    }"#;
    std::fs::write(root.join("src-tauri/tauri.conf.json"), tauri_json).unwrap();

    let resolved = repo_manager::modules::artwork::find_local_repo_icon(&root, "figcraft");
    assert!(resolved.is_some(), "Must resolve icon from Tauri manifest");
    assert_eq!(
        resolved.unwrap(),
        icon_file.canonicalize().unwrap(),
        "Must pick highest-resolution icon-512.png over 32x32.png and 128x128.png"
    );
}

#[test]
fn test_highest_resolution_app_icon_is_chosen_over_low_res_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("designcraft");
    std::fs::create_dir_all(root.join("src-tauri/icons")).unwrap();
    std::fs::create_dir_all(root.join("assets/app-icon")).unwrap();

    // 1. Low-res 32x32 in Tauri manifest
    let icon_32 = root.join("src-tauri/icons/32x32.png");
    image::RgbaImage::new(32, 32).save(&icon_32).unwrap();
    let tauri_json = r#"{"bundle": {"icon": ["icons/32x32.png"]}}"#;
    std::fs::write(root.join("src-tauri/tauri.conf.json"), tauri_json).unwrap();

    // 2. High-res 1024x1024 app icon
    let icon_1024 = root.join("assets/app-icon/designcraft-1024.png");
    image::RgbaImage::new(1024, 1024).save(&icon_1024).unwrap();

    let resolved = repo_manager::modules::artwork::find_local_repo_icon(&root, "designcraft");
    assert!(resolved.is_some(), "Must find icon");
    assert_eq!(
        resolved.unwrap(),
        icon_1024.canonicalize().unwrap(),
        "Must prefer 1024x1024 asset icon over 32x32 manifest icon"
    );
}

#[test]
fn test_store_resolver_prefers_full_svg_over_matching_raster_exports() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("designcraft");
    let icons = repo.join("assets/app-icon");
    std::fs::create_dir_all(&icons).unwrap();
    image::RgbaImage::new(1024, 1024)
        .save(icons.join("designcraft-1024.png"))
        .unwrap();
    image::RgbaImage::new(512, 512)
        .save(icons.join("designcraft-macos-512.png"))
        .unwrap();
    std::fs::write(
        icons.join("designcraft-small.svg"),
        r#"<svg viewBox="0 0 512 512"><circle cx="256" cy="256" r="20"/></svg>"#,
    )
    .unwrap();
    let full_svg = icons.join("designcraft.svg");
    std::fs::write(
        &full_svg,
        r#"<svg viewBox="0 0 512 512"><circle cx="256" cy="256" r="200"/></svg>"#,
    )
    .unwrap();

    let selected = repo_manager::modules::artwork::find_local_repo_icon(&repo, "designcraft")
        .expect("Store icon");
    assert_eq!(selected, full_svg.canonicalize().unwrap());
}

#[test]
fn test_name_initials_clean() {
    use repo_manager::components::avatar::name_initials;
    assert_eq!(name_initials("PhotoCraft"), "PC");
    assert_eq!(name_initials("artcraft-services"), "AS");
    assert_eq!(name_initials("vector_craft"), "VC");
    assert_eq!(name_initials("cadcraft"), "CA");
    assert_eq!(name_initials("figcraft"), "FI");
}
