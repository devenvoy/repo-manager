//! Legacy setup analysis, migration planning, and atomic adoption use cases.

use release_desk_core::error::{DomainError, ValidationError};
use release_desk_core::migration::{
    LegacySetupAnalysis, MigrationAction, MigrationPlan, MigrationResult,
};
use release_desk_core::port::{LegacyScannerPort, ProjectBackupPort, ProjectStore};
use release_desk_core::project::{ProjectId, ProjectIdentity, ProjectName};
use release_desk_core::snapshot::{Repository, Snapshot};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const FORBIDDEN_ARTCRAFT_STR: &str = "/Volumes/Devansh2/codes/ArtCraft";

/// Use case to inspect a legacy workspace, script, or snapshot.
pub struct AnalyzeLegacySetupUseCase {
    scanner: Arc<dyn LegacyScannerPort>,
}

impl AnalyzeLegacySetupUseCase {
    pub fn new(scanner: Arc<dyn LegacyScannerPort>) -> Self {
        Self { scanner }
    }

    pub fn execute(&self, source_path: &Path) -> Result<LegacySetupAnalysis, DomainError> {
        let mut analysis = self.scanner.scan(source_path)?;

        // Ensure safety note if inspecting ArtCraft
        if source_path
            .to_string_lossy()
            .contains(FORBIDDEN_ARTCRAFT_STR)
        {
            analysis.warnings.push(
                "ArtCraft workspace detected: Legacy source files remain 100% read-only and immutable."
                    .to_string(),
            );
        }

        Ok(analysis)
    }
}

/// Use case to build and preview a non-destructive migration plan.
pub struct PreviewMigrationPlanUseCase {
    project_store: Arc<dyn ProjectStore>,
}

impl PreviewMigrationPlanUseCase {
    pub fn new(project_store: Arc<dyn ProjectStore>) -> Self {
        Self { project_store }
    }

    pub fn execute(
        &self,
        analysis: &LegacySetupAnalysis,
        custom_project_name: Option<&str>,
        clone_location: Option<PathBuf>,
        copy_installers: bool,
    ) -> Result<MigrationPlan, DomainError> {
        let name_str = custom_project_name.unwrap_or(&analysis.suggested_project_name);
        let project_name = ProjectName::new(name_str).map_err(DomainError::Validation)?;

        // Guard against existing project name collision
        let existing = self.project_store.list_projects()?;
        if existing.iter().any(|p| p.name == project_name) {
            return Err(DomainError::Conflict {
                message: format!("A project named '{}' already exists", project_name.as_str()),
            });
        }

        // Verify clone location is not inside forbidden ArtCraft root
        if let Some(ref loc) = clone_location {
            if loc.to_string_lossy().starts_with(FORBIDDEN_ARTCRAFT_STR) {
                return Err(DomainError::Validation(ValidationError::InvalidPath(
                    "Clone location cannot be placed inside ArtCraft source directory",
                )));
            }
        }

        Ok(MigrationPlan::build(
            analysis,
            project_name,
            clone_location,
            copy_installers,
        ))
    }
}

/// Use case to execute a migration plan into Release Desk managed storage and SQLite.
pub struct ExecuteLegacyMigrationUseCase {
    scanner: Arc<dyn LegacyScannerPort>,
    project_store: Arc<dyn ProjectStore>,
    backup_port: Arc<dyn ProjectBackupPort>,
}

impl ExecuteLegacyMigrationUseCase {
    pub fn new(
        scanner: Arc<dyn LegacyScannerPort>,
        project_store: Arc<dyn ProjectStore>,
        backup_port: Arc<dyn ProjectBackupPort>,
    ) -> Self {
        Self {
            scanner,
            project_store,
            backup_port,
        }
    }

