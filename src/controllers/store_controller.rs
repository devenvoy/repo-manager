//! Store synchronization and install actions.

use crate::app::{ReleaseDesk, project_auth_token};
use crate::store_catalog::{StoreApp, apps_for_project, catalog_path, merge_project, save};
use crate::workspace::expand_directory;
use crate::workspace_releases::RepositoryReleases;
use gpui_kit::Context;
use release_desk_application::use_case::macos_installer::{HdiutilAdapter, InstallMacOsAppUseCase};
use release_desk_core::app_catalog::AppActionState;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StoreSort {
    #[default]
    Name,
    RecentlyUpdated,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StoreFilter {
    #[default]
    All,
    Installed,
    Updates,
    Available,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreActivity {
    Downloading(Option<u8>),
    Installing,
}

impl StoreActivity {
    pub fn label(self) -> String {
        match self {
            Self::Downloading(Some(percent)) => format!("Downloading {percent}%"),
            Self::Downloading(None) => "Downloading…".into(),
            Self::Installing => "Installing…".into(),
        }
    }
}

enum StoreEvent {
    Progress(Option<u8>),
    Installing,
    Finished(Result<StoreOutcome, String>),
}

enum StoreOutcome {
    Installed,
    Downloaded,
}

impl ReleaseDesk {
    /// Probe installations off the UI thread and discard outdated results.
    pub(crate) fn refresh_store_install_states(&mut self, cx: &mut Context<Self>) {
        self.store_probe_revision = self.store_probe_revision.wrapping_add(1);
        let revision = self.store_probe_revision;
        let items = self.store_items.clone();
        let applications_dir = expand_directory(&self.settings.applications_dir);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let states = items
                .into_iter()
                .map(|item| {
                    let state = item.state(&applications_dir);
                    (item.key, state)
                })
                .collect();
            let _ = tx.send(states);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(states) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.store_probe_revision == revision {
                                    this.store_install_states = states;
                                    cx.notify();
                                }
                            });
                        }
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }

    pub(crate) fn remember_project_releases(
        &mut self,
        project_id: u128,
        repositories: &[RepositoryReleases],
        fresh: Vec<StoreApp>,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return;
        };
        merge_prepared_store_results(&mut self.store_items, project, repositories, fresh);
        self.store_revision = self.store_revision.wrapping_add(1);
        let path = catalog_path(&expand_directory(&self.settings.library_dir));
        save_store_background(path, self.store_items.clone());
        self.refresh_store_install_states(cx);
    }

    pub(crate) fn refresh_store(&mut self, cx: &mut Context<Self>) {
        if self.store_busy {
            return;
        }
        self.store_busy = true;
        cx.notify();
        let projects = self.projects.clone();
        let secret_store = self.secret_store.clone();
        let path = catalog_path(&expand_directory(&self.settings.library_dir));
        let cached = self.store_items.clone();
        let revision = self.store_revision;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut items = cached;
            items.retain(|app| projects.iter().any(|project| project.id == app.project_id));
            let mut errors = Vec::new();
            for project in projects {
                let token = match project_auth_token(&project, &secret_store) {
                    Ok(token) => token,
                    Err(error) => {
                        errors.push(format!("{}: {error}", project.name));
                        continue;
                    }
                };
                let repositories = crate::modules::release_service::fetch_project_releases(
                    &project,
                    token,
                    &secret_store,
                    None,
                );
                let failed: Vec<_> = repositories
                    .iter()
                    .filter(|repo| repo.result.is_err())
                    .map(|repo| {
                        format!(
                            "{}: {}",
                            repo.repository,
                            repo.result.as_ref().err().unwrap_or(&String::new())
                        )
                    })
                    .collect();
                errors.extend(failed);
                merge_store_results(&mut items, &project, &repositories);
            }
            let _ = tx.send((items, errors));
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok((items, errors)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                this.store_busy = false;
                                if this.store_revision == revision {
                                    this.store_items = items;
                                    this.store_revision = this.store_revision.wrapping_add(1);
                                    save_store_background(path, this.store_items.clone());
                                    this.refresh_store_install_states(cx);
                                }
                                if !errors.is_empty() {
                                    eprintln!("Store refresh errors: {}", errors.join("; "));
                                    this.message = "Some apps couldn't be refreshed. Your saved apps are still here.".into();
                                }
                                cx.notify();
                            });
                        }
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                this.store_busy = false;
                                this.message = "Store refresh stopped unexpectedly.".into();
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

    pub(crate) fn store_action(&mut self, key: String, cx: &mut Context<Self>) {
        let Some(app) = self.store_items.iter().find(|app| app.key == key).cloned() else {
            return;
        };
        if !app.is_app_listing() {
            self.message = "This app isn't available for your computer yet.".into();
            cx.notify();
            return;
        }
        if !crate::store_catalog::safe_bundle_name(&app.bundle_name) {
            self.message = format!("Couldn't install {}. Please try again.", app.display_name());
            cx.notify();
            return;
        }
        let applications_dir = expand_directory(&self.settings.applications_dir);
        if matches!(
            app.state(&applications_dir),
            AppActionState::UpToDate { .. }
        ) {
            let path = applications_dir.join(&app.bundle_name);
            let launcher = InstallMacOsAppUseCase::new(HdiutilAdapter, applications_dir);
            match launcher.launch_installed_app(&path) {
                Ok(()) => self.message = format!("Opening {}…", app.display_name()),
                Err(error) => {
                    eprintln!("Store open failed for {}: {error}", app.display_name());
                    self.message =
                        format!("Couldn't open {}. Please try again.", app.display_name());
                }
            }
            cx.notify();
            return;
        }
        let Some(asset) = app.platform_asset().cloned() else {
            self.message = format!(
                "{} isn't available for your computer yet.",
                app.display_name()
            );
            cx.notify();
            return;
        };
        if self.store_activities.contains_key(&key) {
            return;
        }
        let local_asset = app.platform_release().and_then(|release| {
            release_desk_store::workspace_releases::local_release_asset(
                &app.project_directory,
                &app.owner,
                &app.repository,
                &release.tag,
                &asset.name,
                asset.size,
            )
        });
        let Some(project) = self
            .projects
            .iter()
            .find(|project| project.id == app.project_id)
            .cloned()
        else {
            return;
        };
        let private = project.sources.iter().any(|source| {
            source.repositories.iter().any(|repo| {
                repo.name == app.repository
                    && repo.is_private
                    && crate::repository_git::repository_owner(source, repo)
                        .eq_ignore_ascii_case(&app.owner)
            })
        });
        let custom_host = (app.provider == "custom" || app.provider == "azure_devops")
            .then(|| {
                (app.provider == "custom")
                    .then(|| project.custom_host.clone())
                    .flatten()
                    .or_else(|| {
                        project
                            .sources
                            .iter()
                            .flat_map(|source| &source.repositories)
                            .find(|repo| repo.name == app.repository)
                            .and_then(|repo| {
                                crate::modules::release_service::clone_host(&repo.clone_url)
                            })
                            .filter(|host| {
                                app.provider != "azure_devops"
                                    || host == "dev.azure.com"
                                    || host.ends_with(".visualstudio.com")
                            })
                            .map(|host| format!("https://{host}"))
                    })
            })
            .flatten();
        let (token, username) =
            if local_asset.is_none() && private && project.provider.as_str() == app.provider {
                match project_auth_token(&project, &self.secret_store) {
                    Ok(token) => (token, Some(project.account.clone())),
                    Err(error) => {
                        self.message = error;
                        cx.notify();
                        return;
                    }
                }
            } else if local_asset.is_none() && private {
                let host = custom_host
                    .as_deref()
                    .and_then(|value| url::Url::parse(value).ok())
                    .and_then(|value| value.host_str().map(str::to_string))
                    .unwrap_or_else(|| {
                        crate::workspace::ProviderChoice::from_source_provider(&app.provider)
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
        if local_asset.is_none()
            && private
            && token.is_none()
            && !crate::modules::release_service::signed_asset_url(&asset.download_url)
        {
            self.message =
                "Connect the provider account before installing this private release.".into();
            cx.notify();
            return;
        }
        let app_name = app.display_name();
        let was_installed = app.state(&applications_dir).is_installed();
        self.store_activities.insert(
            key.clone(),
            if local_asset.is_some() {
                StoreActivity::Installing
            } else {
                StoreActivity::Downloading(None)
            },
        );
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut last_percent = None;
            let mut on_progress = |bytes: u64, total: Option<u64>| {
                let percent = total
                    .filter(|size| *size > 0)
                    .map(|size| (bytes.saturating_mul(100) / size).min(100) as u8);
                if percent != last_percent {
                    last_percent = percent;
                    let _ = tx.send(StoreEvent::Progress(percent));
                }
            };
            let mut on_installing = || {
                let _ = tx.send(StoreEvent::Installing);
            };
            let result = install_or_download(
                &app,
                &asset,
                token.as_deref(),
                username.as_deref(),
                custom_host.as_deref(),
                &applications_dir,
                local_asset,
                &mut on_progress,
                &mut on_installing,
            );
            let _ = tx.send(StoreEvent::Finished(result));
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                let mut latest = None;
                while let Ok(event) = rx.try_recv() {
                    let done = matches!(event, StoreEvent::Finished(_));
                    latest = Some(event);
                    if done {
                        break;
                    }
                }
                match latest {
                    Some(StoreEvent::Progress(percent)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                this.store_activities
                                    .insert(key.clone(), StoreActivity::Downloading(percent));
                                cx.notify();
                            });
                        }
                    }
                    Some(StoreEvent::Installing) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                this.store_activities.insert(key.clone(), StoreActivity::Installing);
                                cx.notify();
                            });
                        }
                    }
                    Some(StoreEvent::Finished(result)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                this.store_activities.remove(&key);
                                this.refresh_store_install_states(cx);
                                this.message = match result {
                                    Ok(StoreOutcome::Installed) if was_installed =>
                                        format!("{app_name} is up to date."),
                                    Ok(StoreOutcome::Installed) =>
                                        format!("{app_name} is ready to open."),
                                    Ok(StoreOutcome::Downloaded) =>
                                        format!("{app_name} is downloaded. Open the file to finish installing."),
                                    Err(error) => {
                                        eprintln!("Store install failed for {app_name}: {error}");
                                        format!("Couldn't install {app_name}. Please try again.")
                                    }
                                };
                                cx.notify();
                            });
                        }
                        return;
                    }
                    None if matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                this.store_activities.remove(&key);
                                this.message = format!("Couldn't install {app_name}. Please try again.");
                                cx.notify();
                            });
                        }
                        return;
                    }
                    None => {}
                }
            }
        })
        .detach();
    }
}

