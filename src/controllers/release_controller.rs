//! Upstream release shelf and asset downloading controller.

use crate::app::{ReleaseDesk, project_auth_token};
use gpui_kit::{Context, Window};

impl ReleaseDesk {
    pub(crate) fn toggle_release_queue(&mut self, cx: &mut Context<Self>) {
        self.release_queue_open = !self.release_queue_open;
        cx.notify();
    }

    pub(crate) fn cancel_release_download(&mut self, key: String, cx: &mut Context<Self>) {
        if let Some(job) = self.release_queue.iter_mut().find(|job| job.key == key) {
            job.cancel.store(true, std::sync::atomic::Ordering::Release);
            if matches!(job.status, crate::release_queue::DownloadStatus::Queued) {
                job.status = crate::release_queue::DownloadStatus::Cancelled;
                self.release_downloading.remove(&job.key);
            }
        }
        cx.notify();
    }

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
        let download_key = format!(
            "{}/{owner}/{repo}/{tag}/{name}",
            self.selected_project_id.unwrap_or_default()
        );
        if self
            .release_queue
            .iter()
            .any(|job| job.key == download_key && job.is_active())
        {
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
        let custom_host = matches!(provider.as_str(), "custom" | "forgejo" | "azure_devops")
            .then(|| {
                project.custom_host.clone().or_else(|| {
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
        let auth_project = project.clone();
        let secret_store = self.secret_store.clone();
        self.release_downloading.insert(download_key.clone());
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.release_queue.push(crate::release_queue::DownloadJob {
            key: download_key.clone(),
            project_id: self.selected_project_id.unwrap_or_default(),
            repository: format!("{owner}/{repo}"),
            asset: name.clone(),
            status: crate::release_queue::DownloadStatus::Queued,
            received: 0,
            total: size,
            cancel: cancel.clone(),
        });
        self.message = format!("Downloading {name}…");
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        crate::release_queue::submit(move || {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                let _ = tx.send(crate::release_queue::DownloadUpdate::Finished(Err(
                    "Download cancelled.".into(),
                )));
                return;
            }
            let _ = tx.send(crate::release_queue::DownloadUpdate::Progress(0, size));
            let result = (|| {
                let (token, username) = if private && auth_project.provider.as_str() == provider {
                    (
                        project_auth_token(&auth_project, &secret_store)?,
                        Some(auth_project.account.clone()),
                    )
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
                    secret_store
                        .discover_cached_system_git_credentials(&host)
                        .map_or((None, None), |(username, token)| {
                            (Some(token), Some(username))
                        })
                } else {
                    (None, None)
                };
                if private
                    && token.is_none()
                    && !crate::modules::release_service::signed_asset_url(&url)
                {
                    return Err(format!(
                        "Connect a {provider} account with access to download this private asset."
                    ));
                }
                let mut progress = |received, total| -> Result<(), String> {
                    if cancel.load(std::sync::atomic::Ordering::Acquire) {
                        return Err("Download cancelled.".into());
                    }
                    let _ = tx.send(crate::release_queue::DownloadUpdate::Progress(
                        received, total,
                    ));
                    Ok(())
                };
                match (provider.as_str(), token) {
                ("github", Some(token)) => {
                    release_desk_store::workspace_releases::download_github_asset_with_progress(
                        &directory, &owner, &repo, &tag, &name, size, &token, &mut progress,
                    )
                }
                ("gitlab", Some(token)) => {
                    release_desk_store::workspace_releases::download_gitlab_asset_with_progress(
                        &directory, &owner, &repo, &tag, &name, &url, size, &token, &mut progress,
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
                        &mut progress,
                    )
                }
                (_, None) => release_desk_store::workspace_releases::download_asset_with_progress(
                    &directory, &owner, &repo, &tag, &name, &url, size, &mut progress,
                ),
                }
            })();
            let _ = tx.send(crate::release_queue::DownloadUpdate::Finished(result));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(crate::release_queue::DownloadUpdate::Progress(received, total)) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            if let Some(job) = this
                                .release_queue
                                .iter_mut()
                                .find(|job| job.key == download_key)
                            {
                                job.status = crate::release_queue::DownloadStatus::Downloading;
                                job.received = received;
                                job.total = total;
                            }
                            cx.notify();
                        });
                    }
                    Ok(crate::release_queue::DownloadUpdate::Finished(result)) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_downloading.remove(&download_key);
                            let succeeded = result.is_ok();
                            if let Some(job) = this
                                .release_queue
                                .iter_mut()
                                .find(|job| job.key == download_key)
                            {
                                job.status =
                                    if job.cancel.load(std::sync::atomic::Ordering::Acquire) {
                                        crate::release_queue::DownloadStatus::Cancelled
                                    } else if let Err(error) = &result {
                                        crate::release_queue::DownloadStatus::Failed(error.clone())
                                    } else {
                                        crate::release_queue::DownloadStatus::Complete
                                    };
                            }
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
                            this.release_downloading.remove(&download_key);
                            if let Some(job) = this
                                .release_queue
                                .iter_mut()
                                .find(|job| job.key == download_key)
                            {
                                job.status = crate::release_queue::DownloadStatus::Failed(
                                    "Download worker stopped unexpectedly.".into(),
                                );
                            }
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