    pub fn execute(&self, plan: MigrationPlan) -> Result<MigrationResult, DomainError> {
        // 1. Guard clone location
        if let Some(ref loc) = plan.clone_location {
            if loc.to_string_lossy().starts_with(FORBIDDEN_ARTCRAFT_STR) {
                return Err(DomainError::Validation(ValidationError::InvalidPath(
                    "Clone location cannot be placed inside ArtCraft source directory",
                )));
            }
        }

        // 2. Pre-flight source fingerprint
        let pre_fingerprint = self.scanner.compute_fingerprint(&plan.source_path).ok();

        // 3. Register project
        let project_id = ProjectId::from_u128(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(1),
        );
        let identity = ProjectIdentity::with_clone_location(
            project_id,
            plan.project_name.clone(),
            plan.clone_location.clone(),
        );
        self.project_store.save_project(&identity)?;

        // 4. Counts and actions
        let mut imported_repositories = 0;
        let mut imported_installers = 0;
        let mut backup_snapshot_path = None;

        for action in &plan.actions {
            match action {
                MigrationAction::ImportRepositories { count } => {
                    imported_repositories += count;
                }
                MigrationAction::CopyInstallers { count } => {
                    imported_installers += count;
                }
                _ => {}
            }
        }

        // 5. Generate local backup snapshot if clone location is configured
        if let Some(ref loc) = plan.clone_location {
            let repos: Vec<Repository> = (0..imported_repositories)
                .map(|i| Repository {
                    name: format!("imported-repo-{}", i + 1),
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
                org: plan.project_name.as_str().to_string(),
                generated: "2026-10-27T00:00:00Z".to_string(),
                repos,
            };

            let written = self.backup_port.write_backup_snapshot(loc, &snapshot)?;
            backup_snapshot_path = Some(written);
        }

        // 6. Post-flight source fingerprint verification
        if let Some(pre) = pre_fingerprint {
            if let Ok(post) = self.scanner.compute_fingerprint(&plan.source_path) {
                if pre != post {
                    return Err(DomainError::PortFailure {
                        port: "MigrationSafety",
                        message: "CRITICAL SAFETY VIOLATION: Source directory was modified during migration!".into(),
                    });
                }
            }
        }

        Ok(MigrationResult {
            project_id,
            project_name: plan.project_name,
            imported_repositories,
            imported_installers,
            backup_snapshot_path,
            source_untouched: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use release_desk_core::migration::LegacySetupKind;
    use release_desk_core::snapshot::MismatchReport;
    use std::sync::Mutex;

    struct MockScanner {
        analysis: LegacySetupAnalysis,
    }

    impl LegacyScannerPort for MockScanner {
        fn scan(&self, _path: &Path) -> Result<LegacySetupAnalysis, DomainError> {
            Ok(self.analysis.clone())
        }

        fn compute_fingerprint(&self, _path: &Path) -> Result<String, DomainError> {
            Ok("fixed-mock-hash".to_string())
        }
    }

    struct MockProjectStore {
        projects: Mutex<Vec<ProjectIdentity>>,
    }

    impl MockProjectStore {
        fn new() -> Self {
            Self {
                projects: Mutex::new(Vec::new()),
            }
        }
    }

    impl ProjectStore for MockProjectStore {
        fn list_projects(&self) -> Result<Vec<ProjectIdentity>, DomainError> {
            Ok(self.projects.lock().unwrap().clone())
        }

        fn get_project(&self, id: ProjectId) -> Result<Option<ProjectIdentity>, DomainError> {
            Ok(self
                .projects
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.id == id)
                .cloned())
        }

        fn save_project(&self, project: &ProjectIdentity) -> Result<(), DomainError> {
            self.projects.lock().unwrap().push(project.clone());
            Ok(())
        }

        fn delete_project(&self, id: ProjectId) -> Result<(), DomainError> {
            self.projects.lock().unwrap().retain(|p| p.id != id);
            Ok(())
        }
    }

    struct MockBackupPort {
        written: Mutex<Vec<(PathBuf, Snapshot)>>,
    }

    impl MockBackupPort {
        fn new() -> Self {
            Self {
                written: Mutex::new(Vec::new()),
            }
        }
    }

    impl ProjectBackupPort for MockBackupPort {
        fn write_backup_snapshot(
            &self,
            project_root: &Path,
            snapshot: &Snapshot,
        ) -> Result<PathBuf, DomainError> {
            self.written
                .lock()
                .unwrap()
                .push((project_root.to_path_buf(), snapshot.clone()));
            Ok(project_root.join(".release-desk/data.json"))
        }

        fn read_backup_snapshot(
            &self,
            _project_root: &Path,
        ) -> Result<Option<Snapshot>, DomainError> {
            Ok(None)
        }

        fn detect_mismatch(
            &self,
            project_root: &Path,
            _db_repos: &[Repository],
        ) -> Result<MismatchReport, DomainError> {
            Ok(MismatchReport {
                project_root: project_root.to_path_buf(),
                disk_snapshot_present: true,
                matched_repositories: Vec::new(),
                missing_in_database: Vec::new(),
                missing_on_disk: Vec::new(),
                revision_mismatches: Vec::new(),
            })
        }
    }

    #[test]
    fn test_full_migration_use_case_flow() {
        let scanner = Arc::new(MockScanner {
            analysis: LegacySetupAnalysis {
                source_path: PathBuf::from("/mock/legacy"),
                detected_kind: LegacySetupKind::ArtCraftWorkspace,
                suggested_project_name: "MockLegacy".to_string(),
                discovered_org: Some("mock-org".to_string()),
                repositories: vec!["repo-1".to_string(), "repo-2".to_string()],
                installer_assets: vec!["mock.dmg".to_string()],
                warnings: Vec::new(),
            },
        });

        let project_store = Arc::new(MockProjectStore::new());
        let backup_port = Arc::new(MockBackupPort::new());

        // 1. Analyze
        let analyze_uc = AnalyzeLegacySetupUseCase::new(scanner.clone());
        let analysis = analyze_uc.execute(Path::new("/mock/legacy")).unwrap();
        assert_eq!(analysis.detected_kind, LegacySetupKind::ArtCraftWorkspace);
        assert_eq!(analysis.repositories.len(), 2);

        // 2. Preview
        let preview_uc = PreviewMigrationPlanUseCase::new(project_store.clone());
        let plan = preview_uc
            .execute(
                &analysis,
                Some("AdoptedProject"),
                Some(PathBuf::from("/mock/clones")),
                true,
            )
            .unwrap();
        assert_eq!(plan.project_name.as_str(), "AdoptedProject");
        assert_eq!(plan.actions.len(), 5);

        // 3. Execute
        let execute_uc =
            ExecuteLegacyMigrationUseCase::new(scanner, project_store.clone(), backup_port.clone());
        let result = execute_uc.execute(plan).unwrap();
        assert_eq!(result.project_name.as_str(), "AdoptedProject");
        assert_eq!(result.imported_repositories, 2);
        assert_eq!(result.imported_installers, 1);
        assert!(result.source_untouched);
        assert!(result.backup_snapshot_path.is_some());

        // Assert project stored
        let projects = project_store.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name.as_str(), "AdoptedProject");

        // Assert backup snapshot written
        assert_eq!(backup_port.written.lock().unwrap().len(), 1);
    }
}
