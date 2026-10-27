//! Pure domain models and validation rules for legacy setup migration and adoption.

use crate::project::{ProjectId, ProjectName};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Classification of a discovered legacy workspace or configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LegacySetupKind {
    /// ArtCraft workspace with `.release-desk/data.json`, sync script, and/or release-desk.html.
    ArtCraftWorkspace,
    /// Direct standalone `data.json` snapshot document.
    SnapshotFile,
    /// Legacy shell script (e.g. `storytold-sync.sh`) defining repositories and sync logic.
    ShellScript,
    /// Directory containing loose git repository clones.
    CloneDirectory,
    /// Unrecognized or unsupported file or folder structure.
    Unknown,
}

/// Detailed analysis of an inspected legacy source path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacySetupAnalysis {
    pub source_path: PathBuf,
    pub detected_kind: LegacySetupKind,
    pub suggested_project_name: String,
    pub discovered_org: Option<String>,
    pub repositories: Vec<String>,
    pub installer_assets: Vec<String>,
    pub warnings: Vec<String>,
}

impl LegacySetupAnalysis {
    pub fn new(source_path: PathBuf, detected_kind: LegacySetupKind) -> Self {
        let suggested_name = source_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("MigratedProject")
            .to_string();

        Self {
            source_path,
            detected_kind,
            suggested_project_name: suggested_name,
            discovered_org: None,
            repositories: Vec::new(),
            installer_assets: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Returns true if this analysis discovered actionable repositories or snapshot data.
    pub fn is_adoptable(&self) -> bool {
        self.detected_kind != LegacySetupKind::Unknown
            && (!self.repositories.is_empty() || self.discovered_org.is_some())
    }
}

/// Specific discrete step within a migration plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MigrationAction {
    RegisterProject {
        name: ProjectName,
        clone_location: Option<PathBuf>,
    },
    LinkSnapshotReadOnly {
        snapshot_path: PathBuf,
    },
    ImportRepositories {
        count: usize,
    },
    CreateLocalBackupSnapshot {
        backup_path: PathBuf,
    },
    CopyInstallers {
        count: usize,
    },
}

/// An immutable preview plan describing all actions that will be performed during migration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub source_path: PathBuf,
    pub project_name: ProjectName,
    pub clone_location: Option<PathBuf>,
    pub copy_installers: bool,
    pub actions: Vec<MigrationAction>,
}

impl MigrationPlan {
    /// Builds a migration plan from a validated analysis.
    pub fn build(
        analysis: &LegacySetupAnalysis,
        project_name: ProjectName,
        clone_location: Option<PathBuf>,
        copy_installers: bool,
    ) -> Self {
        let mut actions = Vec::new();

        actions.push(MigrationAction::RegisterProject {
            name: project_name.clone(),
            clone_location: clone_location.clone(),
        });

        match analysis.detected_kind {
            LegacySetupKind::ArtCraftWorkspace => {
                let snapshot_file = analysis.source_path.join(".release-desk/data.json");
                actions.push(MigrationAction::LinkSnapshotReadOnly {
                    snapshot_path: snapshot_file,
                });
            }
            LegacySetupKind::SnapshotFile => {
                actions.push(MigrationAction::LinkSnapshotReadOnly {
                    snapshot_path: analysis.source_path.clone(),
                });
            }
            _ => {}
        }

        if !analysis.repositories.is_empty() {
            actions.push(MigrationAction::ImportRepositories {
                count: analysis.repositories.len(),
            });
        }

        if let Some(ref loc) = clone_location {
            actions.push(MigrationAction::CreateLocalBackupSnapshot {
                backup_path: loc.join(".release-desk/data.json"),
            });
        }

        if copy_installers && !analysis.installer_assets.is_empty() {
            actions.push(MigrationAction::CopyInstallers {
                count: analysis.installer_assets.len(),
            });
        }

        Self {
            source_path: analysis.source_path.clone(),
            project_name,
            clone_location,
            copy_installers,
            actions,
        }
    }
}

/// The verifiable outcome of executing a migration plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationResult {
    pub project_id: ProjectId,
    pub project_name: ProjectName,
    pub imported_repositories: usize,
    pub imported_installers: usize,
    pub backup_snapshot_path: Option<PathBuf>,
    pub source_untouched: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_analysis_and_plan_generation() {
        let path = PathBuf::from("/legacy/ArtCraft");
        let mut analysis =
            LegacySetupAnalysis::new(path.clone(), LegacySetupKind::ArtCraftWorkspace);
        analysis.discovered_org = Some("storytold".to_string());
        analysis.repositories = vec!["repo1".to_string(), "repo2".to_string()];
        analysis.installer_assets = vec!["app.dmg".to_string()];

        assert!(analysis.is_adoptable());
        assert_eq!(analysis.suggested_project_name, "ArtCraft");

        let project_name = ProjectName::new("ArtCraftMigrated").unwrap();
        let clone_location = Some(PathBuf::from("/Users/test/ArtCraftClones"));

        let plan = MigrationPlan::build(
            &analysis,
            project_name.clone(),
            clone_location.clone(),
            true,
        );
        assert_eq!(plan.project_name, project_name);
        assert_eq!(plan.actions.len(), 5);

        // Verify actions
        assert_eq!(
            plan.actions[0],
            MigrationAction::RegisterProject {
                name: project_name.clone(),
                clone_location: clone_location.clone(),
            }
        );
        assert_eq!(
            plan.actions[1],
            MigrationAction::LinkSnapshotReadOnly {
                snapshot_path: PathBuf::from("/legacy/ArtCraft/.release-desk/data.json"),
            }
        );
        assert_eq!(
            plan.actions[2],
            MigrationAction::ImportRepositories { count: 2 }
        );
        assert_eq!(
            plan.actions[3],
            MigrationAction::CreateLocalBackupSnapshot {
                backup_path: PathBuf::from("/Users/test/ArtCraftClones/.release-desk/data.json"),
            }
        );
        assert_eq!(
            plan.actions[4],
            MigrationAction::CopyInstallers { count: 1 }
        );
    }
}
