//! Reconcile project snapshot backup use cases.
//!
//! Detects mismatches between the database and the project clone folder's `.release-desk/data.json`
//! backup snapshot, and provides bidirectional synchronization and disaster recovery.

use crate::use_case::discovery::{TrackingStatus, TrackingStore};
use release_desk_core::error::DomainError;
use release_desk_core::port::{ProjectBackupPort, ProjectStore};
use release_desk_core::project::ProjectId;
use release_desk_core::snapshot::{MismatchReport, Repository, Snapshot};
use std::path::PathBuf;
use std::sync::Arc;

/// Use case for reconciling project folder backup snapshots with SQLite.
pub struct ReconcileProjectSnapshotUseCase {
    project_store: Arc<dyn ProjectStore>,
    tracking_store: Arc<dyn TrackingStore>,
    backup_port: Arc<dyn ProjectBackupPort>,
}

impl ReconcileProjectSnapshotUseCase {
    pub fn new(
        project_store: Arc<dyn ProjectStore>,
        tracking_store: Arc<dyn TrackingStore>,
        backup_port: Arc<dyn ProjectBackupPort>,
    ) -> Self {
        Self {
            project_store,
            tracking_store,
            backup_port,
        }
    }

    /// Check for discrepancies between the SQLite database and the project's local `data.json`.
    pub fn check_mismatch(&self, project_id: ProjectId) -> Result<MismatchReport, DomainError> {
        let project =
            self.project_store
                .get_project(project_id)?
                .ok_or_else(|| DomainError::NotFound {
                    entity: "Project",
                    id: project_id.as_u128().to_string(),
                })?;

        let clone_location = project
            .clone_location
            .ok_or_else(|| DomainError::PortFailure {
                port: "Reconcile",
                message: format!(
                    "Project '{}' does not have a clone location configured",
                    project.name
                ),
            })?;

        let db_records = self.tracking_store.list_project_repositories(project_id)?;
        let db_repos: Vec<Repository> = db_records
            .into_iter()
            .map(|r| Repository {
                name: r.repo_name,
                description: None,
                url: String::new(),
                is_private: false,
                is_archived: false,
                pushed_at: None,
                primary_language: None,
                default_branch_ref: None,
                releases: None,
            })
            .collect();

        self.backup_port.detect_mismatch(&clone_location, &db_repos)
    }

    /// Restore or import missing repositories from the project folder's `data.json` into the database.
    pub fn restore_database_from_backup(
        &self,
        project_id: ProjectId,
    ) -> Result<usize, DomainError> {
        let project =
            self.project_store
                .get_project(project_id)?
                .ok_or_else(|| DomainError::NotFound {
                    entity: "Project",
                    id: project_id.as_u128().to_string(),
                })?;

        let clone_location = project
            .clone_location
            .ok_or_else(|| DomainError::PortFailure {
                port: "Reconcile",
                message: format!(
                    "Project '{}' does not have a clone location configured",
                    project.name
                ),
            })?;

        let snapshot = self
            .backup_port
            .read_backup_snapshot(&clone_location)?
            .ok_or_else(|| DomainError::NotFound {
                entity: "SnapshotBackup",
                id: clone_location.display().to_string(),
            })?;

        let existing = self.tracking_store.list_project_repositories(project_id)?;
        let existing_names: std::collections::HashSet<String> =
            existing.into_iter().map(|r| r.repo_name).collect();

        let mut restored_count = 0;
        for (i, repo) in snapshot.repos.into_iter().enumerate() {
            if !existing_names.contains(&repo.name) {
                // Generate a deterministic or fallback repository ID
                let pseudo_id = release_desk_core::repository::GitHubRepositoryId::from_u64(
                    (100_000 + i) as u64,
                );
                self.tracking_store.register_repository(
                    project_id,
                    pseudo_id,
                    &repo.name,
                    TrackingStatus::Inbox,
                )?;
                restored_count += 1;
            }
        }

        Ok(restored_count)
    }

