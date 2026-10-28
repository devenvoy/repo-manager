//! Source discovery, repository selection, and catalog preview controller.

use crate::app::{ReleaseDesk, project_auth_token};
use crate::settings;
use crate::views;
use crate::workspace_sources::WorkspaceSource;
use gpui_kit::{Context, Window};
use release_desk_core::app_catalog::AppActionState;

impl ReleaseDesk {
    pub(crate) fn add_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_busy {
            return;
        }
        let raw = self.source_input.read(cx).value().trim().to_string();
        if raw.is_empty() {
            self.message = "Enter a user handle, organization, or repository URL.".into();
            cx.notify();
            return;
        }

        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
        else {
            self.message = "No project selected.".into();
            cx.notify();
            return;
        };

        let default_provider = project.provider.as_str();
        let (provider, owner, _) =
            release_desk_store::source_fetcher::parse_input_target(&raw, default_provider);

        if owner.is_empty()
            || owner.contains(['?', '#', ' ', '\\'])
            || owner.chars().any(char::is_control)
        {
            self.message = "Enter a valid user or organization handle, or profile URL.".into();
            cx.notify();
            return;
        }

        if let Some(existing_index) = project.sources.iter().position(|existing| {
            existing.provider.eq_ignore_ascii_case(&provider)
                && existing.handle.eq_ignore_ascii_case(&owner)
        }) {
            self.source_dialog = views::project_sources::SourceDialog::Select(existing_index);
            self.message = format!(
                "@{owner} is already a source. Choose repositories from its existing profile."
            );
            cx.notify();
            return;
        }

