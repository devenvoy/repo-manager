//! Upstream release shelf and asset downloading controller.

use crate::app::{ReleaseDesk, project_auth_token};
use gpui_kit::{Context, Window};

impl ReleaseDesk {
    pub(crate) fn select_release_type(&mut self, extension: String, cx: &mut Context<Self>) {
        self.release_type_menu_open = false;
        let mut changed = false;
        if let Ok(extension) = crate::settings::normalized_extension(&extension) {
            if self.settings.installer_extension != extension {
                self.settings.installer_extension = extension;
                changed = true;
                if let Err(error) = crate::settings::save(&self.settings) {
                    self.message = format!("Cannot save release file type: {error}");
                }
            }
        }
        if changed && !self.release_busy {
            self.refresh_release_presentations(cx);
        }
        cx.notify();
    }

    pub(crate) fn refresh_releases(&mut self, cx: &mut Context<Self>) {
        if self.release_busy || self.release_type_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
        else {
            return;
        };
        let project_id = project.id;
        let revision = self.workspace_revision;
        let project_clone = project.clone();
        let secret_store = self.secret_store.clone();
        let extension = self.settings.installer_extension.clone();
        self.release_busy = true;
        self.release_state = crate::workspace_releases::ReleaseViewState::Loading;
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let token = match project_auth_token(&project_clone, &secret_store) {
                Ok(token) => token,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    return;
                }
            };
            let repositories =
                crate::modules::release_service::fetch_project_releases(&project_clone, token);
            let presentations = crate::workspace_releases::build_release_presentations(
                &project_clone.directory,
                &repositories,
                &extension,
            );
            let _ = tx.send(Ok((repositories, presentations, extension)));
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(Ok((repositories, presentations, extension))) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.workspace_revision == revision {
                                    this.release_busy = false;
                                    if this.selected_project_id == Some(project_id) {
                                        this.release_state =
                                            crate::workspace_releases::ReleaseViewState::Loaded {
                                                repositories,
                                                presentations,
                                                extension,
                                            };
                                    }
                                }
                                cx.notify();
                            });
                        }
                        return;
                    }
                    Ok(Err(error)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.workspace_revision == revision
                                    && this.selected_project_id == Some(project_id)
                                {
                                    this.release_busy = false;
                                    this.release_state = Default::default();
                                    this.message = error;
                                    cx.notify();
                                }
                            });
                        }
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.workspace_revision == revision {
                                    this.release_busy = false;
                                    this.release_state = Default::default();
                                    this.message = "Release fetch stopped unexpectedly.".into();
                                }
                                cx.notify();
                            });
                        }
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }

    pub(crate) fn refresh_release_presentations(&mut self, cx: &mut Context<Self>) {
        let crate::workspace_releases::ReleaseViewState::Loaded { repositories, .. } =
            &self.release_state
        else {
            return;
        };
        if self.release_type_busy || self.release_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
        else {
            return;
        };
        let project_id = project.id;
        let revision = self.workspace_revision;
        let repositories = repositories.clone();
        let extension = self.settings.installer_extension.clone();
        let project_directory = project.directory.clone();
        self.release_type_busy = true;
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let presentations = crate::workspace_releases::build_release_presentations(
                &project_directory,
                &repositories,
                &extension,
            );
            let _ = tx.send((repositories, presentations, extension));
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok((repositories, presentations, extension)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.workspace_revision == revision {
                                    this.release_type_busy = false;
                                    if this.selected_project_id == Some(project_id) {
                                        this.release_state =
                                            crate::workspace_releases::ReleaseViewState::Loaded {
                                                repositories,
                                                presentations,
                                                extension,
                                            };
                                    }
                                }
                                cx.notify();
                            });
                        }
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.workspace_revision == revision {
                                    this.release_type_busy = false;
                                }
                                cx.notify();
                            });
                        }
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn download_release(
        &mut self,
        provider: String,
        owner: String,
        repo: String,
        tag: String,
        name: String,
        url: String,
        size: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.release_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
        else {
            return;
        };
        let directory = project.directory.clone();
        let private = project.sources.iter().any(|source| {
            source.provider == provider
                && source.repositories.iter().any(|item| {
                    item.name == repo
                        && item.is_private
                        && crate::repository_git::repository_owner(source, item)
                            .eq_ignore_ascii_case(&owner)
                })
        });
        let token = if private && project.provider.as_str() == provider {
            match project_auth_token(project, &self.secret_store) {
                Ok(token) => token,
                Err(error) => {
                    self.message = error;
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };
        if private && token.is_none() {
            self.message =
                format!("Connect a {provider} account with access to download this private asset.");
            cx.notify();
            return;
        }
        self.release_busy = true;
        self.message = format!("Downloading {name}…");
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = match (provider.as_str(), token) {
                ("github", Some(token)) => {
                    release_desk_store::workspace_releases::download_github_asset(
                        &directory, &owner, &repo, &tag, &name, size, &token,
                    )
                }
                ("gitlab", Some(token)) => {
                    release_desk_store::workspace_releases::download_gitlab_asset(
                        &directory, &owner, &repo, &tag, &name, &url, size, &token,
                    )
                }
                (_, Some(_)) => {
                    Err("Private downloads are not supported for this provider.".into())
                }
                (_, None) => release_desk_store::workspace_releases::download_asset(
                    &directory, &owner, &repo, &tag, &name, &url, size,
                ),
            };
            let _ = tx.send(result);
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_busy = false;
                            let succeeded = result.is_ok();
                            this.message = match result {
                                Ok(path) => format!("Downloaded to {}", path.display()),
                                Err(error) => format!("Download failed: {error}"),
                            };
                            if succeeded {
                                this.refresh_release_presentations(cx);
                            } else {
                                cx.notify();
                            }
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_busy = false;
                            this.message = "Download stopped unexpectedly.".into();
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }
}
