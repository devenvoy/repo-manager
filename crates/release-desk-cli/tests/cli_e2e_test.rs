//! End-to-end integration tests invoking the compiled release-desk-cli binary.

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const ARTCRAFT_FILE: &str = "/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json";

fn login_with_stdin(binary: &str, library_dir: &Path, provider: &str, token: &str) -> Output {
    use std::io::Write as _;
    let mut child = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().expect("library path"),
            "--json",
            "auth",
            "login",
            "--provider",
            provider,
            "--token-stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run auth login");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(token.as_bytes())
        .expect("write test token");
    child.wait_with_output().expect("wait for auth login")
}

#[test]
fn e2e_cli_full_workflow_test() {
    let binary = env!("CARGO_BIN_EXE_release-desk-cli");
    let temp = tempfile::tempdir().expect("tempdir");
    let library_dir = temp.path().join("library");
    let project_clone_dir = temp.path().join("project_clones/ArtCraftApp");

    // Pre-record ArtCraft hash if the file exists and is readable
    let artcraft_path = Path::new(ARTCRAFT_FILE);
    let original_artcraft_hash = if let Ok(bytes) = fs::read(artcraft_path) {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Some(format!("{:x}", hasher.finalize()))
    } else {
        None
    };

    // ---------------------------------------------------------
    // 1. release-desk init
    // ---------------------------------------------------------
    let init_out = Command::new(binary)
        .args(["--library", library_dir.to_str().unwrap(), "--json", "init"])
        .output()
        .expect("run init");

    assert!(
        init_out.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init_out.stderr)
    );

    let init_json: Value = serde_json::from_slice(&init_out.stdout).expect("valid json");
    assert_eq!(init_json["initialized"], true);
    assert!(library_dir.exists());

    // ---------------------------------------------------------
    // 2. release-desk project create (with clone location & auto data.json backup)
    // ---------------------------------------------------------
    let create_out = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().unwrap(),
            "--json",
            "project",
            "create",
            "--id",
            "100",
            "--name",
            "ArtCraftApp",
            "--account",
            "storytold",
            "--location",
            project_clone_dir.to_str().unwrap(),
        ])
        .output()
        .expect("run project create");

    assert!(
        create_out.status.success(),
        "project create failed: {}",
        String::from_utf8_lossy(&create_out.stderr)
    );

    let create_json: Value = serde_json::from_slice(&create_out.stdout).expect("valid json");
    assert_eq!(create_json["id"], 100);
    assert_eq!(create_json["name"], "ArtCraftApp");

    // CRITICAL USER REQUIREMENT:
    // Assert that <project_location>/.release-desk/data.json exists as a local backup snapshot!
    let local_backup_json = project_clone_dir.join(".release-desk/data.json");
    assert!(
        local_backup_json.exists(),
        "Local backup snapshot was not created at {}",
        local_backup_json.display()
    );

    let backup_content = fs::read_to_string(&local_backup_json).expect("read backup");
    let parsed_backup: Value = serde_json::from_str(&backup_content).expect("parse backup json");
    assert_eq!(parsed_backup["org"], "storytold");

    // ---------------------------------------------------------
    // 3. release-desk project list
    // ---------------------------------------------------------
    let list_out = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().unwrap(),
            "--json",
            "project",
            "list",
        ])
        .output()
        .expect("run project list");

    assert!(list_out.status.success());
    let list_json: Value = serde_json::from_slice(&list_out.stdout).expect("valid json");
    assert!(list_json.is_array());
    assert_eq!(list_json[0]["name"], "ArtCraftApp");

    // ---------------------------------------------------------
    // 4. release-desk project reconcile (In sync initially)
    // ---------------------------------------------------------
    let rec_out1 = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().unwrap(),
            "--json",
            "project",
            "reconcile",
            "--id",
            "100",
        ])
        .output()
        .expect("run reconcile");

    assert!(rec_out1.status.success());
    let rec_json1: Value = serde_json::from_slice(&rec_out1.stdout).expect("valid json");
    assert_eq!(rec_json1["report"]["disk_snapshot_present"], true);
    assert!(
        rec_json1["report"]["missing_in_database"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // ---------------------------------------------------------
    // 5. Simulate Disaster/Mismatch: Add repository to local data.json backup
    // ---------------------------------------------------------
    let modified_backup = r#"{
        "org": "storytold",
        "generated": "2026-10-27T00:00:00Z",
        "repos": [
            {
                "name": "restored-core",
                "description": "Core restored from disaster",
                "url": "https://github.com/storytold/restored-core",
                "isPrivate": false,
                "isArchived": false,
                "pushedAt": null,
                "primaryLanguage": null,
                "defaultBranchRef": {
                    "name": "main",
                    "target": { "oid": "rev-111" }
                },
                "releases": null
            }
        ]
    }"#;
    fs::write(&local_backup_json, modified_backup).expect("write modified backup");

    // Reconcile detects mismatch!
    let rec_out2 = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().unwrap(),
            "--json",
            "project",
            "reconcile",
            "--id",
            "100",
        ])
        .output()
        .expect("run reconcile");

    assert!(rec_out2.status.success());
    let rec_json2: Value = serde_json::from_slice(&rec_out2.stdout).expect("valid json");
    let missing_in_db = rec_json2["report"]["missing_in_database"]
        .as_array()
        .unwrap();
    assert_eq!(missing_in_db.len(), 1);
    assert_eq!(missing_in_db[0], "restored-core");

    // ---------------------------------------------------------
    // 6. Restore database from project data.json backup
    // ---------------------------------------------------------
    let restore_out = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().unwrap(),
            "--json",
            "project",
            "reconcile",
            "--id",
            "100",
            "--restore-db",
        ])
        .output()
        .expect("run restore");

    assert!(restore_out.status.success());
    let restore_json: Value = serde_json::from_slice(&restore_out.stdout).expect("valid json");
    assert_eq!(restore_json["restored_records"], 1);

    // Verify database now matches local backup snapshot
    let rec_out3 = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().unwrap(),
            "--json",
            "project",
            "reconcile",
            "--id",
            "100",
        ])
        .output()
        .expect("run reconcile");

    assert!(rec_out3.status.success());
    let rec_json3: Value = serde_json::from_slice(&rec_out3.stdout).expect("valid json");
    assert!(
        rec_json3["report"]["missing_in_database"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let matched = rec_json3["report"]["matched_repositories"]
        .as_array()
        .unwrap();
    assert_eq!(matched[0], "restored-core");

    // ---------------------------------------------------------
    // 7. Multi-Host Authentication: Token login across hosts
    // ---------------------------------------------------------
    let auth_gh = login_with_stdin(binary, &library_dir, "github", "ghp_fake_token_12345");
    if auth_gh.status.success() {
        let auth_gl = login_with_stdin(binary, &library_dir, "gitlab", "glpat_fake_token_67890");
        assert!(
            auth_gl.status.success(),
            "{}",
            String::from_utf8_lossy(&auth_gl.stderr)
        );

        let status_out = Command::new(binary)
            .args([
                "--library",
                library_dir.to_str().unwrap(),
                "--json",
                "auth",
                "status",
            ])
            .output()
            .expect("run auth status");
        assert!(status_out.status.success());
        let status_json: Value = serde_json::from_slice(&status_out.stdout).expect("valid json");
        assert_eq!(status_json["providers"]["GitHub"], true);
        assert_eq!(status_json["providers"]["GitLab"], true);
    } else {
        // macOS sandboxed test runs cannot write to Keychain. The CLI must report this
        // failure explicitly, while the remaining project and backup flow still runs.
        let stderr = String::from_utf8_lossy(&auth_gh.stderr);
        assert!(stderr.contains("Keychain error"), "{stderr}");
    }

    // ---------------------------------------------------------
    // 8. Safe Sync (Dry-Run)
    // ---------------------------------------------------------
    let sync_out = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().unwrap(),
            "--json",
            "sync",
            "--project-id",
            "100",
            "--dry-run",
        ])
        .output()
        .expect("run sync");

    assert!(sync_out.status.success());
    let sync_json: Value = serde_json::from_slice(&sync_out.stdout).expect("valid json");
    assert_eq!(sync_json["project_id"], 100);

    // ---------------------------------------------------------
    // 9. Backup Archive Export
    // ---------------------------------------------------------
    let archive_dest = temp.path().join("full_backup.tar.gz");
    let backup_out = Command::new(binary)
        .args([
            "--library",
            library_dir.to_str().unwrap(),
            "--json",
            "backup",
            "--output",
            archive_dest.to_str().unwrap(),
        ])
        .output()
        .expect("run backup");

    assert!(backup_out.status.success());
    let backup_json: Value = serde_json::from_slice(&backup_out.stdout).expect("valid json");
    assert!(backup_json["bytes_written"].as_u64().unwrap() > 0);
    assert!(archive_dest.exists());

    // ---------------------------------------------------------
    // 10. ArtCraft Safety Verification: File was NOT touched
    // ---------------------------------------------------------
    if let Some(expected_hash) = original_artcraft_hash {
        let current_bytes = fs::read(artcraft_path).expect("read artcraft file again");
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&current_bytes);
        let current_hash = format!("{:x}", hasher.finalize());
        assert_eq!(
            expected_hash, current_hash,
            "SAFETY VIOLATION: ArtCraft data.json was modified during CLI execution!"
        );
    }
}
