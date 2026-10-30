//! Workspace discovery and intelligent importer.
//!
//! Scans a workspace project directory for existing metadata snapshots (`.release-desk/data.json`
//! or `data.json`) and local Git checkouts to automatically detect organizations, avatars,
//! repositories, version notes, and release assets.

use crate::workspace_sources::{SourceRepository, WorkspaceSource};
use release_desk_core::app_catalog::find_known_app;
use release_desk_store::workspace_releases::WorkspaceRelease;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A single discovered repository candidate for import.
#[derive(Clone, Debug)]
pub struct DiscoveredRepoItem {
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub primary_language: Option<String>,
    pub default_branch: String,
    pub clone_url: String,
    pub is_private: bool,
    pub selected: bool,
    pub latest_version: Option<String>,
    pub release_notes: Option<String>,
    pub cached_releases: Vec<WorkspaceRelease>,
}

/// Discovered workspace structure ready for confirmation.
#[derive(Clone, Debug)]
pub struct DiscoveredWorkspace {
    pub source_handle: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub provider: String,
    pub bio: Option<String>,
    pub detected_from_data_json: bool,
    pub snapshot_path: Option<PathBuf>,
    pub repositories: Vec<DiscoveredRepoItem>,
}

/// Discover project workspace metadata and repositories from on-disk data.json or Git directories.
pub fn discover_workspace(project_dir: &Path) -> Option<DiscoveredWorkspace> {
    // 1. Check for .release-desk/data.json or data.json
    let dot_rd = project_dir.join(".release-desk").join("data.json");
    let root_data = project_dir.join("data.json");
    let snapshot_candidate = if dot_rd.is_file() {
        Some(dot_rd)
    } else if root_data.is_file() {
        Some(root_data)
    } else {
        None
    };

    if let Some(snapshot_file) = snapshot_candidate {
        if let Ok(content) = std::fs::read_to_string(&snapshot_file) {
            if let Ok(snapshot) = release_desk_core::parse_snapshot(&content) {
                let org = if !snapshot.org.trim().is_empty() {
                    snapshot.org.trim().to_string()
                } else {
                    snapshot
                        .repos
                        .iter()
                        .find_map(|r| {
                            if let Some(pos) = r.url.find("github.com/") {
                                let path = r.url[pos + 11..].trim_end_matches(".git");
                                path.split_once('/').map(|(o, _)| o.to_string())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| {
                            project_dir
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "workspace".to_string())
                        })
                };

                let avatar_url = Some(format!("https://github.com/{org}.png"));
                let mut repos = Vec::new();

                for r in snapshot.repos {
                    let known = find_known_app(&r.name);
                    let display_desc = r
                        .description
                        .clone()
                        .filter(|d| !d.is_empty())
                        .or_else(|| known.as_ref().map(|k| k.description.clone()));

                    let primary_language = r.primary_language.map(|l| l.name);

                    let default_branch = r
                        .default_branch_ref
                        .map(|b| b.name.trim_start_matches("refs/heads/").to_string())
                        .unwrap_or_else(|| "main".to_string());

                    let clone_url = if !r.url.is_empty() {
                        if r.url.ends_with(".git") {
                            r.url.clone()
                        } else {
                            format!("{}.git", r.url)
                        }
                    } else {
                        format!("https://github.com/{org}/{}.git", r.name)
                    };

                    let full_name = format!("{org}/{}", r.name);

                    // Extract releases if present
                    let mut cached_releases = Vec::new();
                    let mut latest_version = None;
                    let mut release_notes = None;

                    if let Some(rc) = r.releases {
                        for rel in rc.nodes {
                            if latest_version.is_none() && !rel.is_draft {
                                latest_version = Some(rel.tag_name.clone());
                                release_notes = rel.description.clone();
                            }
                            cached_releases.push(WorkspaceRelease::from(rel));
                        }
                    }

                    repos.push(DiscoveredRepoItem {
                        name: r.name,
                        full_name,
                        description: display_desc,
                        primary_language,
                        default_branch,
                        clone_url,
                        is_private: r.is_private,
                        selected: true,
                        latest_version,
                        release_notes,
                        cached_releases,
                    });
                }

                if !repos.is_empty() {
                    return Some(DiscoveredWorkspace {
                        source_handle: org.clone(),
                        display_name: Some(org),
                        avatar_url,
                        provider: "github".to_string(),
                        bio: Some(format!(
                            "Discovered {} repositories from {} snapshot",
                            repos.len(),
                            snapshot_file
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        )),
                        detected_from_data_json: true,
                        snapshot_path: Some(snapshot_file),
                        repositories: repos,
                    });
                }
            }
        }
    }

    // 2. Fallback: inspect on-disk Git directories and remotes
    let mut discovered_repos = Vec::new();
    let mut detected_owners: HashMap<String, usize> = HashMap::new();

    let inspect_dir = |dir: &Path| -> Option<(DiscoveredRepoItem, Option<String>)> {
        let git_dir = dir.join(".git");
        if !git_dir.exists() {
            return None;
        }

        let name = dir.file_name()?.to_string_lossy().to_string();
        if name.is_empty() {
            return None;
        }

        let default_branch = std::fs::read_to_string(git_dir.join("HEAD"))
            .ok()
            .and_then(|head| {
                head.trim()
                    .strip_prefix("ref: refs/heads/")
                    .map(|b| b.to_string())
            })
            .unwrap_or_else(|| "main".to_string());

        let mut clone_url = String::new();
        let mut owner_detected = None;

        if let Ok(config) = std::fs::read_to_string(git_dir.join("config")) {
            let mut in_remote_origin = false;
            for line in config.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    in_remote_origin = trimmed.contains("remote \"origin\"");
                } else if in_remote_origin && trimmed.starts_with("url =") {
                    if let Some((_, url)) = trimmed.split_once('=') {
                        clone_url = url.trim().to_string();
                        break;
                    }
                }
            }
            if clone_url.is_empty() {
                for line in config.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("url =") {
                        if let Some((_, url)) = trimmed.split_once('=') {
                            clone_url = url.trim().to_string();
                            break;
                        }
                    }
                }
            }
        }

        let full_name = if !clone_url.is_empty() {
            if let Some(pos) = clone_url.find("github.com/") {
                let path = clone_url[pos + 11..].trim_end_matches(".git");
                if let Some((o, _)) = path.split_once('/') {
                    owner_detected = Some(o.to_string());
                }
                path.to_string()
            } else if let Some(pos) = clone_url.find("github.com:") {
                let path = clone_url[pos + 11..].trim_end_matches(".git");
                if let Some((o, _)) = path.split_once('/') {
                    owner_detected = Some(o.to_string());
                }
                path.to_string()
            } else if let Some(pos) = clone_url.find("gitlab.com/") {
                let path = clone_url[pos + 11..].trim_end_matches(".git");
                if let Some((o, _)) = path.split_once('/') {
                    owner_detected = Some(o.to_string());
                }
                path.to_string()
            } else {
                format!("local/{name}")
            }
        } else {
            format!("local/{name}")
        };

        let known = find_known_app(&name);
        let description = known
            .as_ref()
            .map(|k| k.description.clone())
            .or_else(|| Some("Imported local repository".to_string()));

        let primary_language = if dir.join("Cargo.toml").exists() {
            Some("Rust".to_string())
        } else if dir.join("pubspec.yaml").exists() {
            Some("Dart".to_string())
        } else if dir.join("package.json").exists() {
            Some("TypeScript".to_string())
        } else if dir.join("pyproject.toml").exists() || dir.join("requirements.txt").exists() {
            Some("Python".to_string())
        } else if dir.join("go.mod").exists() {
            Some("Go".to_string())
        } else if dir.join("Package.swift").exists() {
            Some("Swift".to_string())
        } else {
            None
        };

        let mut latest_version = None;
        let tags_dir = git_dir.join("refs").join("tags");
        if let Ok(entries) = std::fs::read_dir(tags_dir) {
            let mut tags: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            tags.sort();
            latest_version = tags.pop();
        }

        Some((
            DiscoveredRepoItem {
                name,
                full_name,
                description,
                primary_language,
                default_branch,
                clone_url,
                is_private: false,
                selected: true,
                latest_version,
                release_notes: None,
                cached_releases: Vec::new(),
            },
            owner_detected,
        ))
    };

    if let Some((root_repo, maybe_owner)) = inspect_dir(project_dir) {
        if let Some(owner) = maybe_owner {
            *detected_owners.entry(owner).or_insert(0) += 1;
        }
        discovered_repos.push(root_repo);
    }

    if let Ok(entries) = std::fs::read_dir(project_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !path.file_name().is_some_and(|n| n == ".git") {
                if let Some((repo, maybe_owner)) = inspect_dir(&path) {
                    if !discovered_repos.iter().any(|r| r.name == repo.name) {
                        if let Some(owner) = maybe_owner {
                            *detected_owners.entry(owner).or_insert(0) += 1;
                        }
                        discovered_repos.push(repo);
                    }
                } else if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_dir() && !sub_path.file_name().is_some_and(|n| n == ".git") {
                            if let Some((repo, maybe_owner)) = inspect_dir(&sub_path) {
                                if !discovered_repos.iter().any(|r| r.name == repo.name) {
                                    if let Some(owner) = maybe_owner {
                                        *detected_owners.entry(owner).or_insert(0) += 1;
                                    }
                                    discovered_repos.push(repo);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if discovered_repos.is_empty() {
        return None;
    }

    // Use the discovered owner when available; otherwise retain a local source.
    let dominant_owner = detected_owners
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(owner, _)| owner)
        .unwrap_or_else(|| "local".to_string());

    let avatar_url =
        (dominant_owner != "local").then(|| format!("https://github.com/{dominant_owner}.png"));

    let provider = if dominant_owner == "local" {
        "local"
    } else {
        "github"
    };
    Some(DiscoveredWorkspace {
        source_handle: dominant_owner.clone(),
        display_name: Some(dominant_owner),
        avatar_url,
        provider: provider.to_string(),
        bio: Some(format!(
            "Discovered {} repositories from disk checkouts",
            discovered_repos.len()
        )),
        detected_from_data_json: false,
        snapshot_path: None,
        repositories: discovered_repos,
    })
}

/// Convert confirmed discovered workspace into a WorkspaceSource and preloaded releases.
pub fn build_confirmed_source(
    discovered: &DiscoveredWorkspace,
    confirmed_handle: &str,
) -> (WorkspaceSource, HashMap<String, Vec<WorkspaceRelease>>) {
    let clean_handle = confirmed_handle.trim().trim_start_matches('@');
    let effective_handle = if clean_handle.is_empty() {
        &discovered.source_handle
    } else {
        clean_handle
    };

    let avatar_url = (discovered.provider == "github" && effective_handle != "local")
        .then(|| format!("https://github.com/{effective_handle}.png"));
    let mut initial_releases = HashMap::new();

    let repositories: Vec<SourceRepository> = discovered
        .repositories
        .iter()
        .filter(|r| r.selected)
        .map(|r| {
            let full_name = format!("{effective_handle}/{}", r.name);
            let clone_url = if r.clone_url.is_empty()
                && discovered.provider == "github"
                && effective_handle != "local"
            {
                format!("https://github.com/{effective_handle}/{}.git", r.name)
            } else {
                r.clone_url.clone()
            };

            if !r.cached_releases.is_empty() {
                initial_releases.insert(r.name.clone(), r.cached_releases.clone());
            }

            SourceRepository {
                name: r.name.clone(),
                full_name,
                description: r.description.clone(),
                stars: 0,
                forks: 0,
                primary_language: r.primary_language.clone(),
                is_private: r.is_private,
                is_fork: false,
                clone_url,
                default_branch: r.default_branch.clone(),
                selected: true,
            }
        })
        .collect();

    let source = WorkspaceSource {
        handle: effective_handle.to_string(),
        display_name: discovered
            .display_name
            .clone()
            .or_else(|| Some(effective_handle.to_string())),
        avatar_url,
        bio: discovered.bio.clone(),
        provider: discovered.provider.clone(),
        repositories,
    };

    (source, initial_releases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_repository_without_remote_does_not_invent_an_owner_or_url() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("my-repo/.git")).unwrap();
        let discovered = discover_workspace(temp.path()).expect("local repository");
        assert_eq!(discovered.source_handle, "local");
        assert_eq!(discovered.provider, "local");
        assert!(discovered.avatar_url.is_none());
        let (source, _) = build_confirmed_source(&discovered, "local");
        assert!(source.avatar_url.is_none());
        assert!(source.repositories[0].clone_url.is_empty());
    }

    #[test]
    fn discover_from_json_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let dot_rd = temp.path().join(".release-desk");
        std::fs::create_dir_all(&dot_rd).unwrap();
        let json_content = r#"{
            "org": "storytold",
            "generated": "2026-10-27T00:00:00Z",
            "repos": [
                {
                    "name": "PixelCraft",
                    "description": "Image editor",
                    "url": "https://github.com/storytold/PixelCraft",
                    "isPrivate": false,
                    "isArchived": false,
                    "releases": {
                        "nodes": [
                            {
                                "name": "PixelCraft 0.10.0",
                                "tagName": "v0.10.0",
                                "description": "Latest release",
                                "isDraft": false,
                                "isPrerelease": false,
                                "createdAt": "2026-10-18T12:00:00Z",
                                "url": "https://github.com/storytold/PixelCraft/releases/tag/v0.10.0",
                                "releaseAssets": {
                                    "nodes": [
                                        {
                                            "name": "PixelCraft-0.10.0-aarch64.dmg",
                                            "downloadUrl": "https://github.com/storytold/PixelCraft/releases/download/v0.10.0/PixelCraft.dmg",
                                            "size": 192000000
                                        }
                                    ]
                                }
                            }
                        ]
                    }
                }
            ]
        }"#;
        std::fs::write(dot_rd.join("data.json"), json_content).unwrap();

        let disc = discover_workspace(temp.path()).expect("discovered workspace");
        assert_eq!(disc.source_handle, "storytold");
        assert!(disc.detected_from_data_json);
        assert_eq!(disc.repositories.len(), 1);
        assert_eq!(disc.repositories[0].name, "PixelCraft");
        assert_eq!(
            disc.repositories[0].latest_version.as_deref(),
            Some("v0.10.0")
        );
        assert_eq!(disc.repositories[0].cached_releases.len(), 1);

        let (source, releases) = build_confirmed_source(&disc, "storytold");
        assert_eq!(source.handle, "storytold");
        assert_eq!(source.repositories.len(), 1);
        assert_eq!(releases.get("PixelCraft").map(|r| r.len()), Some(1));
    }
}
