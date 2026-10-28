//! Upstream release shelf and asset downloading controller.

use crate::app::{ReleaseDesk, project_auth_token};
use gpui_kit::{Context, Window};

impl ReleaseDesk {
    pub(crate) fn refresh_releases(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.release_busy {
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
        let token = match project_auth_token(project, &self.secret_store) {
            Ok(token) => token,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        let project_clone = project.clone();
        self.release_busy = true;
        self.release_state = crate::workspace_releases::ReleaseViewState::Loading;
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let results =
                crate::modules::release_service::fetch_project_releases(&project_clone, token);
            let _ = tx.send(results);
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(results) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_busy = false;
                            if this.selected_project_id == Some(project_id)
                                && this.workspace_revision == revision
                            {
                                this.release_state =
                                    crate::workspace_releases::ReleaseViewState::Loaded(results);
                            }
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_busy = false;
                            this.release_state = Default::default();
                            this.message = "Release fetch stopped unexpectedly.".into();
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
                            this.message = match result {
                                Ok(path) => format!("Downloaded to {}", path.display()),
                                Err(error) => format!("Download failed: {error}"),
                            };
                            cx.notify();
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
