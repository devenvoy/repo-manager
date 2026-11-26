//! Project tracking and safe local checkout removal actions keyed by stable [`RepoKey`].

use crate::app::ReleaseDesk;
use crate::controllers::job_registry::StatusToast;
use crate::repository_git::{
    delete_repository_checkout, destination_allows_auto_remove, move_incomplete_dir_to_trash,
    remove_incomplete_checkout, repository_owner, resolve_target_path,
};
use crate::views::project_repository_status::RepositoryAction;
use crate::workspace_sources::RepoKey;
use gpui_kit::{Context, Window};
use release_desk_git::{DestinationKind, classify_destination};

impl ReleaseDesk {
    pub(crate) fn begin_repository_action(
        &mut self,
        key: RepoKey,
        action: RepositoryAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == RepositoryAction::RefreshIndex {
            self.refresh_repository_index(key, cx);
            return;
        }
        if action == RepositoryAction::RemoveFromProject {
            let Some(project_id) = self.selected_project_id else {
                return;
            };
            self.change_selected_project(
                |project| {
                    if let Some((source_index, repo_index, _, _)) = project.find_repo_by_key(&key)
                        && let Some(source) = project.sources.get_mut(source_index)
                    {
                        source.repositories.remove(repo_index);
                    }
                },
                cx,
            );
            let removed = self
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .is_some_and(|project| project.find_repo_by_key(&key).is_none());
            if removed {
                self.set_toast(StatusToast::info(
                    "Repository removed from this project. Its files remain on disk.",
                ));
            }
            cx.notify();
            return;
        }
        self.repository_action_confirmation
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.repository_action_dialog = Some((key, action));
        cx.notify();
    }

    pub(crate) fn remove_incomplete_repository(
        &mut self,
        key: RepoKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
            .cloned()
        else {
            return;
        };
        let Some((_, _, source, repo)) = project.find_repo_by_key(&key) else {
            return;
        };
        let path = resolve_target_path(
            &project.directory,
            project.disk_layout,
            repository_owner(source, repo),
            &repo.name,
        );
        let kind = classify_destination(&project.directory, &path);
        if matches!(
            kind,
            DestinationKind::WrongRoot { .. } | DestinationKind::Symlink(_)
        ) {
            self.set_toast(StatusToast::warning(
                "Refusing to remove a symbolic link or directory inside another Git repository.",
            ));
            cx.notify();
            return;
        }
        // Only empty dirs, `.git`-only dirs (zero worktree files), or `.release-desk-partial`-marked
        // dirs can be deleted automatically; non-git folders with user files show contents & Trash confirm.
        if !destination_allows_auto_remove(&project.directory, &path) {
            self.begin_repository_action(key, RepositoryAction::DeleteFromDisk, window, cx);
            return;
        }
        match remove_incomplete_checkout(&project.directory, &path) {
            Ok(()) => {
                let runtime = self.jobs.project_mut(project.id);
                runtime.clone_states.remove(&key);
                self.set_toast(StatusToast::info(format!(
                    "Removed incomplete folder for {}.",
                    repo.name
                )));
                self.inspect_repo_keys(project.id, vec![key], cx);
            }
            Err(error) => {
                self.set_toast(StatusToast::error(error));
            }
        }
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
        let Some((key, action)) = self.repository_action_dialog.clone() else {
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
        let Some((source_index, repo_index, source, repo)) = project.find_repo_by_key(&key) else {
            self.set_toast(StatusToast::warning(
                "Repository no longer exists in this project.",
            ));
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
            self.set_toast(StatusToast::warning(format!(
                "Type {repo_name} exactly to confirm deletion."
            )));
            cx.notify();
            return;
        }
        let path = resolve_target_path(
            &project.directory,
            project.disk_layout,
            repository_owner(source, repo),
            &repo.name,
        );
        if action == RepositoryAction::RemoveStaleLocks {
            self.repository_action_dialog = None;
            self.repository_action_confirmation
                .update(cx, |input, cx| input.set_value(String::new(), window, cx));
            self.remove_repository_stale_locks(key, cx);
            return;
        }
        let kind = classify_destination(&project.directory, &path);
        if matches!(
            kind,
            DestinationKind::WrongRoot { .. } | DestinationKind::Symlink(_)
        ) {
            self.set_toast(StatusToast::error(
                "Refusing to delete a symbolic link or directory inside another Git repository.",
            ));
            cx.notify();
            return;
        }
        let moved_to_trash =
            kind == DestinationKind::NonGitDir && !release_desk_git::has_partial_marker(&path);
        let disk_result = if moved_to_trash {
            move_incomplete_dir_to_trash(&project.directory, &path)
        } else {
            delete_repository_checkout(&project.directory, &path)
        };
        if let Err(error) = disk_result {
            self.set_toast(StatusToast::error(error));
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
            self.set_toast(StatusToast::info(if moved_to_trash {
                format!("Moved {repo_name} to Trash and removed it from this project.")
            } else {
                format!("Deleted {repo_name} from disk and removed it from this project.")
            }));
        } else {
            self.apply_repo_update(
                project.id,
                key.clone(),
                crate::controllers::job_registry::RepoUpdate::Remove,
            );
            self.jobs.project_mut(project.id).clone_states.remove(&key);
            self.set_toast(StatusToast::info(if moved_to_trash {
                format!("Moved {repo_name} folder to macOS Trash.")
            } else {
                format!(
                    "Deleted {repo_name} from disk. It remains in this project's tracking list."
                )
            }));
            self.inspect_repo_keys(project.id, vec![key], cx);
        }
        cx.notify();
    }
}
