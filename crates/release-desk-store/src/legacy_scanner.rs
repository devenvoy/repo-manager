//! Read-only inspection and discovery engine for legacy workspaces, scripts, and snapshots.

use release_desk_core::DomainError;
use release_desk_core::migration::{LegacySetupAnalysis, LegacySetupKind};
use release_desk_core::parse_snapshot;
use release_desk_core::port::LegacyScannerPort;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Production implementation of `LegacyScannerPort`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemLegacyScanner;

impl LegacyScannerPort for SystemLegacyScanner {
    fn scan(&self, path: &Path) -> Result<LegacySetupAnalysis, DomainError> {
        scan_legacy_setup(path).map_err(|e| DomainError::PortFailure {
            port: "LegacyScanner",
            message: format!("Failed to scan legacy setup at {}: {e}", path.display()),
        })
    }

    fn compute_fingerprint(&self, path: &Path) -> Result<String, DomainError> {
        compute_source_fingerprint(path).map_err(|e| DomainError::PortFailure {
            port: "LegacyScanner",
            message: format!("Failed to compute fingerprint for {}: {e}", path.display()),
        })
    }
}

/// Safely inspects a filesystem path to identify legacy Release Desk or ArtCraft setups.
/// Strictly read-only: does not open any file with write permissions or alter timestamps.
pub fn scan_legacy_setup(source_path: &Path) -> Result<LegacySetupAnalysis, io::Error> {
    if !source_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Path does not exist: {}", source_path.display()),
        ));
    }

    if source_path.is_file() {
        return scan_file(source_path);
    }

    scan_directory(source_path)
}

fn scan_file(path: &Path) -> Result<LegacySetupAnalysis, io::Error> {
    let mut analysis = LegacySetupAnalysis::new(path.to_path_buf(), LegacySetupKind::Unknown);

    let content = fs::read_to_string(path)?;

    // Attempt to parse as Snapshot JSON
    if let Ok(snapshot) = parse_snapshot(&content) {
        analysis.detected_kind = LegacySetupKind::SnapshotFile;
        analysis.discovered_org = Some(snapshot.org);
        analysis.repositories = snapshot.repos.iter().map(|r| r.name.clone()).collect();
        return Ok(analysis);
    }

    // Attempt to detect shell sync script
    if path.extension().is_some_and(|ext| ext == "sh") || content.contains("#!/bin/") {
        analysis.detected_kind = LegacySetupKind::ShellScript;
        extract_shell_script_metadata(&content, &mut analysis);
        return Ok(analysis);
    }

    analysis
        .warnings
        .push("File is neither a valid snapshot JSON nor a recognized sync script".into());
    Ok(analysis)
}

fn scan_directory(dir: &Path) -> Result<LegacySetupAnalysis, io::Error> {
    let mut analysis = LegacySetupAnalysis::new(dir.to_path_buf(), LegacySetupKind::Unknown);

    // 1. Check for .release-desk/data.json (ArtCraft convention)
    let dot_release_desk = dir.join(".release-desk/data.json");
    if dot_release_desk.is_file() {
        if let Ok(content) = fs::read_to_string(&dot_release_desk) {
            if let Ok(snapshot) = parse_snapshot(&content) {
                analysis.detected_kind = LegacySetupKind::ArtCraftWorkspace;
                analysis.discovered_org = Some(snapshot.org);
                analysis.repositories = snapshot.repos.iter().map(|r| r.name.clone()).collect();
                find_installer_assets(dir, &mut analysis);
                return Ok(analysis);
            }
        }
    }

    // 2. Check for root data.json
    let root_data_json = dir.join("data.json");
    if root_data_json.is_file() {
        if let Ok(content) = fs::read_to_string(&root_data_json) {
            if let Ok(snapshot) = parse_snapshot(&content) {
                analysis.detected_kind = LegacySetupKind::SnapshotFile;
                analysis.discovered_org = Some(snapshot.org);
                analysis.repositories = snapshot.repos.iter().map(|r| r.name.clone()).collect();
                find_installer_assets(dir, &mut analysis);
                return Ok(analysis);
            }
        }
    }

    // 3. Check for shell scripts
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "sh") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if content.contains("storytold") || content.contains("data.json") {
                        analysis.detected_kind = LegacySetupKind::ShellScript;
                        extract_shell_script_metadata(&content, &mut analysis);
                        find_installer_assets(dir, &mut analysis);
                        return Ok(analysis);
                    }
                }
            }
        }
    }

    // 4. Check for git clone directories
    let mut git_repos = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(".git").exists() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    git_repos.push(name.to_string());
                }
            }
        }
    }

    if !git_repos.is_empty() {
        git_repos.sort();
        analysis.detected_kind = LegacySetupKind::CloneDirectory;
        analysis.repositories = git_repos;
        find_installer_assets(dir, &mut analysis);
        return Ok(analysis);
    }

    analysis.warnings.push("Directory contains no recognized .release-desk/data.json, sync scripts, or git repositories".into());
    Ok(analysis)
}

