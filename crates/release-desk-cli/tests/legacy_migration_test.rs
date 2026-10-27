//! End-to-end integration tests for legacy setup migration and adoption engine.

use release_desk_core::port::ProjectBackupPort;
use release_desk_core::snapshot::Snapshot;
use release_desk_store::ManagedLibrary;
use release_desk_store::legacy_scanner::{SystemLegacyScanner, compute_source_fingerprint};
use release_desk_store::project_backup::ProjectSnapshotManager;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

const REAL_ARTCRAFT_FILES: &[&str] = &[
    "/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json",
    "/Volumes/Devansh2/codes/ArtCraft/storytold-sync.sh",
    "/Volumes/Devansh2/codes/ArtCraft/release-desk.html",
];

fn compute_file_sha256(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

#[test]
fn test_legacy_migration_lifecycle_and_immutability() {
    // 1. Record hashes of real ArtCraft files if they exist
    let mut real_hashes_before = Vec::new();
    for f in REAL_ARTCRAFT_FILES {
        let p = Path::new(f);
        real_hashes_before.push((f, compute_file_sha256(p)));
    }

    // 2. Setup mock legacy directory
    let legacy_temp = tempdir().unwrap();
    let legacy_root = legacy_temp.path();
    let dot_rd = legacy_root.join(".release-desk");
    fs::create_dir_all(&dot_rd).unwrap();

    let legacy_json = r#"{
        "org": "storytold",
        "generated": "2026-10-27T00:00:00Z",
        "repos": [
            {
                "name": "engine",
                "description": "Game engine",
                "url": "https://github.com/storytold/engine",
                "isPrivate": false,
                "isArchived": false,
                "defaultBranchRef": {
                    "name": "main",
                    "target": { "oid": "1111111111111111111111111111111111111111" }
                }
            },
            {
                "name": "audio-driver",
                "description": "Spatial sound",
                "url": "https://github.com/storytold/audio-driver",
                "isPrivate": true,
                "isArchived": false,
                "defaultBranchRef": {
                    "name": "main",
                    "target": { "oid": "2222222222222222222222222222222222222222" }
                }
            }
        ]
    }"#;
    fs::write(dot_rd.join("data.json"), legacy_json).unwrap();

    let script_content = r#"#!/usr/bin/env bash
ORG="storytold"
REPOS="engine audio-driver"
"#;
    fs::write(legacy_root.join("storytold-sync.sh"), script_content).unwrap();

    let installers_dir = legacy_root.join("installers");
    fs::create_dir_all(&installers_dir).unwrap();
    fs::write(
        installers_dir.join("engine-v1.0.dmg"),
        b"mock dmg installer binary bytes",
    )
    .unwrap();

    // Fingerprint mock legacy setup before any operations
    let mock_hash_before = compute_source_fingerprint(legacy_root).unwrap();

    // 3. Setup temporary managed library and clone location
    let library_temp = tempdir().unwrap();
    let library_path = library_temp.path().join("Release Desk Library");
    let _managed_library = ManagedLibrary::new(&library_path).unwrap();

    let clone_temp = tempdir().unwrap();
    let clone_root = clone_temp.path().join("migrated-clones");
    fs::create_dir_all(&clone_root).unwrap();

    // 4. Test Scanner Port directly
    let scanner = SystemLegacyScanner;
    use release_desk_core::port::LegacyScannerPort;
    let analysis = scanner.scan(legacy_root).unwrap();
    assert_eq!(
        analysis.detected_kind,
        release_desk_core::migration::LegacySetupKind::ArtCraftWorkspace
    );
    assert_eq!(analysis.discovered_org, Some("storytold".to_string()));
    assert_eq!(analysis.repositories.len(), 2);
    assert_eq!(analysis.installer_assets.len(), 1);

    // Assert fingerprint unaltered after scan
    let mock_hash_after_scan = compute_source_fingerprint(legacy_root).unwrap();
    assert_eq!(
        mock_hash_before, mock_hash_after_scan,
        "Scan must not modify legacy source"
    );

    // 5. Test CLI Analyze Command
    let cli_bin = env!("CARGO_BIN_EXE_release-desk-cli");
    let analyze_output = Command::new(cli_bin)
        .arg("--library")
        .arg(&library_path)
        .arg("--json")
        .arg("migrate")
        .arg("analyze")
        .arg("--path")
        .arg(legacy_root)
        .output()
        .expect("failed to run release-desk migrate analyze");
    assert!(analyze_output.status.success());
    let analyze_json: serde_json::Value = serde_json::from_slice(&analyze_output.stdout).unwrap();
    assert_eq!(analyze_json["detected_kind"], "ArtCraftWorkspace");
    assert_eq!(analyze_json["repositories"].as_array().unwrap().len(), 2);

    // 6. Test CLI Preview Command
    let preview_output = Command::new(cli_bin)
        .arg("--library")
        .arg(&library_path)
        .arg("--json")
        .arg("migrate")
        .arg("preview")
        .arg("--path")
        .arg(legacy_root)
        .arg("--name")
        .arg("AdoptedStorytold")
        .arg("--location")
        .arg(&clone_root)
        .arg("--copy-installers")
        .output()
        .expect("failed to run release-desk migrate preview");
    assert!(preview_output.status.success());
    let preview_json: serde_json::Value = serde_json::from_slice(&preview_output.stdout).unwrap();
    assert_eq!(preview_json["project_name"], "AdoptedStorytold");
    assert_eq!(preview_json["actions"].as_array().unwrap().len(), 5);

    // 7. Test CLI Execute Command without --confirm (Must Fail)
    let unconfirmed_output = Command::new(cli_bin)
        .arg("--library")
        .arg(&library_path)
        .arg("--json")
        .arg("migrate")
        .arg("execute")
        .arg("--path")
        .arg(legacy_root)
        .arg("--name")
        .arg("AdoptedStorytold")
        .arg("--location")
        .arg(&clone_root)
        .output()
        .expect("failed to run unconfirmed migrate execute");
    assert!(
        !unconfirmed_output.status.success(),
        "Execute without --confirm must fail"
    );

    // 8. Test CLI Execute Command with --confirm (Must Succeed)
    let execute_output = Command::new(cli_bin)
        .arg("--library")
        .arg(&library_path)
        .arg("--json")
        .arg("migrate")
        .arg("execute")
        .arg("--path")
        .arg(legacy_root)
        .arg("--name")
        .arg("AdoptedStorytold")
        .arg("--location")
        .arg(&clone_root)
        .arg("--copy-installers")
        .arg("--confirm")
        .output()
        .expect("failed to run confirmed migrate execute");
    assert!(execute_output.status.success());
    let result_json: serde_json::Value = serde_json::from_slice(&execute_output.stdout).unwrap();
    assert_eq!(result_json["project_name"], "AdoptedStorytold");
    assert_eq!(result_json["imported_repositories"], 2);
    assert_eq!(result_json["imported_installers"], 1);
    assert_eq!(result_json["source_untouched"], true);

    // 9. Verify local backup snapshot exists in clone location
    let local_backup_path = clone_root.join(".release-desk/data.json");
    assert!(
        local_backup_path.is_file(),
        "Local backup snapshot must be written to clone directory"
    );

    let backup_manager = ProjectSnapshotManager::default();
    let read_backup = backup_manager.read_backup_snapshot(&clone_root).unwrap();
    assert!(read_backup.is_some(), "Backup snapshot must be readable");
    let snap: Snapshot = read_backup.unwrap();
    assert_eq!(snap.org, "AdoptedStorytold");
    assert_eq!(snap.repos.len(), 2);

    // 10. Verify legacy source directory remains 100% UNTOUCHED
    let mock_hash_after_migration = compute_source_fingerprint(legacy_root).unwrap();
    assert_eq!(
        mock_hash_before, mock_hash_after_migration,
        "Source directory was modified during migration! Immutability violated!"
    );

    // 11. Verify real ArtCraft files remain 100% UNTOUCHED
    for (f, hash_before) in real_hashes_before {
        let p = Path::new(f);
        let hash_after = compute_file_sha256(p);
        assert_eq!(
            hash_before, hash_after,
            "Real ArtCraft file '{}' was altered during migration tests!",
            f
        );
    }
}
