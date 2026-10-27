//! Project local `.release-desk/data.json` backup snapshot and reconciliation engine.
//!
//! Automatically writes and maintains a `data.json` snapshot inside the project's
//! clone location, providing local backup redundancy and detecting database-disk mismatches.

use crate::boundary::FORBIDDEN_ARTCRAFT_ROOT;
use release_desk_core::error::DomainError;
use release_desk_core::port::ProjectBackupPort;
pub use release_desk_core::snapshot::{MismatchReport, RevisionMismatch};
use release_desk_core::snapshot::{Repository, Snapshot, parse_snapshot};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Manager for reading, writing, and reconciling a project folder's local `.release-desk/data.json`.
#[derive(Clone, Debug, Default)]
pub struct ProjectSnapshotManager;

impl ProjectSnapshotManager {
    /// Create a new project snapshot manager.
    pub fn new() -> Self {
        Self
    }

    /// The path to the local backup snapshot file (`<project_root>/.release-desk/data.json`).
    pub fn snapshot_path(project_root: &Path) -> PathBuf {
        project_root.join(".release-desk/data.json")
    }

    /// Ensure the target project root is not inside the forbidden ArtCraft legacy path.
    fn validate_boundary(root: &Path) -> Result<(), DomainError> {
        let forbidden = Path::new(FORBIDDEN_ARTCRAFT_ROOT);
        if root == forbidden || root.starts_with(forbidden) {
            return Err(DomainError::PortFailure {
                port: "ProjectBackup",
                message: format!(
                    "SAFETY VIOLATION: project clone location cannot be inside ArtCraft: {}",
                    root.display()
                ),
            });
        }
        Ok(())
    }
}

impl ProjectBackupPort for ProjectSnapshotManager {
    /// Write or update the local `data.json` backup snapshot inside the project directory.
    fn write_backup_snapshot(
        &self,
        project_root: &Path,
        snapshot: &Snapshot,
    ) -> Result<PathBuf, DomainError> {
        Self::validate_boundary(project_root)?;

        let snapshot_file = Self::snapshot_path(project_root);
        let parent = snapshot_file
            .parent()
            .ok_or_else(|| DomainError::PortFailure {
                port: "ProjectBackup",
                message: format!("invalid snapshot path {}", snapshot_file.display()),
            })?;

        fs::create_dir_all(parent).map_err(|e| DomainError::PortFailure {
            port: "ProjectBackup",
            message: format!("cannot create .release-desk directory: {e}"),
        })?;

        let json =
            serde_json::to_string_pretty(snapshot).map_err(|e| DomainError::PortFailure {
                port: "ProjectBackup",
                message: format!("cannot serialize snapshot: {e}"),
            })?;

        // Write atomically via temporary file
        let mut temp =
            tempfile::NamedTempFile::new_in(parent).map_err(|e| DomainError::PortFailure {
                port: "ProjectBackup",
                message: format!("cannot create temporary snapshot file: {e}"),
            })?;

        temp.write_all(json.as_bytes())
            .map_err(|e| DomainError::PortFailure {
                port: "ProjectBackup",
                message: format!("cannot write snapshot bytes: {e}"),
            })?;
        temp.flush().map_err(|e| DomainError::PortFailure {
            port: "ProjectBackup",
            message: format!("cannot flush snapshot: {e}"),
        })?;

        temp.persist(&snapshot_file)
            .map_err(|e| DomainError::PortFailure {
                port: "ProjectBackup",
                message: format!("cannot persist backup snapshot: {}", e.error),
            })?;

        Ok(snapshot_file)
    }

    /// Read the local `data.json` backup snapshot if it exists.
    fn read_backup_snapshot(&self, project_root: &Path) -> Result<Option<Snapshot>, DomainError> {
        let path = Self::snapshot_path(project_root);
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).map_err(|e| DomainError::PortFailure {
            port: "ProjectBackup",
            message: format!("cannot read snapshot {}: {e}", path.display()),
        })?;

        let snapshot = parse_snapshot(&content).map_err(|e| DomainError::PortFailure {
            port: "ProjectBackup",
            message: format!("invalid snapshot in {}: {e}", path.display()),
        })?;

