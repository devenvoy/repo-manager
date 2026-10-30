//! Integration tests for profile URL parsing, repository checklist selection,
//! layout computation (Grouped vs Flat), and SQLite persistence.

use release_desk_application::use_case::profile_ingestion::{
    DiscoveredRepoItem, IngestProfileUseCase, parse_profile_input,
};
use release_desk_core::port::ProjectStore;
use release_desk_core::project::{DiskLayout, ProjectId, ProjectIdentity, ProjectName};
use release_desk_core::provider::GitProviderKind;
use release_desk_store::boundary::ManagedLibrary;
use release_desk_store::project_backup::ProjectSnapshotManager;
use release_desk_store::sqlite_store::SqliteStore;
use std::path::PathBuf;

#[test]
fn test_parse_profile_urls_across_hosts() {
    let gh = parse_profile_input("https://github.com/storytold", GitProviderKind::GitHub).unwrap();
    assert_eq!(gh.provider, GitProviderKind::GitHub);
    assert_eq!(gh.login.as_str(), "storytold");

    let gl = parse_profile_input("https://gitlab.com/cloud-ops/", GitProviderKind::GitHub).unwrap();
    assert_eq!(gl.provider, GitProviderKind::GitLab);
    assert_eq!(gl.login.as_str(), "cloud-ops");

    let bb = parse_profile_input(
        "https://bitbucket.org/atlassian-team",
        GitProviderKind::GitHub,
    )
    .unwrap();
    assert_eq!(bb.provider, GitProviderKind::Bitbucket);
    assert_eq!(bb.login.as_str(), "atlassian-team");

    let bare = parse_profile_input("torvalds", GitProviderKind::GitHub).unwrap();
    assert_eq!(bare.provider, GitProviderKind::GitHub);
    assert_eq!(bare.login.as_str(), "torvalds");
}

#[test]
fn test_selectable_repos_selection_and_clone_planning() {
    let use_case = IngestProfileUseCase::new();
    let root = PathBuf::from("/Users/example/Codes/Projects");

    let proj = ProjectIdentity::with_clone_location(
        ProjectId::from_u128(77),
        ProjectName::new("Creative Fleet").unwrap(),
        Some(root.clone()),
    )
    .with_layout(DiskLayout::Grouped);

    let raw_repos = vec![
        DiscoveredRepoItem {
            name: "photocraft".to_string(),
            clone_url: "https://github.com/storytold/photocraft.git".to_string(),
            description: Some("RAW photo developer".to_string()),
            is_private: false,
            stars: 240,
            default_branch: "main".to_string(),
            has_release_asset: true,
        },
        DiscoveredRepoItem {
            name: "gridcraft".to_string(),
            clone_url: "https://github.com/storytold/gridcraft.git".to_string(),
            description: Some("Spreadsheet app".to_string()),
            is_private: false,
            stars: 95,
            default_branch: "main".to_string(),
            has_release_asset: true,
        },
    ];

    let mut selectable = use_case.prepare_selectable_repos(raw_repos);
    assert_eq!(selectable.len(), 2);
    assert!(selectable[0].selected);
    assert!(selectable[1].selected);

    // Grouped layout clone paths
    let plans = use_case
        .plan_clones(&proj, "storytold", &selectable)
        .unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(
        plans[0].target_path,
        PathBuf::from("/Users/example/Codes/Projects/storytold/photocraft")
    );
    assert_eq!(
        plans[1].target_path,
        PathBuf::from("/Users/example/Codes/Projects/storytold/gridcraft")
    );

    // Unselect one repo
    selectable[1].selected = false;
    let filtered_plans = use_case
        .plan_clones(&proj, "storytold", &selectable)
        .unwrap();
    assert_eq!(filtered_plans.len(), 1);
    assert_eq!(filtered_plans[0].repo_name, "photocraft");

    // Flat layout clone paths
    let flat_proj = proj.with_layout(DiskLayout::Flat);
    selectable[1].selected = true;
    let flat_plans = use_case
        .plan_clones(&flat_proj, "storytold", &selectable)
        .unwrap();
    assert_eq!(
        flat_plans[0].target_path,
        PathBuf::from("/Users/example/Codes/Projects/photocraft")
    );
}

#[test]
fn test_project_layout_roundtrip_in_sqlite() {
    let temp = tempfile::tempdir().unwrap();
    let library = ManagedLibrary::new(temp.path()).unwrap();
    let store = SqliteStore::open_in_memory(library).unwrap();

    let id = ProjectId::from_u128(999);
    let name = ProjectName::new("Flat Fleet Project").unwrap();
    let clone_loc = Some(temp.path().join("clones"));

    let project = ProjectIdentity::with_clone_location(id, name, clone_loc.clone())
        .with_layout(DiskLayout::Flat);

    store.save_project(&project).unwrap();

    let retrieved = store.get_project(id).unwrap().expect("project found");
    assert_eq!(retrieved.id, id);
    assert_eq!(retrieved.disk_layout, DiskLayout::Flat);
    assert_eq!(retrieved.clone_location, clone_loc);
}

#[test]
fn test_project_backup_offline_data_json() {
    let temp = tempfile::tempdir().unwrap();
    let clone_root = temp.path().join("MyProjectFleet");
    std::fs::create_dir_all(&clone_root).unwrap();

    let snapshot_file = ProjectSnapshotManager::snapshot_path(&clone_root);
    assert_eq!(snapshot_file, clone_root.join(".release-desk/data.json"));
}
