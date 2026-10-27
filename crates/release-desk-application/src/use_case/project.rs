//! Project management use cases.

use release_desk_core::error::DomainError;
use release_desk_core::port::{ProjectBackupPort, ProjectStore};
use release_desk_core::project::{AccountIdentity, ProjectId, ProjectIdentity, ProjectName};
use release_desk_core::snapshot::Snapshot;
use std::path::PathBuf;
use std::sync::Arc;

/// Input command for creating a new project.
pub struct CreateProjectCommand {
    pub id: ProjectId,
    pub name: ProjectName,
    pub account: AccountIdentity,
    pub clone_location: Option<PathBuf>,
}

/// Use case for creating and registering a new project.
pub struct CreateProjectUseCase {
    project_store: Arc<dyn ProjectStore>,
    backup_port: Option<Arc<dyn ProjectBackupPort>>,
}

impl CreateProjectUseCase {
    pub fn new(project_store: Arc<dyn ProjectStore>) -> Self {
        Self {
            project_store,
            backup_port: None,
        }
    }

    pub fn with_backup_port(
        project_store: Arc<dyn ProjectStore>,
        backup_port: Arc<dyn ProjectBackupPort>,
    ) -> Self {
        Self {
            project_store,
            backup_port: Some(backup_port),
        }
    }

    pub fn execute(&self, cmd: CreateProjectCommand) -> Result<ProjectIdentity, DomainError> {
        let identity =
            ProjectIdentity::with_clone_location(cmd.id, cmd.name, cmd.clone_location.clone());

        // Check if project already exists
        if self.project_store.get_project(identity.id)?.is_some() {
            return Err(DomainError::Conflict {
                message: format!("Project with id {} already exists", identity.id.as_u128()),
            });
        }

        // If clone location is specified and backup port is present, initialize/verify local backup snapshot
        if let (Some(root), Some(backup)) = (&cmd.clone_location, &self.backup_port) {
            if backup.read_backup_snapshot(root)?.is_none() {
                let initial = Snapshot {
                    org: cmd.account.login.as_str().to_string(),
                    generated: "2026-10-27T00:00:00Z".to_string(),
                    repos: Vec::new(),
                };
                backup.write_backup_snapshot(root, &initial)?;
            }
        }

        self.project_store.save_project(&identity)?;
        Ok(identity)
    }
}

/// Use case for listing all active projects.
pub struct ListProjectsUseCase {
    project_store: Arc<dyn ProjectStore>,
}

impl ListProjectsUseCase {
    pub fn new(project_store: Arc<dyn ProjectStore>) -> Self {
        Self { project_store }
    }

    pub fn execute(&self) -> Result<Vec<ProjectIdentity>, DomainError> {
        self.project_store.list_projects()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use release_desk_core::project::AccountLogin;
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

    #[test]
    fn create_and_list_projects() {
        let store = Arc::new(InMemoryProjectStore(Mutex::new(Vec::new())));
        let create_uc = CreateProjectUseCase::new(store.clone());
        let list_uc = ListProjectsUseCase::new(store);

        let project_id = ProjectId::from_u128(101);
        let name = ProjectName::new("ArtCraft").unwrap();
        let account = AccountIdentity {
            login: AccountLogin::new("storytold").unwrap(),
            kind: release_desk_core::project::AccountKind::Organization,
        };

        let created = create_uc
            .execute(CreateProjectCommand {
                id: project_id,
                name: name.clone(),
                account,
                clone_location: None,
            })
            .expect("create project");

        assert_eq!(created.id, project_id);
        assert_eq!(created.name, name);

        let projects = list_uc.execute().expect("list projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name.as_str(), "ArtCraft");
    }
}