fn merge_store_results(
    items: &mut Vec<StoreApp>,
    project: &crate::workspace::WorkspaceProject,
    repositories: &[RepositoryReleases],
) {
    let fresh = apps_for_project(project, repositories);
    merge_prepared_store_results(items, project, repositories, fresh);
}

fn merge_prepared_store_results(
    items: &mut Vec<StoreApp>,
    project: &crate::workspace::WorkspaceProject,
    repositories: &[RepositoryReleases],
    mut fresh: Vec<StoreApp>,
) {
    for repo in repositories.iter().filter(|repo| repo.result.is_err()) {
        fresh.extend(
            items
                .iter()
                .filter(|app| {
                    app.project_id == project.id
                        && app.provider == repo.provider
                        && app.owner == repo.owner
                        && app.repository == repo.repository
                })
                .cloned(),
        );
    }
    merge_project(items, project.id, fresh);
}

// Serialize writes and discard older queued snapshots so they cannot overwrite newer data.
fn save_store_background(path: std::path::PathBuf, items: Vec<StoreApp>) {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let Ok(_guard) = SAVE_LOCK.lock() else { return };
        if SEQUENCE.load(std::sync::atomic::Ordering::SeqCst) != sequence {
            return;
        }
        if let Err(error) = save(&path, &items) {
            eprintln!("Could not save store catalog: {error}");
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn install_or_download(
    app: &StoreApp,
    asset: &crate::store_catalog::StoreAsset,
    token: Option<&str>,
    username: Option<&str>,
    custom_host: Option<&str>,
    applications_dir: &std::path::Path,
    local_asset: Option<std::path::PathBuf>,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
    on_installing: &mut dyn FnMut(),
) -> Result<StoreOutcome, String> {
    let release = app
        .platform_release()
        .ok_or("No compatible release is available")?;
    let directory = &app.project_directory;
    let path = if let Some(path) = local_asset {
        path
    } else {
        match (app.provider.as_str(), token) {
            ("github", Some(token)) => {
                release_desk_store::workspace_releases::download_github_asset_with_progress(
                    directory,
                    &app.owner,
                    &app.repository,
                    &release.tag,
                    &asset.name,
                    asset.size,
                    token,
                    on_progress,
                )
            }
            ("gitlab", Some(token)) => {
                release_desk_store::workspace_releases::download_gitlab_asset_with_progress(
                    directory,
                    &app.owner,
                    &app.repository,
                    &release.tag,
                    &asset.name,
                    &asset.download_url,
                    asset.size,
                    token,
                    on_progress,
                )
            }
            (provider, Some(token)) => {
                release_desk_store::workspace_releases::download_provider_asset_with_progress(
                    directory,
                    provider,
                    &app.owner,
                    &app.repository,
                    &release.tag,
                    &asset.name,
                    &asset.download_url,
                    asset.size,
                    token,
                    username,
                    custom_host,
                    on_progress,
                )
            }
            (_, None) => release_desk_store::workspace_releases::download_asset_with_progress(
                directory,
                &app.owner,
                &app.repository,
                &release.tag,
                &asset.name,
                &asset.download_url,
                asset.size,
                on_progress,
            ),
        }?
    };
    if cfg!(target_os = "macos") && asset.name.to_ascii_lowercase().ends_with(".dmg") {
        on_installing();
        let installer = InstallMacOsAppUseCase::new(HdiutilAdapter, applications_dir.to_path_buf());
        installer
            .install_from_dmg(&path, &app.bundle_name)
            .map_err(|error| error.to_string())?;
        Ok(StoreOutcome::Installed)
    } else {
        Ok(StoreOutcome::Downloaded)
    }
}
