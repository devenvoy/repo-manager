//! Project management use cases.

use release_desk_core::error::DomainError;
use release_desk_core::port::ProjectStore;
use release_desk_core::project::{AccountIdentity, ProjectId, ProjectIdentity, ProjectName};
use std::sync::Arc;

/// Input command for creating a new project.
pub struct CreateProjectCommand {
    pub id: ProjectId,
    pub name: ProjectName,
    pub account: AccountIdentity,
}

/// Use case for creating and registering a new project.
pub struct CreateProjectUseCase {
    project_store: Arc<dyn ProjectStore>,
}

impl CreateProjectUseCase {
    pub fn new(project_store: Arc<dyn ProjectStore>) -> Self {
        Self { project_store }
    }

    pub fn execute(&self, cmd: CreateProjectCommand) -> Result<ProjectIdentity, DomainError> {
        let identity = ProjectIdentity {
            id: cmd.id,
            name: cmd.name,
        };

        // Check if project already exists
        if self.project_store.get_project(identity.id)?.is_some() {
            return Err(DomainError::Conflict {
                message: format!("Project with id {} already exists", identity.id.as_u128()),
            });
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
            })
            .expect("create project");

        assert_eq!(created.id, project_id);
        assert_eq!(created.name, name);

        let projects = list_uc.execute().expect("list projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name.as_str(), "ArtCraft");
    }
}