        let project_id = project.id;
        let auth_token = if provider == project.provider.as_str() {
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

        let input_for_fetch = raw.clone();
        let provider_for_fetch = default_provider.to_string();
        let auth_token_clone = auth_token.clone();

        self.source_busy = true;
        self.message = format!("Fetching live repositories for @{owner} from {provider}...");
        cx.notify();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = release_desk_store::source_fetcher::fetch_source_input(
                &input_for_fetch,
                &provider_for_fetch,
                auth_token_clone.as_deref(),
            );
            if let Ok((profile, _)) = &res {
                if let Some((url, path)) =
                    WorkspaceSource::from_fetched(profile.clone(), None).avatar_cache_request()
                {
                    let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path);
                }
            }
            let _ = tx.send(res);
        });

        let display_owner = owner.clone();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;

                match rx.try_recv() {
                    Ok(res) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.source_busy = false;
                            match res {
                                Ok((profile, specific_repo)) => {
                                    let source = WorkspaceSource::from_fetched(
                                        profile,
                                        specific_repo.as_deref(),
                                    );
                                    if specific_repo.is_some() && !source.repositories.iter().any(|repo| repo.selected) {
                                        this.message = format!("Repository {} was not found or is not accessible.", specific_repo.as_deref().unwrap_or_default());
                                        cx.notify();
                                        return;
                                    }
                                    let repo_count = source.repositories.len();
                                    let handle = source.handle.clone();
                                    let mut updated = this.settings.clone();
                                    if let Some(project) = updated.projects.iter_mut().find(|p| p.id == project_id) {
                                        if !project.sources.iter().any(|existing| existing.provider == source.provider && existing.handle.eq_ignore_ascii_case(&source.handle)) {
                                            project.sources.push(source);
                                            let source_index = project.sources.len() - 1;
                                            match settings::save(&updated) {
                                                Ok(()) => {
                                                    this.projects = updated.projects.clone();
                                                    this.settings = updated;
                                                    this.workspace_revision = this.workspace_revision.wrapping_add(1);
                                                    this.git_statuses.clear();
                                                    this.release_state = Default::default();
                                                    if this.selected_project_id == Some(project_id) {
                                                        this.source_dialog = views::project_sources::SourceDialog::Select(source_index);
                                                    }
                                                    if this.selected_project_id == Some(project_id) && this.source_input.read(cx).value().trim() == raw {
                                                        this.source_input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
                                                    }
                                                    this.message = format!("Discovered {repo_count} repositories for @{handle}.");
                                                }
                                                Err(error) => this.message = error,
                                            }
                                        } else { this.message = format!("Source @{handle} is already in this project."); }
                                    } else { this.message = "The project was removed while the source was loading.".into(); }
                                }
                                Err(err) => {
                                    this.message =
                                        format!("Could not fetch source @{display_owner}: {err}");
                                }
                            }
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _window, cx| {
                            this.source_busy = false;
                            this.message = format!(
                                "Failed to fetch source @{display_owner}: request aborted."
                            );
                            cx.notify();
                        });
                        return;
                    }
                }
            }
        })
        .detach();
    }

    pub(crate) fn refresh_source(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.source_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
        else {
            return;
        };
        let Some(source) = project.sources.get(index) else {
            return;
        };
        let project_id = project.id;
        let handle = source.handle.clone();
        let provider = source.provider.clone();
        let token = if provider == project.provider.as_str() {
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
        let fetch_handle = handle.clone();
        let fetch_provider = provider.clone();
        self.source_busy = true;
        self.message = format!("Refreshing @{handle}…");
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = release_desk_store::source_fetcher::fetch_profile_and_repos(
                &fetch_handle,
                &fetch_provider,
                token.as_deref(),
            );
            if let Ok(profile) = &result {
                if let Some((url, path)) =
                    WorkspaceSource::from_fetched(profile.clone(), None).avatar_cache_request()
                {
                    let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path);
                }
            }
            let _ = tx.send(result);
        });
        cx.spawn_in(window, async move |this, cx| { loop {
            cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
            match rx.try_recv() {
                Ok(result) => { let _ = this.update_in(cx, |this, _, cx| {
                    this.source_busy = false;
                    match result {
                        Err(error) => this.message = format!("Could not refresh @{handle}: {error}"),
                        Ok(profile) => {
                            let mut updated = this.settings.clone();
                            if let Some(source) = updated.projects.iter_mut().find(|p| p.id == project_id).and_then(|p| p.sources.iter_mut().find(|s| s.provider == provider && s.handle.eq_ignore_ascii_case(&handle))) {
                                source.refresh_from_fetched(profile);
                                match settings::save(&updated) {
                                    Ok(()) => { this.settings = updated; this.projects = this.settings.projects.clone(); this.workspace_revision = this.workspace_revision.wrapping_add(1); this.git_statuses.clear(); this.release_state = Default::default(); this.message = format!("Refreshed @{handle}; your selections were kept."); }
                                    Err(error) => this.message = error,
                                }
                            } else { this.message = format!("@{handle} was removed while refreshing."); }
                        }
                    } cx.notify();
                }); return; }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { let _ = this.update_in(cx, |this, _, cx| { this.source_busy = false; this.message = "Source refresh stopped unexpectedly.".into(); cx.notify(); }); return; }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }}).detach();
    }

    pub(crate) fn remove_source(&mut self, source_index: usize, cx: &mut Context<Self>) {
        self.source_dialog = views::project_sources::SourceDialog::Closed;
        self.change_selected_project(
            |project| {
                if source_index < project.sources.len() {
                    project.sources.remove(source_index);
                }
            },
            cx,
        );
    }

    pub(crate) fn open_source_url(&mut self, url: &str, cx: &mut Context<Self>) {
        let safe = url::Url::parse(url).is_ok_and(|parsed| {
            (parsed.scheme() == "https" || parsed.scheme() == "http")
                && parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none()
        });
        if !safe {
            self.message = "Could not open this provider link safely.".into();
            cx.notify();
            return;
        }

        let now = std::time::Instant::now();
        if let Some((last_url, last_time)) = &self.last_browser_open {
            if last_url == url
                && now.duration_since(*last_time) < std::time::Duration::from_millis(1000)
            {
                return;
            }
        }
        self.last_browser_open = Some((url.to_string(), now));

        self.message = format!("Opening {url} in browser…");
        cx.open_url(url);
        cx.notify();
    }

    pub(crate) fn toggle_source_repo(
        &mut self,
        source_index: usize,
        repo_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.change_selected_project(
            |project| {
                if let Some(repo) = project
                    .sources
                    .get_mut(source_index)
                    .and_then(|source| source.repositories.get_mut(repo_index))
                {
                    repo.selected = !repo.selected;
                }
            },
            cx,
        );
    }

    pub(crate) fn preview_app_action(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(item) = self
            .store_items
            .iter_mut()
            .find(|item| item.metadata.id == id)
        else {
            return;
        };
        let action = match item.state {
            AppActionState::Installable => "installed",
            AppActionState::UpdateAvailable { .. } => "updated",
            AppActionState::UpToDate { .. } => "opened",
        };
        if action != "opened" {
            item.installed_version = item.latest_release_tag.clone();
            item.state = AppActionState::UpToDate {
                version: item.latest_release_tag.clone().unwrap_or_default(),
            };
        }
        self.message = format!("{} {action} in preview only.", item.metadata.name);
        cx.notify();
    }
}
