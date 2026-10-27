use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn initializes_and_reports_linked_snapshot_without_editing_it() {
    let temp = tempfile::tempdir().expect("temporary folder");
    let source = temp.path().join("ArtCraft/.release-desk/data.json");
    fs::create_dir_all(source.parent().expect("source parent")).expect("source directory");
    let raw = r#"{"org":"storytold","generated":"2026-10-27T00:00:00Z","repos":[{"name":"example","description":null,"url":"https://github.com/storytold/example","isPrivate":false,"isArchived":false,"pushedAt":null,"primaryLanguage":null,"defaultBranchRef":{"name":"main","target":{"oid":"abc"}},"releases":{"nodes":[]}}]}"#;
    fs::write(&source, raw).expect("snapshot fixture");
    let config = temp.path().join("config/config.json");
    let library = temp.path().join("library");
    let binary = env!("CARGO_BIN_EXE_release-desk-cli");

    let init = Command::new(binary)
        .args([
            "--config",
            config.to_str().expect("config path"),
            "init",
            "--library",
            library.to_str().expect("library path"),
            "--snapshot",
            source.to_str().expect("snapshot path"),
        ])
        .output()
        .expect("run init");
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert_eq!(fs::read_to_string(&source).expect("source content"), raw);

    let status = Command::new(binary)
        .args([
            "--config",
            config.to_str().expect("config path"),
            "status",
            "--json",
        ])
        .output()
        .expect("run status");
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let value: Value = serde_json::from_slice(&status.stdout).expect("valid JSON");
    assert_eq!(value["project"], "ArtCraft");
    assert_eq!(value["repositories"], 1);
    assert_eq!(value["stale"], false);

    let sync = Command::new(binary)
        .args([
            "--config",
            config.to_str().expect("config path"),
            "sync",
            "example",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("run dry sync");
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let sync_value: Value = serde_json::from_slice(&sync.stdout).expect("valid sync JSON");
    assert_eq!(sync_value["status"], "NotCloned");
    assert!(
        std::path::Path::new(sync_value["path"].as_str().expect("clone path"))
            .starts_with(library.canonicalize().expect("canonical library"))
    );
    assert_eq!(fs::read_to_string(&source).expect("source content"), raw);
}

#[test]
fn noninteractive_init_requires_explicit_library() {
    let binary = env!("CARGO_BIN_EXE_release-desk-cli");
    let result = Command::new(binary)
        .args([
            "--config",
            "/private/tmp/release-desk-never-created-config.json",
            "init",
        ])
        .output()
        .expect("run init");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--library is required"));
}
