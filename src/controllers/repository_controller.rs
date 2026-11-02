//! Project tracking and safe local checkout removal actions.

use crate::app::ReleaseDesk;
use crate::repository_git::{delete_repository_checkout, repository_owner, resolve_target_path};
use crate::views::project_repository_status::RepositoryAction;
use gpui_kit::{Context, Window};

impl ReleaseDesk {
    pub(crate) fn begin_repository_action(
        &mut self,
        source_index: usize,
        repo_index: usize,
        action: RepositoryAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == RepositoryAction::RemoveFromProject {
            self.change_selected_project(
                |project| {
                    if let Some(source) = project.sources.get_mut(source_index)
                        && repo_index < source.repositories.len()
                    {
                        source.repositories.remove(repo_index);
                    }
                },
                cx,
            );
            let removed = self.selected_project_id.is_some_and(|id| {
                self.projects
                    .iter()
                    .find(|project| project.id == id)
                    .is_some_and(|project| {
                        project
                            .sources
                            .get(source_index)
                            .is_none_or(|source| source.repositories.get(repo_index).is_none())
                    })
            });
            if removed {
                self.message =
                    "Repository removed from this project. Its files remain on disk.".into();
                self.check_repository_updates(true, cx);
            }
            cx.notify();
            return;
        }
        self.repository_action_confirmation
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.repository_action_dialog = Some((source_index, repo_index, action));
        cx.notify();
    }

    pub(crate) fn cancel_repository_action(&mut self, cx: &mut Context<Self>) {
        self.repository_action_dialog = None;
        cx.notify();
    }

    pub(crate) fn confirm_repository_action(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((source_index, repo_index, action)) = self.repository_action_dialog else {
            return;
        };
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
            .cloned()
        else {
            self.repository_action_dialog = None;
            cx.notify();
            return;
        };
        let Some(source) = project.sources.get(source_index) else {
            self.message = "Repository source no longer exists.".into();
            self.repository_action_dialog = None;
            cx.notify();
            return;
        };
        let Some(repo) = source.repositories.get(repo_index) else {
            self.message = "Repository no longer exists in this project.".into();
            self.repository_action_dialog = None;
            cx.notify();
            return;
        };
        let repo_name = repo.name.clone();
        let confirmation = self
            .repository_action_confirmation
            .read(cx)
            .value()
            .to_string();
        if confirmation != repo_name {
            self.message = format!("Type {repo_name} exactly to confirm deletion.");
            cx.notify();
            return;
        }
        let path = resolve_target_path(
            &project.directory,
            project.disk_layout,
            repository_owner(source, repo),
            &repo.name,
        );
        if let Err(error) = delete_repository_checkout(&project.directory, &path) {
            self.message = error;
            cx.notify();
            return;
        }

        self.repository_action_dialog = None;
        self.repository_action_confirmation
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        if action == RepositoryAction::RemoveAndDelete {
            self.change_selected_project(
                |project| {
                    if let Some(source) = project.sources.get_mut(source_index)
                        && repo_index < source.repositories.len()
                    {
                        source.repositories.remove(repo_index);
                    }
                },
                cx,
            );
            let untracked = self.selected_project_id.is_some_and(|id| {
                self.projects
                    .iter()
                    .find(|project| project.id == id)
                    .is_some_and(|project| {
                        project.sources.get(source_index).is_none_or(|source| {
                            source
                                .repositories
                                .get(repo_index)
                                .is_none_or(|repo| repo.name != repo_name)
                        })
                    })
            });
            if untracked {
                self.message =
                    format!("Deleted {repo_name} from disk and removed it from this project.");
            }
        } else {
            self.workspace_revision = self.workspace_revision.wrapping_add(1);
            self.git_statuses.clear();
            self.repository_presentations.clear();
            self.selected_repository = None;
            self.message = format!(
                "Deleted {repo_name} from disk. It remains in this project's tracking list."
            );
        }
        self.check_repository_updates(true, cx);
        cx.notify();
    }
}