fn extract_shell_script_metadata(script: &str, analysis: &mut LegacySetupAnalysis) {
    for line in script.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("ORG=") || trimmed.starts_with("ORGANIZATION=") {
            let org = trimmed
                .split('=')
                .nth(1)
                .unwrap_or_default()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !org.is_empty() {
                analysis.discovered_org = Some(org);
            }
        } else if trimmed.starts_with("REPOS=") {
            let repos_str = trimmed
                .split('=')
                .nth(1)
                .unwrap_or_default()
                .trim_matches('"')
                .trim_matches('\'');
            for r in repos_str.split_whitespace() {
                if !r.is_empty() && !analysis.repositories.contains(&r.to_string()) {
                    analysis.repositories.push(r.to_string());
                }
            }
        }
    }
}

fn find_installer_assets(dir: &Path, analysis: &mut LegacySetupAnalysis) {
    let candidate_dirs = [
        dir.to_path_buf(),
        dir.join("installers"),
        dir.join("releases"),
        dir.join(".release-desk/installers"),
    ];

    for candidate in candidate_dirs {
        if !candidate.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&candidate) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        let lower = ext.to_ascii_lowercase();
                        if matches!(lower.as_str(), "dmg" | "pkg" | "zip" | "tar.gz") {
                            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                if !analysis.installer_assets.contains(&name.to_string()) {
                                    analysis.installer_assets.push(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    analysis.installer_assets.sort();
}

/// Computes a deterministic SHA-256 fingerprint of all file paths and contents under a root.
/// Used to verify that legacy workspaces remain completely unaltered before and after migration.
pub fn compute_source_fingerprint(root: &Path) -> Result<String, io::Error> {
    let mut files = Vec::new();
    collect_files_recursive(root, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(&file);
        hasher.update(relative.to_string_lossy().as_bytes());

        let mut f = fs::File::open(&file)?;
        let mut buffer = [0u8; 8192];
        loop {
            let n = f.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), io::Error> {
    if dir.is_file() {
        files.push(dir.to_path_buf());
        return Ok(());
    }
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                files.push(path);
            } else if path.is_dir() {
                collect_files_recursive(&path, files)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_scan_artcraft_style_directory() {
        let temp = tempdir().unwrap();
        let release_desk_dir = temp.path().join(".release-desk");
        fs::create_dir_all(&release_desk_dir).unwrap();

        let json = r#"{"org":"storytold","generated":"2026-10-27T00:00:00Z","repos":[{"name":"engine","url":"https://github.com/storytold/engine","isPrivate":false,"isArchived":false}]}"#;
        fs::write(release_desk_dir.join("data.json"), json).unwrap();

        let installers_dir = temp.path().join("installers");
        fs::create_dir_all(&installers_dir).unwrap();
        fs::write(installers_dir.join("engine-v1.0.dmg"), b"mock dmg").unwrap();

        let fingerprint_before = compute_source_fingerprint(temp.path()).unwrap();

        let analysis = scan_legacy_setup(temp.path()).unwrap();
        assert_eq!(analysis.detected_kind, LegacySetupKind::ArtCraftWorkspace);
        assert_eq!(analysis.discovered_org, Some("storytold".to_string()));
        assert_eq!(analysis.repositories, vec!["engine".to_string()]);
        assert_eq!(
            analysis.installer_assets,
            vec!["engine-v1.0.dmg".to_string()]
        );

        let fingerprint_after = compute_source_fingerprint(temp.path()).unwrap();
        assert_eq!(
            fingerprint_before, fingerprint_after,
            "Scan must not modify source directory"
        );
    }

    #[test]
    fn test_scan_legacy_shell_script() {
        let temp = tempdir().unwrap();
        let script_path = temp.path().join("sync.sh");
        let script = r#"#!/usr/bin/env bash
ORG="custom-org"
REPOS="alpha beta gamma"
echo "Syncing $ORG..."
"#;
        fs::write(&script_path, script).unwrap();

        let analysis = scan_legacy_setup(&script_path).unwrap();
        assert_eq!(analysis.detected_kind, LegacySetupKind::ShellScript);
        assert_eq!(analysis.discovered_org, Some("custom-org".to_string()));
        assert_eq!(
            analysis.repositories,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
    }
}