    /// Export the current database state to the project folder's `.release-desk/data.json` backup snapshot.
    pub fn sync_backup_from_database(&self, project_id: ProjectId) -> Result<PathBuf, DomainError> {
        let project =
            self.project_store
                .get_project(project_id)?
                .ok_or_else(|| DomainError::NotFound {
                    entity: "Project",
                    id: project_id.as_u128().to_string(),
                })?;

        let clone_location = project
            .clone_location
            .ok_or_else(|| DomainError::PortFailure {
                port: "Reconcile",
                message: format!(
                    "Project '{}' does not have a clone location configured",
                    project.name
                ),
            })?;

        let db_records = self.tracking_store.list_project_repositories(project_id)?;
        let repos: Vec<Repository> = db_records
            .into_iter()
            .map(|r| Repository {
                name: r.repo_name,
                description: None,
                url: String::new(),
                is_private: false,
                is_archived: false,
                pushed_at: None,
                primary_language: None,
                default_branch_ref: None,
                releases: None,
            })
            .collect();

        let snapshot = Snapshot {
            org: project.name.as_str().to_string(),
            generated: "2026-10-27T00:00:00Z".to_string(),
            repos,
        };

        self.backup_port
            .write_backup_snapshot(&clone_location, &snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_case::discovery::InMemoryTrackingStore;
    use release_desk_core::project::{ProjectIdentity, ProjectName};
    use std::sync::Mutex;

    struct InMemoryProjectStore(Mutex<Vec<ProjectIdentity>>);

    impl ProjectStore for InMemoryProjectStore {
        fn list_projects(&self) -> Result<Vec<ProjectIdentity>, DomainError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn get_project(&self, id: ProjectId) -> Result<Option<ProjectIdentity>, DomainError> {
            Ok(self.0.lock().unwrap().iter().find(|p| p.id == id).cloned())
        }

        fn save_project(&self, project: &ProjectIdentity) -> Result<(), DomainError> {
            self.0.lock().unwrap().push(project.clone());
            Ok(())
        }

        fn delete_project(&self, id: ProjectId) -> Result<(), DomainError> {
            self.0.lock().unwrap().retain(|p| p.id != id);
            Ok(())
        }
    }

    struct MockBackupPort {
        snapshot: Mutex<Option<Snapshot>>,
    }

    impl MockBackupPort {
        fn new(snap: Option<Snapshot>) -> Self {
            Self {
                snapshot: Mutex::new(snap),
            }
        }
    }

    impl ProjectBackupPort for MockBackupPort {
        fn write_backup_snapshot(
            &self,
            project_root: &std::path::Path,
            snapshot: &Snapshot,
        ) -> Result<PathBuf, DomainError> {
            *self.snapshot.lock().unwrap() = Some(snapshot.clone());
            Ok(project_root.join(".release-desk/data.json"))
        }

        fn read_backup_snapshot(
            &self,
            _project_root: &std::path::Path,
        ) -> Result<Option<Snapshot>, DomainError> {
            Ok(self.snapshot.lock().unwrap().clone())
        }

        fn detect_mismatch(
            &self,
            project_root: &std::path::Path,
            db_repos: &[Repository],
        ) -> Result<MismatchReport, DomainError> {
            let snap = self.snapshot.lock().unwrap().clone();
            let Some(s) = snap else {
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

            let disk_names: std::collections::HashSet<String> =
                s.repos.into_iter().map(|r| r.name).collect();
            let db_names: std::collections::HashSet<String> =
                db_repos.iter().map(|r| r.name.clone()).collect();

            for name in &disk_names {
                if db_names.contains(name) {
                    report.matched_repositories.push(name.clone());
                } else {
                    report.missing_in_database.push(name.clone());
                }
            }
            for name in &db_names {
                if !disk_names.contains(name) {
                    report.missing_on_disk.push(name.clone());
                }
            }

            report.matched_repositories.sort();
            report.missing_in_database.sort();
            report.missing_on_disk.sort();

            Ok(report)
        }
    }

    #[test]
    fn reconcile_flow_detects_mismatch_and_restores() {
        let project_id = ProjectId::from_u128(500);
        let name = ProjectName::new("Reconcile Test").unwrap();
        let clone_dir = PathBuf::from("/tmp/reconcile-test-dir");
        let project = ProjectIdentity::with_clone_location(project_id, name, Some(clone_dir));

        let project_store = Arc::new(InMemoryProjectStore(Mutex::new(vec![project])));
        let tracking_store = Arc::new(InMemoryTrackingStore::default());

        let initial_snapshot = Snapshot {
            org: "test-org".into(),
            generated: "".into(),
            repos: vec![Repository {
                name: "backup-repo-1".into(),
                description: None,
                url: "".into(),
                is_private: false,
                is_archived: false,
                pushed_at: None,
                primary_language: None,
                default_branch_ref: None,
                releases: None,
            }],
        };

        let backup_port = Arc::new(MockBackupPort::new(Some(initial_snapshot)));
        let reconcile_uc = ReconcileProjectSnapshotUseCase::new(
            project_store,
            tracking_store.clone(),
            backup_port,
        );

        // 1. Check mismatch: backup-repo-1 is in backup snapshot, but missing in DB
        let report = reconcile_uc.check_mismatch(project_id).unwrap();
        assert!(!report.is_synchronized());
        assert_eq!(report.missing_in_database, vec!["backup-repo-1"]);

        // 2. Restore DB from backup
        let restored = reconcile_uc
            .restore_database_from_backup(project_id)
            .unwrap();
        assert_eq!(restored, 1);

        // 3. Now verify DB has it
        let repos = tracking_store
            .list_project_repositories(project_id)
            .unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].repo_name, "backup-repo-1");

        // 4. Check mismatch again: now synchronized!
        let report2 = reconcile_uc.check_mismatch(project_id).unwrap();
        assert!(report2.is_synchronized());
        assert_eq!(report2.matched_repositories, vec!["backup-repo-1"]);
    }
}