        Ok(Some(snapshot))
    }

    /// Compare the local `.release-desk/data.json` backup snapshot against the SQLite database records.
    fn detect_mismatch(
        &self,
        project_root: &Path,
        db_repos: &[Repository],
    ) -> Result<MismatchReport, DomainError> {
        let disk_snapshot = self.read_backup_snapshot(project_root)?;

        let Some(disk) = disk_snapshot else {
            return Ok(MismatchReport {
                project_root: project_root.to_path_buf(),
                disk_snapshot_present: false,
                missing_on_disk: db_repos.iter().map(|r| r.name.clone()).collect(),
                ..Default::default()
            });
        };

        let mut report = MismatchReport {
            project_root: project_root.to_path_buf(),
            disk_snapshot_present: true,
            ..Default::default()
        };

        let disk_repo_map: HashMap<&str, &Repository> =
            disk.repos.iter().map(|r| (r.name.as_str(), r)).collect();
        let db_repo_map: HashMap<&str, &Repository> =
            db_repos.iter().map(|r| (r.name.as_str(), r)).collect();

        let all_names: HashSet<&str> = disk_repo_map
            .keys()
            .chain(db_repo_map.keys())
            .copied()
            .collect();

        for name in all_names {
            match (disk_repo_map.get(name), db_repo_map.get(name)) {
                (Some(disk_r), Some(db_r)) => {
                    report.matched_repositories.push(name.to_string());

                    // Check head revision
                    let disk_rev = disk_r
                        .default_branch_ref
                        .as_ref()
                        .map(|b| b.target.oid.clone());
                    let db_rev = db_r
                        .default_branch_ref
                        .as_ref()
                        .map(|b| b.target.oid.clone());

                    if disk_rev != db_rev {
                        report.revision_mismatches.push(RevisionMismatch {
                            repository: name.to_string(),
                            disk_revision: disk_rev,
                            db_revision: db_rev,
                        });
                    }
                }
                (Some(_), None) => {
                    report.missing_in_database.push(name.to_string());
                }
                (None, Some(_)) => {
                    report.missing_on_disk.push(name.to_string());
                }
                (None, None) => {}
            }
        }

        report.matched_repositories.sort();
        report.missing_in_database.sort();
        report.missing_on_disk.sort();
        report
            .revision_mismatches
            .sort_by(|a, b| a.repository.cmp(&b.repository));

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use release_desk_core::git::{BranchRef, Revision};

    #[test]
    fn rejects_artcraft_root_for_backup() {
        let manager = ProjectSnapshotManager::new();
        let snapshot = Snapshot {
            org: "org".into(),
            generated: "".into(),
            repos: vec![],
        };
        assert!(
            manager
                .write_backup_snapshot(Path::new(FORBIDDEN_ARTCRAFT_ROOT), &snapshot)
                .is_err()
        );
        assert!(
            manager
                .write_backup_snapshot(
                    &Path::new(FORBIDDEN_ARTCRAFT_ROOT).join("subdir"),
                    &snapshot
                )
                .is_err()
        );
    }

    #[test]
    fn write_and_read_backup_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let manager = ProjectSnapshotManager::new();

        let snapshot = Snapshot {
            org: "storytold".into(),
            generated: "2026-10-27T00:00:00Z".into(),
            repos: vec![Repository {
                name: "app-core".into(),
                description: None,
                url: "https://github.com/storytold/app-core".into(),
                is_private: false,
                is_archived: false,
                pushed_at: None,
                primary_language: None,
                default_branch_ref: Some(BranchRef {
                    name: "main".into(),
                    target: Revision {
                        oid: "commit_hash_123".into(),
                    },
                }),
                releases: None,
            }],
        };

        let written_path = manager
            .write_backup_snapshot(temp.path(), &snapshot)
            .unwrap();
        assert!(written_path.exists());
        assert_eq!(written_path, temp.path().join(".release-desk/data.json"));

        let loaded = manager.read_backup_snapshot(temp.path()).unwrap().unwrap();
        assert_eq!(loaded.org, "storytold");
        assert_eq!(loaded.repos.len(), 1);
        assert_eq!(loaded.repos[0].name, "app-core");
    }

    #[test]
    fn detect_mismatch_and_synchronization() {
        let temp = tempfile::tempdir().unwrap();
        let manager = ProjectSnapshotManager::new();

        let repo1 = Repository {
            name: "repo-alpha".into(),
            description: None,
            url: "".into(),
            is_private: false,
            is_archived: false,
            pushed_at: None,
            primary_language: None,
            default_branch_ref: Some(BranchRef {
                name: "main".into(),
                target: Revision { oid: "rev1".into() },
            }),
            releases: None,
        };

        let snapshot = Snapshot {
            org: "org".into(),
            generated: "".into(),
            repos: vec![repo1.clone()],
        };
        manager
            .write_backup_snapshot(temp.path(), &snapshot)
            .unwrap();

        // 1. In sync
        let report = manager
            .detect_mismatch(temp.path(), &[repo1.clone()])
            .unwrap();
        assert!(report.is_synchronized());
        assert_eq!(report.matched_repositories, vec!["repo-alpha"]);

        // 2. Database has additional repo
        let repo2 = Repository {
            name: "repo-beta".into(),
            description: None,
            url: "".into(),
            is_private: false,
            is_archived: false,
            pushed_at: None,
            primary_language: None,
            default_branch_ref: None,
            releases: None,
        };

        let report_mismatch = manager
            .detect_mismatch(temp.path(), &[repo1, repo2])
            .unwrap();
        assert!(!report_mismatch.is_synchronized());
        assert_eq!(report_mismatch.missing_on_disk, vec!["repo-beta"]);
    }
}
