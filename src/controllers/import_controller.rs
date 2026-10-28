//! Interactive repository and metadata import controller.

use crate::app::ReleaseDesk;
use crate::views;
use gpui_kit::{Context, Window};

impl ReleaseDesk {
    pub(crate) fn begin_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };

        let Some(discovered) = crate::modules::importer::discover_workspace(&project.directory)
        else {
            self.message = format!(
                "No metadata (.release-desk/data.json) or Git repositories found in project folder ({}).",
                project.directory.display()
            );
            cx.notify();
            return;
        };

        if discovered.repositories.is_empty() {
            self.message = format!(
                "No repositories found in project folder ({}).",
                project.directory.display()
            );
            cx.notify();
            return;
        }

        let state = views::import_dialog::ImportDialogState::new(
            project.id,
            project.directory,
            discovered,
            window,
            cx,
        );

        // Pre-cache avatar proactively in background so it displays immediately in the dialog
        if let Some(ref url) = state.discovered.avatar_url {
            if let Some(ref path) = state.avatar_path {
                let url_clone = url.clone();
                let path_clone = path.clone();
                std::thread::spawn(move || {
                    let _ =
                        release_desk_store::source_fetcher::cache_avatar(&url_clone, &path_clone);
                });
            }
        }

        self.import_dialog = Some(state);
        cx.notify();
    }

    pub(crate) fn toggle_import_repo(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(ref mut dialog) = self.import_dialog {
            dialog.toggle_repo(index);
            cx.notify();
        }
    }

    pub(crate) fn select_all_import_repos(&mut self, select: bool, cx: &mut Context<Self>) {
        if let Some(ref mut dialog) = self.import_dialog {
            dialog.set_all_selected(select);
            cx.notify();
        }
    }

    pub(crate) fn cancel_import_dialog(&mut self, cx: &mut Context<Self>) {
        self.import_dialog = None;
        cx.notify();
    }

    pub(crate) fn confirm_import_dialog(
        &mut self,
        _candidate_handle: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.import_dialog.take() else {
            return;
        };

        let input_val = dialog.handle_input.read(cx).value().to_string();
        let confirmed_handle = if !input_val.trim().is_empty() {
            input_val.trim().to_string()
        } else {
            dialog.discovered.source_handle.clone()
        };

        let mut discovered = dialog.discovered;
        for (idx, &is_selected) in dialog.selected_indices.iter().enumerate() {
            if let Some(repo) = discovered.repositories.get_mut(idx) {
                repo.selected = is_selected;
            }
        }

        let (new_source, initial_releases) =
            crate::modules::importer::build_confirmed_source(&discovered, &confirmed_handle);

        let discovered_repos_count = new_source.repositories.len();

        if discovered_repos_count == 0 {
            self.message = "No repositories were selected for import.".into();
            cx.notify();
            return;
        }

        let source_handle_clean = new_source.handle.clone();
        let avatar_req = new_source.avatar_cache_request();

        self.change_selected_project(
            |p| {
                if let Some(existing) = p.sources.iter_mut().find(|s| s.handle == new_source.handle)
                {
                    for repo in new_source.repositories {
                        if let Some(existing_repo) = existing
                            .repositories
                            .iter_mut()
                            .find(|r| r.name == repo.name)
                        {
                            existing_repo.selected = true;
                        } else {
                            existing.repositories.push(repo);
                        }
                    }
                } else {
                    p.sources.push(new_source);
                }
            },
            cx,
        );

        // Preload release state if snapshot has releases
        if !initial_releases.is_empty() {
            let mut loaded_repos = Vec::new();
            for (repo_name, releases) in initial_releases {
                loaded_repos.push(crate::workspace_releases::RepositoryReleases {
                    provider: "github".to_string(),
                    owner: source_handle_clean.clone(),
                    repository: repo_name,
                    result: Ok(releases),
                });
            }
            let extension = self.settings.installer_extension.clone();
            self.release_state = crate::workspace_releases::ReleaseViewState::Loaded {
                repositories: loaded_repos,
                presentations: Vec::new(),
                extension,
            };
            self.refresh_release_presentations(cx);
        }

        // Cache avatar in background
        if let Some((url, path)) = avatar_req {
            if !path.is_file() {
                std::thread::spawn(move || {
                    let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path);
                });
            }
        }

        self.message =
            format!("Imported {discovered_repos_count} repositories from @{source_handle_clean}.");
        self.check_repository_updates(None, true, cx);
        cx.notify();
    }
}
