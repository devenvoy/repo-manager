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
            self.release_scroll_handle
                .set_offset(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(0.)));
            self.release_scroll_generation
                .set(self.release_scroll_generation.get().wrapping_add(1));
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
        let cancelled = self.workspace_cancel.clone();
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
            let repositories = crate::modules::release_service::fetch_project_releases(
                &project_clone,
                token,
                &secret_store,
                Some(&cancelled),
            );
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            let presentations = crate::workspace_releases::build_release_presentations(
                &project_clone.directory,
                &repositories,
                &extension,
            );
            let store_apps = crate::store_catalog::apps_for_project(&project_clone, &repositories);
            let _ = tx.send(Ok((repositories, presentations, extension, store_apps)));
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(Ok((repositories, presentations, extension, store_apps))) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.workspace_revision == revision {
                                    this.release_busy = false;
                                    this.remember_project_releases(
                                        project_id,
                                        &repositories,
                                        store_apps,
                                        cx,
                                    );
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
            source.repositories.iter().any(|item| {
                item.name == repo
                    && item.is_private
                    && crate::repository_git::repository_owner(source, item)
                        .eq_ignore_ascii_case(&owner)
            })
        });
        let custom_host = (provider == "custom" || provider == "azure_devops")
            .then(|| {
                (provider == "custom")
                    .then(|| project.custom_host.clone())
                    .flatten()
                    .or_else(|| {
                        project
                            .sources
                            .iter()
                            .flat_map(|source| &source.repositories)
                            .find(|item| item.name == repo)
                            .and_then(|item| {
                                crate::modules::release_service::clone_host(&item.clone_url)
                            })
                            .filter(|host| {
                                provider != "azure_devops"
                                    || host == "dev.azure.com"
                                    || host.ends_with(".visualstudio.com")
                            })
                            .map(|host| format!("https://{host}"))
                    })
            })
            .flatten();
        let (token, username) = if private && project.provider.as_str() == provider {
            match project_auth_token(project, &self.secret_store) {
                Ok(token) => (token, Some(project.account.clone())),
                Err(error) => {
                    self.message = error;
                    cx.notify();
                    return;
                }
            }
        } else if private {
            let host = custom_host
                .as_deref()
                .and_then(|value| url::Url::parse(value).ok())
                .and_then(|value| value.host_str().map(str::to_string))
                .unwrap_or_else(|| {
                    crate::workspace::ProviderChoice::from_source_provider(&provider)
                        .default_host()
                        .to_string()
                });
            match release_desk_store::OsSecretStore::discover_system_git_credentials(&host) {
                Some((username, token)) => (Some(token), Some(username)),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        if private && token.is_none() && !crate::modules::release_service::signed_asset_url(&url) {
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
                (provider, Some(token)) => {
                    release_desk_store::workspace_releases::download_provider_asset_with_progress(
                        &directory,
                        provider,
                        &owner,
                        &repo,
                        &tag,
                        &name,
                        &url,
                        size,
                        &token,
                        username.as_deref(),
                        custom_host.as_deref(),
                        &mut |_, _| {},
                    )
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
