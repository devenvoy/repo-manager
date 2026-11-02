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

        let project_id = project.id;
        let directory = project.directory;
        let revision = self.workspace_revision;
        let worker_directory = directory.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.message = "Searching project metadata…".into();
        cx.notify();
        std::thread::spawn(move || {
            let _ = tx.send(crate::modules::importer::discover_workspace(
                &worker_directory,
            ));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
                match rx.try_recv() {
                    Ok(discovered) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if this.workspace_revision != revision || this.selected_project_id != Some(project_id) { return; }
                            let Some(discovered) = discovered.filter(|value| !value.repositories.is_empty()) else {
                                this.message = format!("No metadata (.release-desk/data.json) or Git repositories found in project folder ({}).", directory.display());
                                cx.notify();
                                return;
                            };
                            let state = views::import_dialog::ImportDialogState::new(project_id, directory, discovered, window, cx);
                            if let (Some(url), Some(path)) = (state.discovered.avatar_url.clone(), state.avatar_path.clone()) {
                                std::thread::spawn(move || { let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path); });
                            }
                            this.import_dialog = Some(state);
                            this.message.clear();
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| { if this.workspace_revision == revision { this.message = "Project search stopped unexpectedly.".into(); cx.notify(); } });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        }).detach();
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

        let (new_source, _initial_releases) =
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

        // Cache avatar in background
        if let Some((url, path)) = avatar_req {
            std::thread::spawn(move || {
                if !path.is_file() {
                    let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path);
                }
            });
        }

        self.message =
            format!("Imported {discovered_repos_count} repositories from @{source_handle_clean}.");
        self.check_repository_updates(true, cx);
        self.refresh_releases(cx);
        cx.notify();
    }
}
