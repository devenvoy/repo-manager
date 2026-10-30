use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn initializes_and_links_snapshot_read_only() {
    let temp = tempfile::tempdir().expect("temporary folder");
    let source = temp.path().join("ArtCraft/.release-desk/data.json");
    fs::create_dir_all(source.parent().expect("source parent")).expect("source directory");
    let raw = r#"{"org":"storytold","generated":"2026-10-27T00:00:00Z","repos":[{"name":"example","description":null,"url":"https://github.com/storytold/example","isPrivate":false,"isArchived":false,"pushedAt":null,"primaryLanguage":null,"defaultBranchRef":{"name":"main","target":{"oid":"abc"}},"releases":{"nodes":[]}}]}"#;
    fs::write(&source, raw).expect("snapshot fixture");
    let library = temp.path().join("library");
    let binary = env!("CARGO_BIN_EXE_release-desk-cli");

    let init = Command::new(binary)
        .args([
            "--library",
            library.to_str().expect("library path"),
            "--json",
            "init",
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

    let val: Value = serde_json::from_slice(&init.stdout).expect("valid JSON");
    assert_eq!(val["initialized"], true);

    // Assert source file was NOT edited or modified
    assert_eq!(fs::read_to_string(&source).expect("source content"), raw);
}

#[test]
fn migration_command_is_not_exposed() {
    let binary = env!("CARGO_BIN_EXE_release-desk-cli");
    let help = Command::new(binary)
        .arg("--help")
        .output()
        .expect("run help");
    assert!(help.status.success());
    assert!(!String::from_utf8_lossy(&help.stdout).contains("migrate"));
    let result = Command::new(binary)
        .arg("migrate")
        .output()
        .expect("run command");
    assert!(!result.status.success());
}
