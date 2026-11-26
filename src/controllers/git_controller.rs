//! Git status checking, cloning, and local update controller.

use crate::app::ReleaseDesk;
use crate::controllers::job_registry::{
    CloneBatchSummary, ProjectJobKind, RepoClonePhase, RepoUpdate, RepoUpdateKind, StatusToast,
    WorkerDropGuard,
};
use crate::workspace::ProjectSection;
use crate::workspace_sources::RepoKey;
use gpui_kit::{Context, Window};
use std::collections::HashSet;

const CLONE_WORKERS: usize = 3;
const CLONE_PER_HOST_CAP: usize = 2;

/// The active Git operation shown in repository progress controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitActivity {
    Inspecting,
    Checking,
    Pulling,
}

#[derive(Clone, Debug)]
pub struct ProjectCacheStatus {
    pub project_id: u128,
    pub running: bool,
    pub recreate: bool,
    pub completed: usize,
    pub total: usize,
    pub detail: String,
    pub failed: bool,
}

enum ProjectCacheEvent {
    Progress(crate::modules::icon_agent::IconRefreshProgress),
    Finished(Result<(String, bool), String>),
}

impl ReleaseDesk {
    pub(crate) fn clear_selected_project_cache(&mut self, cx: &mut Context<Self>) {
        self.run_project_cache_action(false, cx);
    }

    pub(crate) fn recreate_selected_project_icons(&mut self, cx: &mut Context<Self>) {
        self.run_project_cache_action(true, cx);
    }

    fn run_project_cache_action(&mut self, recreate: bool, cx: &mut Context<Self>) {
        if self
            .project_cache_status
            .as_ref()
            .is_some_and(|status| status.running)
        {
            self.message = "An icon operation is already running. Wait for it to finish.".into();
            cx.notify();
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
            .cloned()
        else {
            return;
        };
        let project_id = project.id;
        self.project_cache_status = Some(ProjectCacheStatus {
            project_id,
            running: true,
            recreate,
            completed: 0,
            total: 0,
            detail: if recreate {
                "Finding cloned repositories…"
            } else {
                "Clearing icon cache…"
            }
            .into(),
            failed: false,
        });
        self.message = if recreate {
            "Recreating repository icons…"
        } else {
            "Clearing project cache…"
        }
        .into();
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = if recreate {
                crate::modules::icon_agent::recreate_project_icons_with_progress(
                    &project,
                    |progress| {
                        let _ = tx.send(ProjectCacheEvent::Progress(progress));
                    },
                )
                .map(|report| {
                    let failed = !report.failures.is_empty();
                    let message = if failed {
                        let examples = report
                            .failures
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" · ");
                        format!(
                            "Updated {} of {} repository icons; {} had no usable icon; {} failed. {}{}",
                            report.updated,
                            report.total,
                            report.skipped,
                            report.failures.len(),
                            examples,
                            if report.failures.len() > 3 {
                                " · More failures omitted"
                            } else {
                                ""
                            }
                        )
                    } else {
                        format!(
                            "Updated {} of {} repository icons; {} had no usable icon.",
                            report.updated, report.total, report.skipped
                        )
                    };
                    (message, failed)
                })
            } else {
                crate::modules::icon_agent::clear_project_cache(&project.directory)
                    .map(|()| ("Project cache cleared, including icons.".to_string(), false))
                    .map_err(|error| error.to_string())
            };
            let _ = tx.send(ProjectCacheEvent::Finished(result));
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(ProjectCacheEvent::Progress(progress)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this
                                    .project_cache_status
                                    .as_ref()
                                    .is_some_and(|status| status.project_id == project_id)
                                {
                                    if let Some(status) = &mut this.project_cache_status {
                                        if progress.completed < status.completed {
                                            return;
                                        }
                                        status.completed = progress.completed;
                                        status.total = progress.total;
                                        status.detail = if progress.repository.is_empty() {
                                            format!(
                                                "Scanning 0 of {} repositories…",
                                                progress.total
                                            )
                                        } else {
                                            format!(
                                                "Scanned {} of {} repositories · {}",
                                                progress.completed,
                                                progress.total,
                                                progress.repository
                                            )
                                        };
                                    }
                                    cx.notify();
                                }
                            });
                        }
                    }
                    Ok(ProjectCacheEvent::Finished(result)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this
                                    .project_cache_status
                                    .as_ref()
                                    .is_some_and(|status| status.project_id == project_id)
                                {
                                    match result {
                                        Ok((message, failed)) => {
                                            if this.selected_project_id == Some(project_id) {
                                                this.refresh_project_icon_views(project_id, cx);
                                            }
                                            if this.selected_project_id == Some(project_id) {
                                                this.message = message.clone();
                                            }
                                            if let Some(status) = &mut this.project_cache_status {
                                                status.running = false;
                                                status.failed = failed;
                                                status.detail = message;
                                            }
                                        }
                                        Err(error) => {
                                            let message =
                                                format!("Project cache action failed: {error}");
                                            if this.selected_project_id == Some(project_id) {
                                                this.message = message.clone();
                                            }
                                            if let Some(status) = &mut this.project_cache_status {
                                                status.running = false;
                                                status.failed = true;
                                                status.detail = message;
                                            }
                                        }
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
                                this.message = "Project cache action stopped unexpectedly.".into();
                                if let Some(status) = &mut this.project_cache_status {
                                    status.running = false;
                                    status.failed = true;
                                    status.detail = this.message.clone();
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

    pub(crate) fn refresh_project_icon_views(&mut self, project_id: u128, cx: &mut Context<Self>) {
        let Some(project) = self
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            return;
        };
        self.icon_revision = self.icon_revision.wrapping_add(1);
        let icon_revision = self.icon_revision;
        let workspace_revision = self.workspace_revision;
        let store_revision = self.store_revision;
        let projects = self.projects.clone();
        let mut store_items = self.store_items.clone();
        let release_keys = match &self.release_state {
            crate::workspace_releases::ReleaseViewState::Loaded { repositories, .. } => {
                repositories
                    .iter()
                    .map(|repo| {
                        (
                            repo.provider.clone(),
                            repo.owner.clone(),
                            repo.repository.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            }
            _ => Vec::new(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut icons = Vec::new();
            for source in &project.sources {
                for repo in &source.repositories {
                    let path = crate::modules::icon_agent::resolve_icon_for(
                        Some(&project.directory),
                        &source.provider,
                        crate::repository_git::repository_owner(source, repo),
                        &repo.name,
                    );
                    icons.push((project.repo_key(source, repo), path));
                }
            }
            let releases = release_keys
                .into_iter()
                .map(|(provider, owner, repository)| {
                    let path = crate::modules::icon_agent::resolve_icon_for(
                        Some(&project.directory),
                        &provider,
                        &owner,
                        &repository,
                    );
                    ((provider, owner, repository), path)
                })
                .collect::<Vec<_>>();
            crate::store_catalog::refresh_local_artwork(&mut store_items, &projects);
            let _ = tx.send((icons, releases, store_items));
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok((icons, releases, store_items)) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.icon_revision != icon_revision {
                                    return;
                                }
                                if this.projects.iter().any(|p| p.id == project_id)
                                    && this.workspace_revision == workspace_revision
                                {
                                    let updates = icons
                                        .into_iter()
                                        .map(|(key, icon)| (key, RepoUpdate::SetIcon(icon)))
                                        .collect();
                                    this.apply_repo_updates_batch(project_id, updates);
                                }
                                if this.selected_project_id == Some(project_id)
                                    && let crate::workspace_releases::ReleaseViewState::Loaded {
                                        repositories,
                                        presentations,
                                        ..
                                    } = &mut this.release_state
                                {
                                    for (repository, presentation) in
                                        repositories.iter().zip(presentations.iter_mut())
                                    {
                                        if let Some((_, path)) = releases.iter().find(|(key, _)| {
                                            key.0 == repository.provider
                                                && key.1 == repository.owner
                                                && key.2 == repository.repository
                                        }) {
                                            presentation.icon_path = path.clone();
                                        }
                                    }
                                }
                                if this.store_revision == store_revision {
                                    this.store_items = store_items;
                                    this.store_revision = this.store_revision.wrapping_add(1);
                                }
                                cx.notify();
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

    /// Single authoritative mutation path for per-repository state (updates memory, bumps
    /// per-repo version, checks D3 invariants in debug builds, and persists to SQLite).
    pub(crate) fn apply_repo_update(&mut self, project_id: u128, key: RepoKey, update: RepoUpdate) {
        self.apply_repo_updates_batch(project_id, vec![(key, update)]);
    }

    /// Batched version of [`Self::apply_repo_update`] so multi-repo checks and probes
    /// persist in a single SQLite transaction.
    pub(crate) fn apply_repo_updates_batch(
        &mut self,
        project_id: u128,
        updates: Vec<(RepoKey, RepoUpdate)>,
    ) {
        if updates.is_empty() {
            return;
        }
        let valid_keys = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .map(crate::workspace::WorkspaceProject::selected_repo_keys)
            .unwrap_or_default();
        let mut to_upsert = Vec::new();
        let mut to_remove = Vec::new();
        {
            let runtime = self.jobs.project_mut(project_id);
            for (key, update) in updates {
                let is_remove = matches!(update, RepoUpdate::Remove);
                if !is_remove && !valid_keys.contains(&key) {
                    continue;
                }
                if is_remove {
                    runtime.apply_repo_update_in_memory(&key, RepoUpdate::Remove);
                    to_remove.push(key);
                } else if let Some(rec) = runtime.apply_repo_update_in_memory(&key, update) {
                    to_upsert.push(rec);
                }
            }
        }
        if let Some(ref cache) = self.repo_cache {
            if !to_upsert.is_empty() {
                let _ = cache.upsert_repos_batch(&to_upsert);
            }
            for key in to_remove {
                let _ = cache.delete_repo(project_id, &key);
            }
        }
    }

    pub(crate) fn select_repository(&mut self, key: RepoKey, cx: &mut Context<Self>) {
        let Some(project_id) = self.selected_project_id else {
            self.editor_menu_open = false;
            cx.notify();
            return;
        };
        let needs_lazy_load = {
            let runtime = self.jobs.project_mut(project_id);
            if runtime.selected_repository.as_ref() != Some(&key) {
                self.repo_detail_scroll
                    .set_offset(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(0.)));
            }
            runtime.selected_repository = Some(key.clone());
            match runtime.repo(&key) {
                None => true,
                Some(entry) => entry.is_cloned() && entry.presentation().recent_commits.is_empty(),
            }
        };
        self.editor_menu_open = false;
        cx.notify();

        if needs_lazy_load
            && let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned()
            && let Ok(targets) = crate::repository_git::targets(&project)
            && let Some(target) = targets.into_iter().find(|t| t.key == key)
        {
            let existing_is_repo = self
                .jobs
                .project(project_id)
                .and_then(|r| r.repo(&key))
                .is_some_and(|e| {
                    e.record().destination == release_desk_git::DestinationKind::GitRepo
                });
            let icon_map =
                crate::modules::icon_agent::db::load_project_icons(Some(&project.directory));
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let load_commits_and_untracked = |path: &std::path::Path| {
                    let commits = crate::repository_git::load_recent_commits(path, 15)
                        .into_iter()
                        .map(|c| crate::modules::repo_cache::StoredCommit {
                            hash: c.hash,
                            short_hash: c.short_hash,
                            author_name: c.author_name,
                            date: c.date,
                            message: c.message,
                        })
                        .collect();
                    let has_untracked = release_desk_git::GitClient::new()
                        .with_timeout(std::time::Duration::from_secs(10))
                        .inspect_untracked_present(path)
                        .ok()
                        .map(|u| (u, crate::modules::repo_cache::now_unix_secs()));
                    (commits, has_untracked)
                };
                let update = if existing_is_repo {
                    let (commits, has_untracked) = load_commits_and_untracked(&target.path);
                    RepoUpdate::SetLazyDetail {
                        commits,
                        has_untracked,
                    }
                } else {
                    let mut rec = crate::repository_git::revalidate_or_inspect_target_with_policy(
                        project_id,
                        &target,
                        None,
                        false,
                        false,
                        &icon_map,
                        std::time::Duration::from_secs(10),
                    )
                    .0;
                    if rec.destination == release_desk_git::DestinationKind::GitRepo {
                        let (commits, has_untracked) = load_commits_and_untracked(&target.path);
                        rec.recent_commits = commits;
                        if let Some((u, ts)) = has_untracked {
                            rec.has_untracked = Some(u);
                            rec.has_untracked_at = Some(ts);
                        }
                    }
                    RepoUpdate::replace(rec, RepoUpdateKind::Inspect)
                };
                let _ = tx.send(update);
            });
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(60))
                        .await;
                    match rx.try_recv() {
                        Ok(update) => {
                            let Some(this) = this.upgrade() else {
                                return;
                            };
                            this.update(cx, |this, cx| {
                                this.apply_repo_update(project_id, key.clone(), update);
                                cx.notify();
                            });
                            return;
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                        Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    }
                }
            })
            .detach();
        }
    }

    pub(crate) fn schedule_revalidation(
        &mut self,
        project_id: u128,
        trigger: crate::modules::scheduler_service::TriggerKind,
        cx: &mut Context<Self>,
    ) {
        use crate::modules::remote_probe::{ProbeTarget, RemoteProbe as _, TieredRemoteProbe};
        use crate::modules::repo_cache::{CachedRepoRecord, StoredReleaseAsset, now_unix_secs};
        use crate::modules::scheduler_service::{
            TriggerKind, prioritize_probe_order, sort_slowest_first,
        };
        use std::collections::HashMap;

        let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() else {
            return;
        };
        let cooldown = if matches!(trigger, TriggerKind::Focus | TriggerKind::Interval) {
            self.scheduler_config.focus_cooldown
        } else {
            std::time::Duration::from_secs(30)
        };
        if !self
            .trigger_coalescer
            .request(project_id, trigger, cooldown)
        {
            return;
        }
        let job_kind = if trigger == TriggerKind::Focus {
            ProjectJobKind::Inspect
        } else {
            ProjectJobKind::Check
        };
        let Some((job_id, cancel)) = self.jobs.begin_job(project_id, job_kind) else {
            self.trigger_coalescer.complete(project_id, trigger);
            return;
        };
        self.scheduler_batches_started += 1;

        let auto_fetch = self.settings.is_auto_fetch_enabled(project_id);
        let auto_refresh_stale_index = self
            .settings
            .is_auto_refresh_stale_index_enabled(project_id);
        let secret_store = self.secret_store.clone();
        let volume_limiter = self.volume_limiter.clone();
        let cache = self.repo_cache.clone();

        let (existing_records, selected_key) = {
            let rt = self.jobs.project_mut(project_id);
            rt.activity = Some(if trigger == TriggerKind::Focus {
                GitActivity::Inspecting
            } else {
                GitActivity::Checking
            });
            rt.check_completed = 0;
            rt.check_total = 0;
            (rt.repo_records_cloned(), rt.selected_repository.clone())
        };

        let mut visible_keys: HashSet<RepoKey> = self
            .visible_repo_keys
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        if let Some(ref sel) = selected_key {
            visible_keys.insert(sel.clone());
        }
        let last_probed_map: HashMap<RepoKey, u64> = existing_records
            .iter()
            .filter_map(|(k, r)| r.last_probed_at.map(|t| (k.clone(), t)))
            .collect();

        let scoped_keys = visible_keys.clone();
        let filter_visible_only = matches!(trigger, TriggerKind::Focus | TriggerKind::Interval);

        if let Ok(mut initial_targets) = crate::repository_git::targets(&project) {
            if filter_visible_only {
                if !scoped_keys.is_empty() {
                    initial_targets.retain(|t| scoped_keys.contains(&t.key));
                } else {
                    initial_targets.truncate(6);
                }
            }
            let runtime = self.jobs.project_mut(project_id);
            runtime.check_total = initial_targets.len();
            runtime.pending = initial_targets.iter().map(|t| t.key.clone()).collect();
            cx.notify();
        }

        enum SchedEvent {
            Batch(Vec<(RepoKey, RepoUpdate)>),
            Error(String),
            Finished { ts: u64, offline: bool },
            Aborted,
        }
        let (tx, rx) = std::sync::mpsc::channel::<SchedEvent>();
        let revision = self.workspace_revision;
        let _ = revision;

        std::thread::spawn(move || {
            let tx_guard = tx.clone();
            let mut guard = WorkerDropGuard::new(move || {
                let _ = tx_guard.send(SchedEvent::Aborted);
            });

            let mut targets = match crate::repository_git::targets(&project) {
                Ok(t) => t,
                Err(err) => {
                    guard.disarm();
                    let _ = tx.send(SchedEvent::Error(err));
                    return;
                }
            };
            if filter_visible_only {
                if !scoped_keys.is_empty() {
                    targets.retain(|t| scoped_keys.contains(&t.key));
                } else {
                    targets.truncate(6);
                }
            }

            if targets.is_empty() {
                guard.disarm();
                let _ = tx.send(SchedEvent::Finished {
                    ts: now_unix_secs(),
                    offline: false,
                });
                return;
            }

            // Load project icons with ONE SQLite open + ONE query (~0.8 ms).
            let icon_map =
                crate::modules::icon_agent::db::load_project_icons(Some(&project.directory));

            // Order slowest/largest repos first for optimal HDD/SSD utilization.
            sort_slowest_first(&mut targets, |t| t.path.clone());
            let check_dirty = matches!(trigger, TriggerKind::Focus | TriggerKind::ManualCheck);
            let status_timeout = std::time::Duration::from_secs(10);

            let local_records: Vec<CachedRepoRecord> = volume_limiter.run_on_volume_with_paths(
                &project.directory,
                cache.as_deref(),
                &targets,
                |t| Some(t.path.as_path()),
                |target| {
                    if cancel.load(std::sync::atomic::Ordering::Acquire) {
                        let fallback =
                            existing_records
                                .get(&target.key)
                                .cloned()
                                .unwrap_or_else(|| {
                                    crate::repository_git::revalidate_or_inspect_target(
                                        project_id,
                                        target,
                                        None,
                                        false,
                                        &icon_map,
                                        status_timeout,
                                    )
                                    .0
                                });
                        return (fallback, false);
                    }
                    let (rec, timed_out) =
                        crate::repository_git::revalidate_or_inspect_target_with_policy(
                            project_id,
                            target,
                            existing_records.get(&target.key),
                            check_dirty || Some(&target.key) == selected_key.as_ref(),
                            auto_refresh_stale_index,
                            &icon_map,
                            status_timeout,
                        );
                    (rec, timed_out)
                },
            );

            let by_key: HashMap<RepoKey, CachedRepoRecord> = local_records
                .iter()
                .map(|r| (r.key.clone(), r.clone()))
                .collect();
            let local_updates: Vec<(RepoKey, RepoUpdate)> = local_records
                .into_iter()
                .map(|rec| {
                    (
                        rec.key.clone(),
                        RepoUpdate::replace(rec, RepoUpdateKind::Revalidate),
                    )
                })
                .collect();
            let _ = tx.send(SchedEvent::Batch(local_updates));

            let mut any_offline = false;
            if matches!(
                trigger,
                TriggerKind::Startup | TriggerKind::Interval | TriggerKind::ManualCheck
            ) && !cancel.load(std::sync::atomic::Ordering::Acquire)
            {
                let ordered_keys = prioritize_probe_order(
                    &targets.iter().map(|t| t.key.clone()).collect::<Vec<_>>(),
                    selected_key.as_ref(),
                    &visible_keys,
                    &last_probed_map,
                );
                let target_map: HashMap<RepoKey, _> =
                    targets.into_iter().map(|t| (t.key.clone(), t)).collect();
                let ordered_targets: Vec<_> = ordered_keys
                    .into_iter()
                    .filter_map(|k| target_map.get(&k).cloned())
                    .collect();

                if auto_fetch {
                    let fetched = crate::modules::parallel::map_bounded_per_host(
                        &ordered_targets,
                        8,
                        8,
                        |t| t.key.host.clone(),
                        crate::repository_git::check_for_updates,
                    );
                    let fetched_icon_map = HashMap::new();
                    let mut updates = Vec::new();
                    for st in fetched {
                        let key = st.key.clone();
                        if let Some(target) = target_map.get(&key) {
                            let rec = crate::repository_git::record_from_status(
                                project_id,
                                target,
                                &st,
                                by_key.get(&key),
                                &fetched_icon_map,
                                selected_key.as_ref() == Some(&key),
                            );
                            updates.push((key, RepoUpdate::replace(rec, RepoUpdateKind::Fetch)));
                        }
                    }
                    if !updates.is_empty() {
                        let _ = tx.send(SchedEvent::Batch(updates));
                    }
                } else {
                    let probe_targets: Vec<ProbeTarget> = ordered_targets
                        .iter()
                        .filter(|t| {
                            by_key
                                .get(&t.key)
                                .is_some_and(|r| r.destination.is_git_repo())
                        })
                        .map(|t| ProbeTarget {
                            key: t.key.clone(),
                            clone_url: t.url.clone(),
                            default_branch: t.branch.clone(),
                            credential_ref: t.credential_ref.clone(),
                            endpoint: t.endpoint.clone(),
                        })
                        .collect();
                    let token = crate::app::project_auth_token(&project, &secret_store)
                        .ok()
                        .flatten();
                    let probe = TieredRemoteProbe::new();
                    let results = probe.probe_batch(&probe_targets, token.as_deref(), &cancel);
                    let now = now_unix_secs();
                    let all_offline = !results.is_empty() && results.iter().all(|r| r.offline);
                    any_offline = all_offline;
                    let mut updates = Vec::new();
                    for res in results {
                        if res.offline {
                            updates.push((res.key, RepoUpdate::MarkOffline));
                            continue;
                        }
                        let latest_release = res.latest_release_tag.map(|tag| {
                            (
                                tag,
                                res.latest_release_date,
                                res.latest_release_assets
                                    .into_iter()
                                    .map(|a| StoredReleaseAsset {
                                        name: a.name,
                                        download_url: a.download_url,
                                        size: a.size,
                                    })
                                    .collect(),
                            )
                        });
                        updates.push((
                            res.key,
                            RepoUpdate::ApplyProbe {
                                remote_sha: res.remote_sha,
                                failed_reason: res.error,
                                issue: res.issue,
                                latest_release,
                                probed_at: now,
                            },
                        ));
                    }
                    if !updates.is_empty() {
                        let _ = tx.send(SchedEvent::Batch(updates));
                    }
                }
            }

            guard.disarm();
            let _ = tx.send(SchedEvent::Finished {
                ts: now_unix_secs(),
                offline: any_offline,
            });
        });

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                let mut events = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
                let disconnected = matches!(
                    rx.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Disconnected)
                );
                if !events.is_empty() || disconnected {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let mut should_exit = false;
                    this.update(cx, |this, cx| {
                        let valid_keys = this
                            .projects
                            .iter()
                            .find(|p| p.id == project_id)
                            .map(crate::workspace::WorkspaceProject::selected_repo_keys)
                            .unwrap_or_default();
                        for ev in events {
                            match ev {
                                SchedEvent::Batch(updates) => {
                                    {
                                        let runtime = this.jobs.project_mut(project_id);
                                        for (key, _) in &updates {
                                            if valid_keys.contains(key) {
                                                runtime.pending.remove(key);
                                            }
                                        }
                                        runtime.check_completed = runtime
                                            .check_total
                                            .saturating_sub(runtime.pending.len());
                                    }
                                    this.apply_repo_updates_batch(project_id, updates);
                                }
                                SchedEvent::Error(err) => {
                                    this.jobs.finish_job(project_id, job_id, job_kind);
                                    this.trigger_coalescer.complete(project_id, trigger);
                                    if this.selected_project_id == Some(project_id) {
                                        this.set_toast(StatusToast::error(err));
                                    }
                                    should_exit = true;
                                }
                                SchedEvent::Finished { ts, offline } => {
                                    this.jobs.finish_job(project_id, job_id, job_kind);
                                    {
                                        let rt = this.jobs.project_mut(project_id);
                                        rt.offline = offline;
                                        if !offline {
                                            rt.last_checked_at = Some(ts);
                                        }
                                    }
                                    this.trigger_coalescer.complete(project_id, trigger);
                                    if this.selected_project_id == Some(project_id) {
                                        this.message = if offline {
                                            "Offline — showing cached state."
                                        } else {
                                            "Repository check finished."
                                        }
                                        .into();
                                    }
                                    should_exit = true;
                                }
                                SchedEvent::Aborted => {
                                    this.jobs.finish_job(project_id, job_id, job_kind);
                                    this.trigger_coalescer.complete(project_id, trigger);
                                    should_exit = true;
                                }
                            }
                        }
                        if disconnected && !should_exit {
                            this.jobs.finish_job(project_id, job_id, job_kind);
                            this.trigger_coalescer.complete(project_id, trigger);
                            should_exit = true;
                        }
                        cx.notify();
                    });
                    if should_exit {
                        return;
                    }
                }
            }
        })
        .detach();
    }

    pub(crate) fn check_repository_updates(&mut self, local_only: bool, cx: &mut Context<Self>) {
        if !local_only {
            if let Some(pid) = self.selected_project_id {
                self.schedule_revalidation(
                    pid,
                    crate::modules::scheduler_service::TriggerKind::ManualCheck,
                    cx,
                );
            }
            return;
        }
        self.run_repository_operation(GitActivity::Inspecting, None, cx);
    }

    pub(crate) fn inspect_repo_keys(
        &mut self,
        project_id: u128,
        keys: Vec<RepoKey>,
        cx: &mut Context<Self>,
    ) {
        if keys.is_empty() {
            return;
        }
        let filter: HashSet<RepoKey> = keys.into_iter().collect();
        self.run_repository_operation_for_project(
            project_id,
            GitActivity::Inspecting,
            Some(filter),
            false,
            cx,
        );
    }

    pub(crate) fn pull_repository_updates(
        &mut self,
        key: RepoKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enqueue_repository_pull(key, window, cx);
    }

    pub(crate) fn pull_all_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.enqueue_available_repository_pulls(window, cx);
    }

    pub(crate) fn check_one_repository(&mut self, key: RepoKey, cx: &mut Context<Self>) {
        let filter = HashSet::from([key]);
        self.run_repository_operation(GitActivity::Checking, Some(filter), cx);
    }

    fn run_repository_operation(
        &mut self,
        activity: GitActivity,
        filter: Option<HashSet<RepoKey>>,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.selected_project_id else {
            return;
        };
        let auto_check_after = false;
        self.run_repository_operation_for_project(
            project_id,
            activity,
            filter,
            auto_check_after,
            cx,
        );
    }

    fn run_repository_operation_for_project(
        &mut self,
        project_id: u128,
        activity: GitActivity,
        filter: Option<HashSet<RepoKey>>,
        auto_check_after: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() else {
            return;
        };
        let kind = match activity {
            GitActivity::Inspecting => ProjectJobKind::Inspect,
            GitActivity::Checking | GitActivity::Pulling => ProjectJobKind::Check,
        };
        let Some((job_id, cancel)) = self.jobs.begin_job(project_id, kind) else {
            return;
        };
        {
            let runtime = self.jobs.project_mut(project_id);
            runtime.activity = Some(activity);
            runtime.check_completed = 0;
            runtime.check_total = 0;
        }
        let revision = self.workspace_revision;
        self.message = "Preparing repositories…".into();
        cx.notify();

        enum OpEvent {
            Icon(RepoKey, Option<std::path::PathBuf>),
            Row(Box<crate::modules::repo_cache::CachedRepoRecord>),
            TargetsError(String),
            Finished(Option<String>),
            Aborted,
        }

        let (tx, rx) = std::sync::mpsc::channel::<OpEvent>();
        let cached_records = self
            .jobs
            .project(project_id)
            .map(|r| r.repo_records_cloned())
            .unwrap_or_default();
        let selected_repo = self
            .jobs
            .project(project_id)
            .and_then(|r| r.selected_repository.clone());
        let pull_eligible: HashSet<RepoKey> = if activity == GitActivity::Pulling {
            self.jobs
                .project(project_id)
                .map(|r| {
                    r.repos()
                        .values()
                        .filter(|e| e.can_pull())
                        .map(|e| e.record().key.clone())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            HashSet::new()
        };

        // Pre-populate pending keys from the current project target list synchronously.
        if let Ok(targets) = crate::repository_git::targets(&project) {
            let matching: Vec<RepoKey> = targets
                .into_iter()
                .filter(|t| filter.as_ref().is_none_or(|f| f.contains(&t.key)))
                .filter(|t| activity != GitActivity::Pulling || pull_eligible.contains(&t.key))
                .map(|t| t.key)
                .collect();
            let runtime = self.jobs.project_mut(project_id);
            runtime.check_total = matching.len();
            runtime.pending = matching.into_iter().collect();
            self.message = match activity {
                GitActivity::Inspecting => "Inspecting local repositories…",
                GitActivity::Checking => "Checking repositories for updates…",
                GitActivity::Pulling => "Pulling repository updates…",
            }
            .into();
            cx.notify();
        }

        std::thread::spawn(move || {
            let tx_guard = tx.clone();
            let mut guard = WorkerDropGuard::new(move || {
                let _ = tx_guard.send(OpEvent::Aborted);
            });

            let all_targets = match crate::repository_git::targets(&project) {
                Ok(t) => t,
                Err(error) => {
                    guard.disarm();
                    let _ = tx.send(OpEvent::TargetsError(error));
                    return;
                }
            };
            let mut targets: Vec<_> = all_targets
                .into_iter()
                .filter(|target| filter.as_ref().is_none_or(|f| f.contains(&target.key)))
                .filter(|target| {
                    activity != GitActivity::Pulling || pull_eligible.contains(&target.key)
                })
                .collect();

            if targets.is_empty() {
                guard.disarm();
                let msg = if activity == GitActivity::Pulling {
                    Some("No repositories have confirmed updates. Check for updates first.".into())
                } else {
                    None
                };
                let _ = tx.send(OpEvent::Finished(msg));
                return;
            }

            let icon_map =
                crate::modules::icon_agent::db::load_project_icons(Some(&project.directory));
            if activity == GitActivity::Inspecting {
                for target in &targets {
                    let icon_key = (
                        target.provider.to_ascii_lowercase(),
                        target.key.owner.to_ascii_lowercase(),
                        target.name.to_ascii_lowercase(),
                    );
                    if let Some(path) = icon_map.get(&icon_key) {
                        let _ = tx.send(OpEvent::Icon(target.key.clone(), Some(path.clone())));
                    }
                }
                crate::modules::scheduler_service::sort_slowest_first(&mut targets, |t| {
                    t.path.clone()
                });
            }

            let messages = crate::modules::parallel::map_bounded_per_host(
                &targets,
                if activity == GitActivity::Inspecting {
                    crate::modules::scheduler_service::INITIAL_VOLUME_CONCURRENCY
                } else {
                    crate::modules::parallel::IO_WORKERS
                },
                if activity == GitActivity::Inspecting {
                    crate::modules::scheduler_service::INITIAL_VOLUME_CONCURRENCY
                } else {
                    8
                },
                |target| target.key.host.clone(),
                |target| {
                    if cancel.load(std::sync::atomic::Ordering::Acquire) {
                        return None;
                    }
                    let (status, result) = match activity {
                        GitActivity::Inspecting => (crate::repository_git::inspect(target), None),
                        GitActivity::Checking => {
                            (crate::repository_git::check_for_updates(target), None)
                        }
                        GitActivity::Pulling => {
                            match crate::repository_git::pull_existing(target) {
                                Ok(outcome) => {
                                    let (message, status) = match outcome {
                                        release_desk_git::SyncOutcome::FastForwarded { .. } => {
                                            ("updated successfully".to_string(), None)
                                        }
                                        release_desk_git::SyncOutcome::AlreadyCurrent => {
                                            ("already up to date".to_string(), None)
                                        }
                                        release_desk_git::SyncOutcome::Cloned { .. } => {
                                            ("cloned".to_string(), None)
                                        }
                                        release_desk_git::SyncOutcome::Skipped(state) => {
                                            let message =
                                                format!("not updated ({:?})", state.status);
                                            (
                                                message,
                                                Some(crate::repository_git::RepositoryStatus {
                                                    key: target.key.clone(),
                                                    destination:
                                                        release_desk_git::classify_destination(
                                                            &target.project_root,
                                                            &target.path,
                                                        ),
                                                    state: Ok(state),
                                                    fetched: false,
                                                }),
                                            )
                                        }
                                    };
                                    let status = status.unwrap_or_else(|| {
                                        let mut inspected = crate::repository_git::inspect(target);
                                        inspected.fetched = inspected.state.is_ok();
                                        inspected
                                    });
                                    let succeeded = status.state.is_ok() && status.fetched;
                                    (
                                        status,
                                        Some((succeeded, format!("{}: {message}", target.name))),
                                    )
                                }
                                Err(error) => (
                                    crate::repository_git::RepositoryStatus {
                                        key: target.key.clone(),
                                        destination: release_desk_git::classify_destination(
                                            &target.project_root,
                                            &target.path,
                                        ),
                                        state: Err(error.clone()),
                                        fetched: false,
                                    },
                                    Some((false, format!("{}: {error}", target.name))),
                                ),
                            }
                        }
                    };
                    let load_commits = activity == GitActivity::Pulling
                        || selected_repo.as_ref() == Some(&target.key)
                        || targets.len() == 1;
                    let rec = crate::repository_git::record_from_status(
                        project_id,
                        target,
                        &status,
                        cached_records.get(&target.key),
                        &icon_map,
                        load_commits,
                    );
                    let _ = tx.send(OpEvent::Row(Box::new(rec)));
                    result
                },
            );

            let results: Vec<_> = messages.into_iter().flatten().collect();
            let message = if activity == GitActivity::Pulling && results.len() > 1 {
                let succeeded = results.iter().filter(|(success, _)| *success).count();
                Some(format!(
                    "Pulled {succeeded} repositories; {} need attention.",
                    results.len() - succeeded
                ))
            } else {
                results.into_iter().next().map(|(_, message)| message)
            };
            guard.disarm();
            let _ = tx.send(OpEvent::Finished(message));
        });

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                let mut events = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
                let disconnected = matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected));
                if !events.is_empty() || disconnected {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let mut should_exit = false;
                    this.update(cx, |this, cx| {
                        let valid_keys = this
                            .projects
                            .iter()
                            .find(|p| p.id == project_id)
                            .map(crate::workspace::WorkspaceProject::selected_repo_keys)
                            .unwrap_or_default();
                        let stale_revision = this.workspace_revision != revision;

                        for ev in events {
                            match ev {
                                OpEvent::Icon(key, icon) => {
                                    if !stale_revision && valid_keys.contains(&key) {
                                        this.apply_repo_update(
                                            project_id,
                                            key,
                                            RepoUpdate::SetIcon(icon),
                                        );
                                    }
                                }
                                OpEvent::Row(rec) => {
                                    let rec = *rec;
                                    {
                                        let runtime = this.jobs.project_mut(project_id);
                                        runtime.check_completed += 1;
                                        runtime.pending.remove(&rec.key);
                                    }
                                    if valid_keys.contains(&rec.key) {
                                        let update_kind = match activity {
                                            GitActivity::Inspecting => RepoUpdateKind::Inspect,
                                            GitActivity::Checking => RepoUpdateKind::Check,
                                            GitActivity::Pulling => RepoUpdateKind::Pull,
                                        };
                                        let rkey = rec.key.clone();
                                        this.apply_repo_update(
                                            project_id,
                                            rkey,
                                            RepoUpdate::replace(rec, update_kind),
                                        );
                                    }
                                }
                                OpEvent::TargetsError(error) => {
                                    this.jobs.finish_job(project_id, job_id, kind);
                                    if this.selected_project_id == Some(project_id) {
                                        this.set_toast(StatusToast::error(error));
                                    }
                                    should_exit = true;
                                }
                                OpEvent::Finished(message) => {
                                    this.jobs.finish_job(project_id, job_id, kind);
                                    if matches!(
                                        activity,
                                        GitActivity::Checking | GitActivity::Pulling
                                    ) {
                                        let runtime = this.jobs.project_mut(project_id);
                                        runtime.last_checked_at =
                                            Some(crate::modules::repo_cache::now_unix_secs());
                                    }
                                    if this.selected_project_id == Some(project_id) {
                                        if let Some(msg) = message {
                                            this.message = msg;
                                        } else {
                                            this.message = match activity {
                                                GitActivity::Inspecting => {
                                                    "Local repository inspection finished."
                                                }
                                                GitActivity::Checking => {
                                                    "Repository check finished. See each repository for its status."
                                                }
                                                GitActivity::Pulling => {
                                                    "Repository pulls finished. See each repository for its status."
                                                }
                                            }
                                            .into();
                                        }
                                    }
                                    if auto_check_after && !stale_revision {
                                        this.check_repository_updates(false, cx);
                                    }
                                    should_exit = true;
                                }
                                OpEvent::Aborted => {
                                    this.jobs.finish_job(project_id, job_id, kind);
                                    if this.selected_project_id == Some(project_id) {
                                        this.set_toast(StatusToast::error(
                                            "Repository check stopped unexpectedly.",
                                        ));
                                    }
                                    should_exit = true;
                                }
                            }
                        }
                        if disconnected && !should_exit {
                            this.jobs.finish_job(project_id, job_id, kind);
                            should_exit = true;
                        }
                        cx.notify();
                    });
                    if should_exit {
                        return;
                    }
                }
            }
        })
        .detach();
    }

    pub(crate) fn execute_clone_plan(
        &mut self,
        source_index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        self.source_dialog = crate::views::project_sources::SourceDialog::Closed;
        self.project_section = ProjectSection::Repositories;
        let targets: Vec<_> = match crate::repository_git::targets(&project) {
            Ok(all) => all
                .into_iter()
                .filter(|target| {
                    target.source_index == source_index
                        && release_desk_git::classify_destination(
                            &target.project_root,
                            &target.path,
                        )
                        .is_clonable()
                })
                .collect(),
            Err(error) => {
                self.set_toast(StatusToast::error(error));
                cx.notify();
                return;
            }
        };
        self.start_clone_jobs(project, targets, false, cx);
    }

    pub(crate) fn clone_single_repository(
        &mut self,
        key: RepoKey,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let targets: Vec<_> = match crate::repository_git::targets(&project) {
            Ok(all) => all.into_iter().filter(|t| t.key == key).collect(),
            Err(error) => {
                self.set_toast(StatusToast::error(error));
                cx.notify();
                return;
            }
        };
        self.start_clone_jobs(project, targets, false, cx);
    }

    pub(crate) fn retry_repository_clone(
        &mut self,
        key: RepoKey,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let targets: Vec<_> = match crate::repository_git::targets(&project) {
            Ok(all) => all.into_iter().filter(|t| t.key == key).collect(),
            Err(error) => {
                self.set_toast(StatusToast::error(error));
                cx.notify();
                return;
            }
        };
        self.start_clone_jobs(project, targets, true, cx);
    }

    pub(crate) fn clone_all_missing_repositories(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let targets: Vec<_> = match crate::repository_git::targets(&project) {
            Ok(all) => all
                .into_iter()
                .filter(|target| {
                    release_desk_git::classify_destination(&target.project_root, &target.path)
                        .is_clonable()
                })
                .collect(),
            Err(error) => {
                self.set_toast(StatusToast::error(error));
                cx.notify();
                return;
            }
        };
        self.start_clone_jobs(project, targets, false, cx);
    }

    fn start_clone_jobs(
        &mut self,
        project: crate::workspace::WorkspaceProject,
        targets: Vec<crate::repository_git::RepositoryTarget>,
        allow_repair_empty_git: bool,
        cx: &mut Context<Self>,
    ) {
        let project_id = project.id;
        if targets.is_empty() {
            self.set_toast(StatusToast::info("No selected repositories need cloning."));
            cx.notify();
            return;
        }
        let Some((job_id, cancel)) = self.jobs.begin_job(project_id, ProjectJobKind::Clone) else {
            self.set_toast(StatusToast::info(
                "A clone operation is already running for this project.",
            ));
            cx.notify();
            return;
        };
        {
            let runtime = self.jobs.project_mut(project_id);
            for target in &targets {
                runtime
                    .clone_states
                    .insert(target.key.clone(), RepoClonePhase::Queued);
            }
        }
        self.message = format!("Cloning {} repositories…", targets.len());
        cx.notify();

        enum CloneEvent {
            Phase(RepoKey, RepoClonePhase),
            Finished(CloneBatchSummary),
            Aborted,
        }

        let (tx, rx) = std::sync::mpsc::channel::<CloneEvent>();
        std::thread::spawn(move || {
            let tx_guard = tx.clone();
            let mut guard = WorkerDropGuard::new(move || {
                let _ = tx_guard.send(CloneEvent::Aborted);
            });

            let outcomes = crate::modules::parallel::map_bounded_per_host(
                &targets,
                CLONE_WORKERS,
                CLONE_PER_HOST_CAP,
                |target| target.key.host.clone(),
                |target| {
                    if cancel.load(std::sync::atomic::Ordering::Acquire) {
                        let _ = tx.send(CloneEvent::Phase(
                            target.key.clone(),
                            RepoClonePhase::Cancelled,
                        ));
                        return (target.key.clone(), Err("Cancelled".to_string()), true);
                    }
                    let _ = tx.send(CloneEvent::Phase(
                        target.key.clone(),
                        RepoClonePhase::Cloning {
                            percent: Some(0),
                            detail: "Starting git clone…".into(),
                        },
                    ));
                    let res = crate::repository_git::clone_target_with_progress(
                        target,
                        allow_repair_empty_git,
                        &cancel,
                        |progress| {
                            let _ = tx.send(CloneEvent::Phase(
                                target.key.clone(),
                                RepoClonePhase::Cloning {
                                    percent: progress.percent,
                                    detail: progress.detail,
                                },
                            ));
                        },
                    );
                    let _ = tx.send(CloneEvent::Phase(
                        target.key.clone(),
                        RepoClonePhase::Verifying,
                    ));
                    match res {
                        Ok(release_desk_git::SyncOutcome::Cloned { .. }) => {
                            let _ = tx
                                .send(CloneEvent::Phase(target.key.clone(), RepoClonePhase::Done));
                            (target.key.clone(), Ok(()), false)
                        }
                        Ok(release_desk_git::SyncOutcome::Skipped(state)) => {
                            let err = state.message.unwrap_or_else(|| {
                                format!("clone needs attention ({:?})", state.status)
                            });
                            let _ = tx.send(CloneEvent::Phase(
                                target.key.clone(),
                                RepoClonePhase::Failed(err.clone()),
                            ));
                            (target.key.clone(), Err(err), false)
                        }
                        Ok(_) => {
                            let _ = tx
                                .send(CloneEvent::Phase(target.key.clone(), RepoClonePhase::Done));
                            (target.key.clone(), Ok(()), false)
                        }
                        Err(error) => {
                            let cancelled = cancel.load(std::sync::atomic::Ordering::Acquire)
                                && error == "Git operation cancelled";
                            if cancelled {
                                let _ = tx.send(CloneEvent::Phase(
                                    target.key.clone(),
                                    RepoClonePhase::Cancelled,
                                ));
                                (target.key.clone(), Err(error), true)
                            } else {
                                let _ = tx.send(CloneEvent::Phase(
                                    target.key.clone(),
                                    RepoClonePhase::Failed(error.clone()),
                                ));
                                (target.key.clone(), Err(error), false)
                            }
                        }
                    }
                },
            );

            let mut summary = CloneBatchSummary {
                total: outcomes.len(),
                ..Default::default()
            };
            let mut cloned_targets = Vec::new();
            for (target, (key, res, cancelled)) in targets.iter().zip(outcomes) {
                if cancelled {
                    summary.cancelled.push(key);
                } else if let Err(err) = res {
                    summary.failed.push((key, err));
                } else {
                    summary.succeeded.push(key);
                    cloned_targets.push(target.clone());
                }
            }
            if !cloned_targets.is_empty() {
                let _ = crate::modules::parallel::map_bounded(
                    &cloned_targets,
                    crate::modules::parallel::IO_WORKERS,
                    |target| crate::modules::icon_agent::generate_for_target(&project, target),
                );
            }

            guard.disarm();
            let _ = tx.send(CloneEvent::Finished(summary));
        });

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let mut events = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
                let disconnected = matches!(
                    rx.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Disconnected)
                );
                if !events.is_empty() || disconnected {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let mut should_exit = false;
                    this.update(cx, |this, cx| {
                        for ev in events {
                            match ev {
                                CloneEvent::Phase(key, phase) => {
                                    this.jobs
                                        .project_mut(project_id)
                                        .clone_states
                                        .insert(key, phase);
                                }
                                CloneEvent::Finished(summary) => {
                                    this.jobs
                                        .finish_job(project_id, job_id, ProjectJobKind::Clone);
                                    let succeeded_keys = summary.succeeded.clone();
                                    let toast = summary.to_toast();
                                    if this.selected_project_id == Some(project_id) {
                                        this.set_toast(toast);
                                    }
                                    if !succeeded_keys.is_empty() {
                                        this.refresh_project_icon_views(project_id, cx);
                                        this.inspect_repo_keys(project_id, succeeded_keys, cx);
                                    }
                                    should_exit = true;
                                }
                                CloneEvent::Aborted => {
                                    this.jobs
                                        .finish_job(project_id, job_id, ProjectJobKind::Clone);
                                    if this.selected_project_id == Some(project_id) {
                                        this.set_toast(StatusToast::error(
                                            "Clone operation stopped unexpectedly.",
                                        ));
                                    }
                                    should_exit = true;
                                }
                            }
                        }
                        if disconnected && !should_exit {
                            this.jobs
                                .finish_job(project_id, job_id, ProjectJobKind::Clone);
                            should_exit = true;
                        }
                        cx.notify();
                    });
                    if should_exit {
                        return;
                    }
                }
            }
        })
        .detach();
    }

    pub(crate) fn import_local_repositories(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_import_dialog(window, cx);
    }

    /// Toggles opt-in `auto_fetch` (full git fetch during scheduled checks) for one project.
    pub(crate) fn toggle_auto_fetch(&mut self, project_id: u128, cx: &mut Context<Self>) {
        if !self.settings.auto_fetch_projects.remove(&project_id) {
            self.settings.auto_fetch_projects.insert(project_id);
        }
        let _ = crate::settings::save(&self.settings);
        cx.notify();
    }

    /// Toggles opt-in automatic index refresh (at most once per 6h per repo when stale).
    pub(crate) fn toggle_auto_refresh_stale_index(
        &mut self,
        project_id: u128,
        cx: &mut Context<Self>,
    ) {
        if !self
            .settings
            .auto_refresh_stale_index_projects
            .remove(&project_id)
        {
            self.settings
                .auto_refresh_stale_index_projects
                .insert(project_id);
        }
        let _ = crate::settings::save(&self.settings);
        cx.notify();
    }

    /// Refresh `.git/index` stat cache ONLY if no `index.lock` exists, at low I/O priority.
    pub(crate) fn refresh_repository_index(&mut self, key: RepoKey, cx: &mut Context<Self>) {
        let Some(project_id) = self.selected_project_id else {
            return;
        };
        let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() else {
            return;
        };
        let Ok(targets) = crate::repository_git::targets(&project) else {
            return;
        };
        let Some(target) = targets.into_iter().find(|t| t.key == key) else {
            return;
        };
        let path = target.path.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = release_desk_git::GitClient::new()
                .refresh_index_if_unlocked(&path)
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(60))
                    .await;
                if let Ok(res) = rx.try_recv() {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    this.update(cx, |this, cx| {
                        match res {
                            Ok(true) => {
                                this.apply_repo_update(
                                    project_id,
                                    key.clone(),
                                    RepoUpdate::ClearStaleIndex {
                                        refreshed_at: crate::modules::repo_cache::now_unix_secs(),
                                    },
                                );
                                this.set_toast(StatusToast::success(
                                    "Refreshed Git index stat cache.",
                                ));
                            }
                            Ok(false) => {
                                this.set_toast(StatusToast::warning(
                                    "Skipped index refresh: .git/index.lock is active.",
                                ));
                            }
                            Err(err) => {
                                this.set_toast(StatusToast::error(format!(
                                    "Index refresh failed: {err}"
                                )));
                            }
                        }
                        cx.notify();
                    });
                    return;
                }
            }
        })
        .detach();
    }

    /// Explicitly remove stale lock files ONLY via confirmation dialog (`RepositoryAction::RemoveStaleLocks`).
    pub(crate) fn remove_repository_stale_locks(&mut self, key: RepoKey, cx: &mut Context<Self>) {
        let Some(project_id) = self.selected_project_id else {
            return;
        };
        let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() else {
            return;
        };
        let Ok(targets) = crate::repository_git::targets(&project) else {
            return;
        };
        let Some(target) = targets.into_iter().find(|t| t.key == key) else {
            return;
        };
        match release_desk_git::remove_stale_locks_explicit(&target.path) {
            Ok(count) => {
                self.apply_repo_update(project_id, key.clone(), RepoUpdate::ClearStaleLocks);
                self.set_toast(StatusToast::success(format!(
                    "Removed {count} stale lock file(s)."
                )));
            }
            Err(err) => {
                self.set_toast(StatusToast::error(format!(
                    "Failed to remove stale locks: {err}"
                )));
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod phase3_clone_and_state_tests {
    use super::*;
    use crate::controllers::job_registry::{
        CloneBatchSummary, JobRegistry, ProjectJobKind, ToastSeverity, WorkerDropGuard,
    };
    use crate::workspace::{ProjectSection, ProviderChoice, WorkspaceProject};
    use crate::workspace_sources::{
        RepoKey, RepositoryListMode, SourceRepository, WorkspaceSource,
    };
    use gpui_kit::{AppContext as _, TestAppContext, VisualTestContext};
    use release_desk_core::project::DiskLayout;
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    pub(super) fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git installed");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    pub(super) fn init_remote(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "test@example.invalid"]);
        git(dir, &["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "hi\n").unwrap();
        git(dir, &["add", "README.md"]);
        git(dir, &["commit", "-m", "init"]);
    }

    pub(super) fn test_project(root: &Path, repos: Vec<SourceRepository>) -> WorkspaceProject {
        let dir = root.join("project");
        std::fs::create_dir_all(&dir).unwrap();
        WorkspaceProject {
            id: 999_001,
            name: "Phase3Project".into(),
            directory: dir,
            provider: ProviderChoice::GitHub,
            account: "owner".into(),
            custom_host: None,
            credential_ref: None,
            disk_layout: DiskLayout::Grouped,
            sources: vec![WorkspaceSource {
                handle: "owner".into(),
                display_name: None,
                avatar_url: None,
                bio: None,
                provider: "github".into(),
                repositories: repos,
            }],
            repository_list_mode: RepositoryListMode::Grouped,
        }
    }

    pub(super) fn make_repo(name: &str, url: String) -> SourceRepository {
        SourceRepository {
            name: name.into(),
            full_name: format!("owner/{name}"),
            description: None,
            stars: 0,
            forks: 0,
            primary_language: None,
            is_private: false,
            is_fork: false,
            clone_url: url,
            default_branch: "main".into(),
            selected: true,
        }
    }

    #[gpui_kit::test]
    async fn revision_bump_mid_clone_never_sticks_busy_and_allows_later_inspect_and_check(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("remote-one");
        init_remote(&remote);
        let project = test_project(
            temp.path(),
            vec![make_repo("one", remote.to_string_lossy().into_owned())],
        );

        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new(window, cx))
            })
            .unwrap()
        });
        let mut vcx = VisualTestContext::from_window(window, cx);

        let _ = vcx.update_window(window, |_, window, cx| {
            view.update(cx, |this, cx| {
                this.projects = vec![project.clone()];
                this.selected_project_id = Some(project.id);
                this.project_section = ProjectSection::Sources;
                this.execute_clone_plan(0, window, cx);
                assert!(
                    this.jobs
                        .project(project.id)
                        .is_some_and(|s| s.is_clone_busy())
                );
                // Mutate workspace_revision while clone is active.
                this.workspace_revision = this.workspace_revision.wrapping_add(1);
            });
        });

        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(25));
            vcx.background_executor
                .advance_clock(Duration::from_millis(60));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|s| s.is_any_busy())
            }) {
                break;
            }
        }

        view.read_with(cx, |this, _| {
            let state = this.jobs.project(project.id).unwrap();
            assert!(
                !state.is_clone_busy(),
                "clone job must clear even when workspace_revision changes mid-flight"
            );
            assert!(!state.is_inspect_or_check_busy());
            let key = RepoKey::new("github", "github.com", "owner", "one");
            assert!(
                state.repo(&key).is_some(),
                "post-clone incremental inspect must populate status even after revision bump"
            );
        });

        // Subsequent check_repository_updates must NOT be blocked!
        view.update(cx, |this, cx| {
            this.check_repository_updates(true, cx);
            let state = this.jobs.project(project.id).unwrap();
            assert!(
                state.is_inspect_or_check_busy(),
                "later inspect/check must start normally after revision-bumped clone"
            );
        });

        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(25));
            vcx.background_executor
                .advance_clock(Duration::from_millis(50));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|s| s.is_any_busy())
            }) {
                break;
            }
        }

        view.read_with(cx, |this, _| {
            assert!(!this.jobs.project(project.id).unwrap().is_any_busy());
        });
    }

    #[gpui_kit::test]
    async fn execute_clone_plan_surfaces_real_errors_leaves_no_empty_dirs_and_preserves_existing_repos(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let good_remote = temp.path().join("good-remote");
        init_remote(&good_remote);
        let second_remote = temp.path().join("second-remote");
        init_remote(&second_remote);
        let bad_remote = temp.path().join("missing-remote");
        let project = test_project(
            temp.path(),
            vec![
                make_repo("already", good_remote.to_string_lossy().into_owned()),
                make_repo("fresh", second_remote.to_string_lossy().into_owned()),
                make_repo("broken", bad_remote.to_string_lossy().into_owned()),
            ],
        );

        let already_path = project.directory.join("owner").join("already");
        std::fs::create_dir_all(already_path.parent().unwrap()).unwrap();
        git(
            temp.path(),
            &[
                "clone",
                good_remote.to_str().unwrap(),
                already_path.to_str().unwrap(),
            ],
        );

        let (window, view) = cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new(window, cx))
            })
            .unwrap()
        });
        let mut vcx = VisualTestContext::from_window(window, cx);

        let already_key = RepoKey::new("github", "github.com", "owner", "already");
        let fresh_key = RepoKey::new("github", "github.com", "owner", "fresh");
        let broken_key = RepoKey::new("github", "github.com", "owner", "broken");

        let _ = vcx.update_window(window, |_, window, cx| {
            view.update(cx, |this, cx| {
                this.projects = vec![project.clone()];
                this.selected_project_id = Some(project.id);
                // Seed status for `already` so we verify execute_clone_plan preserves existing entries!
                let inspected =
                    crate::repository_git::inspect(&crate::repository_git::RepositoryTarget {
                        key: already_key.clone(),
                        source_index: 0,
                        repo_index: 0,
                        name: "already".into(),
                        url: good_remote.to_string_lossy().into_owned(),
                        branch: "main".into(),
                        path: already_path.clone(),
                        project_root: project.directory.clone(),
                        credential_ref: None,
                        provider: "github".into(),
                        account: "owner".into(),
                        endpoint: None,
                    });
                let rec = crate::repository_git::record_from_status(
                    project.id,
                    &crate::repository_git::RepositoryTarget {
                        key: already_key.clone(),
                        source_index: 0,
                        repo_index: 0,
                        name: "already".into(),
                        url: good_remote.to_string_lossy().into_owned(),
                        branch: "main".into(),
                        path: already_path.clone(),
                        project_root: project.directory.clone(),
                        credential_ref: None,
                        provider: "github".into(),
                        account: "owner".into(),
                        endpoint: None,
                    },
                    &inspected,
                    None,
                    &std::collections::HashMap::new(),
                    false,
                );
                this.apply_repo_update(
                    project.id,
                    already_key.clone(),
                    RepoUpdate::replace(rec, RepoUpdateKind::Inspect),
                );
                this.project_section = ProjectSection::Sources;
                this.execute_clone_plan(0, window, cx);
                assert_eq!(this.project_section, ProjectSection::Repositories);
            });
        });

        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(25));
            vcx.background_executor
                .advance_clock(Duration::from_millis(60));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|s| s.is_any_busy())
            }) {
                break;
            }
        }

        view.read_with(cx, |this, _| {
            let state = this.jobs.project(project.id).unwrap();
            assert!(!state.is_any_busy());
            // Existing `already` status preserved!
            assert!(state.repo(&already_key).is_some());
            // Newly cloned `fresh` was incrementally inspected and populated!
            assert!(state.repo(&fresh_key).is_some());
            assert!(
                state
                    .repo(&fresh_key)
                    .is_some_and(|e| e.presentation().present)
            );
            // `broken` failed cleanly without leaving an empty folder!
            assert!(!project.directory.join("owner").join("broken").exists());
            assert!(matches!(
                state.clone_states.get(&broken_key),
                Some(RepoClonePhase::Failed(_))
            ));
            // Structured toast is Warning (1 succeeded, 1 failed), NOT Success/Info!
            let toast = this.last_toast.as_ref().expect("structured toast present");
            assert_eq!(toast.severity, ToastSeverity::Warning);
            assert!(toast.text.contains("failed"), "toast: {}", toast.text);
        });
    }

    #[gpui_kit::test]
    async fn stable_repo_keys_prevent_index_shift_corruption_and_preserve_unmodified_entries(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let remote_a = temp.path().join("remote-a");
        let remote_b = temp.path().join("remote-b");
        init_remote(&remote_a);
        init_remote(&remote_b);
        let mut project = test_project(
            temp.path(),
            vec![
                make_repo("alpha", remote_a.to_string_lossy().into_owned()),
                make_repo("beta", remote_b.to_string_lossy().into_owned()),
            ],
        );

        let alpha_key = RepoKey::new("github", "github.com", "owner", "alpha");
        let beta_key = RepoKey::new("github", "github.com", "owner", "beta");

        let window = cx.add_window(ReleaseDesk::new);
        let view = window.root(cx).unwrap();
        view.update(cx, |this, _| {
            this.projects = vec![project.clone()];
            this.selected_project_id = Some(project.id);
            let mut rec_a = crate::modules::repo_cache::CachedRepoRecord {
                project_id: project.id,
                key: alpha_key.clone(),
                destination: release_desk_git::DestinationKind::GitRepo,
                status: release_desk_git::CloneStatus::Current,
                latest_release_tag: Some("v1.0.0-alpha".into()),
                ..Default::default()
            };
            rec_a.head_sha = Some("aaa".into());
            let mut rec_b = crate::modules::repo_cache::CachedRepoRecord {
                project_id: project.id,
                key: beta_key.clone(),
                destination: release_desk_git::DestinationKind::GitRepo,
                status: release_desk_git::CloneStatus::Current,
                latest_release_tag: Some("v2.0.0-beta".into()),
                ..Default::default()
            };
            rec_b.head_sha = Some("bbb".into());
            this.apply_repo_updates_batch(
                project.id,
                vec![
                    (
                        alpha_key.clone(),
                        RepoUpdate::replace(rec_a, RepoUpdateKind::Inspect),
                    ),
                    (
                        beta_key.clone(),
                        RepoUpdate::replace(rec_b, RepoUpdateKind::Inspect),
                    ),
                ],
            );

            // Remove "alpha" at repo_index 0; "beta" shifts to index 0, yet its key is unchanged!
            project.sources[0].repositories.remove(0);
            let valid_keys = project.selected_repo_keys();
            this.projects[0] = project.clone();
            this.jobs
                .project_mut(project.id)
                .retain_valid_keys(&valid_keys);

            let runtime = this.jobs.project(project.id).unwrap();
            assert!(runtime.repo(&alpha_key).is_none());
            assert_eq!(
                runtime
                    .repo(&beta_key)
                    .and_then(|e| e.presentation().version_tag.as_deref()),
                Some("v2.0.0-beta"),
                "beta keeps its exact presentation after alpha removal"
            );
        });
    }

    #[test]
    fn worker_drop_guard_and_clone_batch_summary_behave_safely() {
        // Panic/unwind safety: WorkerDropGuard sends abort notification even on panic.
        let (tx, rx) = std::sync::mpsc::channel::<&'static str>();
        let panicked = std::panic::catch_unwind(|| {
            let guard = WorkerDropGuard::new(move || {
                let _ = tx.send("aborted");
            });
            let _keep = guard;
            panic!("simulated worker panic");
        });
        assert!(panicked.is_err());
        assert_eq!(rx.try_recv().ok(), Some("aborted"));

        // JobRegistry busy slot recovery after disconnect/abort.
        let mut registry = JobRegistry::default();
        let (job_id, _) = registry.begin_job(42, ProjectJobKind::Clone).unwrap();
        assert!(registry.begin_job(42, ProjectJobKind::Clone).is_none());
        registry.finish_job(42, job_id, ProjectJobKind::Clone);
        assert!(registry.begin_job(42, ProjectJobKind::Clone).is_some());

        // CloneBatchSummary severities.
        let k1 = RepoKey::new("github", "github.com", "o", "a");
        let k2 = RepoKey::new("github", "github.com", "o", "b");
        let all_fail = CloneBatchSummary {
            total: 1,
            failed: vec![(k1.clone(), "network error".into())],
            ..Default::default()
        };
        assert_eq!(all_fail.to_toast().severity, ToastSeverity::Error);

        let partial = CloneBatchSummary {
            total: 2,
            succeeded: vec![k1.clone()],
            failed: vec![(k2.clone(), "auth failed".into())],
            ..Default::default()
        };
        assert_eq!(partial.to_toast().severity, ToastSeverity::Warning);

        let all_ok = CloneBatchSummary {
            total: 2,
            succeeded: vec![k1, k2],
            ..Default::default()
        };
        assert_eq!(all_ok.to_toast().severity, ToastSeverity::Success);
    }

    #[gpui_kit::test]
    async fn ui_repro_clone_close_dialog_switch_tabs_toggle_repo_and_recheck(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let remote1 = temp.path().join("r1");
        let remote2 = temp.path().join("r2");
        let remote3 = temp.path().join("r3");
        init_remote(&remote1);
        init_remote(&remote2);
        init_remote(&remote3);
        let mut r3_unselected = make_repo("three", remote3.to_string_lossy().into_owned());
        r3_unselected.selected = false;

        let project = test_project(
            temp.path(),
            vec![
                make_repo("one", remote1.to_string_lossy().into_owned()),
                make_repo("two", remote2.to_string_lossy().into_owned()),
                r3_unselected,
            ],
        );

        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new(window, cx))
            })
            .unwrap()
        });
        let mut vcx = VisualTestContext::from_window(window, cx);

        // 1. Open SourceDialog::Review, trigger execute_clone_plan -> modal closes & switches to Repositories.
        let _ = vcx.update_window(window, |_, window, cx| {
            view.update(cx, |this, cx| {
                this.projects = vec![project.clone()];
                this.selected_project_id = Some(project.id);
                this.project_section = ProjectSection::Sources;
                this.source_dialog = crate::views::project_sources::SourceDialog::Review(0);
                this.execute_clone_plan(0, window, cx);
                assert_eq!(
                    this.source_dialog,
                    crate::views::project_sources::SourceDialog::Closed
                );
                assert_eq!(this.project_section, ProjectSection::Repositories);
                assert!(
                    this.jobs
                        .project(project.id)
                        .is_some_and(|s| s.is_clone_busy())
                );
            });
        });

        // 2. Switch tabs (Overview -> Repositories) and toggle another repo (bumps revision) mid-clone.
        let _ = vcx.update_window(window, |_, _, cx| {
            view.update(cx, |this, cx| {
                this.navigate_to(
                    crate::app::NavLocation::Project(project.id, ProjectSection::Overview),
                    cx,
                );
                // Toggle repo "three" in-memory (equivalent to user clicking checkbox without writing real settings).
                this.projects[0].sources[0].repositories[2].selected = true;
                this.workspace_revision = this.workspace_revision.wrapping_add(1);
                let valid = this.projects[0].selected_repo_keys();
                this.jobs.project_mut(project.id).retain_valid_keys(&valid);

                this.navigate_to(
                    crate::app::NavLocation::Project(project.id, ProjectSection::Repositories),
                    cx,
                );
            });
        });

        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(25));
            vcx.background_executor
                .advance_clock(Duration::from_millis(50));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|s| s.is_any_busy())
            }) {
                break;
            }
        }

        let k1 = RepoKey::new("github", "github.com", "owner", "one");
        let k2 = RepoKey::new("github", "github.com", "owner", "two");
        view.read_with(cx, |this, _| {
            let state = this.jobs.project(project.id).unwrap();
            assert!(!state.is_any_busy());
            for key in [&k1, &k2] {
                let entry = state.repo(key).expect("cloned repo inspected");
                let st = entry.status();
                assert_eq!(st.destination, release_desk_git::DestinationKind::GitRepo);
                assert_ne!(
                    st.state.as_ref().unwrap().status,
                    release_desk_git::CloneStatus::NotCloned,
                    "cloned repo must never show NotCloned"
                );
                assert!(entry.presentation().present);
            }
        });

        // 3. Re-run both local Inspect and remote Check and verify both still run cleanly.
        for local_only in [true, false] {
            view.update(cx, |this, cx| {
                this.check_repository_updates(local_only, cx);
                assert!(
                    this.jobs
                        .project(project.id)
                        .is_some_and(|s| s.is_inspect_or_check_busy())
                );
            });
            for _ in 0..80 {
                std::thread::sleep(Duration::from_millis(25));
                vcx.background_executor
                    .advance_clock(Duration::from_millis(50));
                vcx.run_until_parked();
                if view.read_with(cx, |this, _| {
                    !this
                        .jobs
                        .project(project.id)
                        .is_some_and(|s| s.is_any_busy())
                }) {
                    break;
                }
            }
            view.read_with(cx, |this, _| {
                let state = this.jobs.project(project.id).unwrap();
                assert!(!state.is_any_busy());
                if !local_only {
                    assert!(state.repo(&k1).is_some_and(|e| e.status().fetched));
                    assert!(state.repo(&k2).is_some_and(|e| e.status().fetched));
                }
            });
        }
    }

    #[gpui_kit::test]
    async fn duplicate_source_toggle_is_rejected_with_clear_message(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let mut project = test_project(
            temp.path(),
            vec![make_repo("dup", "https://github.com/Owner/Dup.git".into())],
        );
        let mut second_repo = make_repo("dup", "https://github.com/owner/dup.git".into());
        second_repo.selected = false;
        project.sources.push(WorkspaceSource {
            handle: "owner".into(),
            display_name: None,
            avatar_url: None,
            bio: None,
            provider: "github".into(),
            repositories: vec![second_repo],
        });

        let (_window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new(window, cx))
            })
            .unwrap()
        });
        view.update(cx, |this, cx| {
            this.projects = vec![project.clone()];
            this.selected_project_id = Some(project.id);
            this.toggle_source_repo(1, 0, cx);
            assert!(
                !this.projects[0].sources[1].repositories[0].selected,
                "duplicate selection across sources must be rejected"
            );
            let toast = this.last_toast.as_ref().expect("warning toast expected");
            assert_eq!(toast.severity, ToastSeverity::Warning);
            assert!(toast.text.contains("already selected"));
        });
    }
}

#[cfg(test)]
mod phase4_cache_and_scheduler_tests {
    use super::*;
    use crate::modules::remote_probe::{ProbeTarget, RemoteProbe as _, TieredRemoteProbe};
    use crate::modules::repo_cache::{
        CachedRepoRecord, RepoCache, StoredCommit, reset_trace_counters, thread_trace_snapshot,
    };
    use crate::modules::scheduler_service::{
        TriggerCoalescer, TriggerKind, VolumeConcurrencyLimiter,
    };
    use crate::repository_git::{RepositoryTarget, revalidate_or_inspect_target};
    use crate::workspace::{ProjectSection, ProviderChoice, WorkspaceProject};
    use crate::workspace_sources::{
        RepoKey, RepositoryListMode, SourceRepository, WorkspaceSource,
    };
    use gpui_kit::{AppContext as _, TestAppContext, VisualTestContext};
    use release_desk_core::project::DiskLayout;
    use release_desk_git::{CloneStatus, DestinationKind};
    use std::collections::HashMap;
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    fn git_run(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("git failed");
        assert!(
            out.status.success(),
            "git {:?} in {}: {}",
            args,
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn init_local_repo(dir: &Path, extra_init_args: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut init = vec!["init", "-b", "main"];
        init.extend_from_slice(extra_init_args);
        git_run(dir, &init);
        git_run(dir, &["config", "user.email", "test@example.com"]);
        git_run(dir, &["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "initial\n").unwrap();
        git_run(dir, &["add", "."]);
        git_run(dir, &["commit", "-m", "initial"]);
    }

    fn commit_file(dir: &Path, name: &str, contents: &str) {
        std::fs::write(dir.join(name), contents).unwrap();
        git_run(dir, &["add", "."]);
        git_run(dir, &["commit", "-m", "update"]);
    }

    pub(super) fn synthetic_project(nonexistent_dir: &Path, count: usize) -> WorkspaceProject {
        let repos: Vec<SourceRepository> = (0..count)
            .map(|i| SourceRepository {
                name: format!("repo-{i:02}"),
                full_name: format!("owner/repo-{i:02}"),
                description: Some(format!("Synthetic repository {i}")),
                stars: i as u32,
                forks: 0,
                primary_language: Some("Rust".into()),
                is_private: false,
                is_fork: false,
                clone_url: format!("https://github.com/owner/repo-{i:02}.git"),
                default_branch: "main".into(),
                selected: true,
            })
            .collect();
        WorkspaceProject {
            id: 909_001,
            name: "Synthetic-29".into(),
            directory: nonexistent_dir.to_path_buf(),
            provider: ProviderChoice::GitHub,
            account: "owner".into(),
            custom_host: None,
            credential_ref: None,
            repository_list_mode: RepositoryListMode::Grouped,
            disk_layout: DiskLayout::Grouped,
            sources: vec![WorkspaceSource {
                handle: "owner".into(),
                display_name: None,
                avatar_url: None,
                bio: None,
                provider: "github".into(),
                repositories: repos,
            }],
        }
    }

    #[gpui_kit::test]
    async fn t1_t2_cached_repositories_tab_paints_zero_subprocesses_zero_project_fs_and_survives_restart(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("cache.sqlite3");
        // Intentionally non-existent project directory on disk: any FS access is caught by trace counters
        // and proves cache paint never touches the project volume.
        let project_dir = temp.path().join("missing-volume-dir");
        let project = synthetic_project(&project_dir, 29);

        let now = crate::modules::repo_cache::now_unix_secs();
        let records: Vec<CachedRepoRecord> = project.sources[0]
            .repositories
            .iter()
            .enumerate()
            .map(|(idx, repo)| {
                let key = project.repo_key(&project.sources[0], repo);
                CachedRepoRecord {
                    project_id: project.id,
                    key,
                    destination: DestinationKind::GitRepo,
                    status: if idx % 7 == 0 {
                        CloneStatus::Behind
                    } else {
                        CloneStatus::Current
                    },
                    branch: Some("main".into()),
                    head_sha: Some(format!("{idx:040x}")),
                    remote_sha: Some(format!("{idx:040x}")),
                    probed_remote_sha: Some(if idx % 7 == 0 {
                        format!("{:040x}", idx + 1000)
                    } else {
                        format!("{idx:040x}")
                    }),
                    ahead_count: Some(0),
                    behind_count: Some(if idx % 7 == 0 { 2 } else { 0 }),
                    error_message: None,
                    status_slow_skipped: idx == 3,
                    probe_failed_reason: (idx == 5).then(|| "network".to_string()),
                    fetched: true,
                    dirty_tracked: Some(false),
                    dirty_tracked_at: Some(now),
                    has_untracked: Some(false),
                    has_untracked_at: Some(now),
                    recent_commits: vec![StoredCommit {
                        hash: "abcd1234abcd1234abcd1234abcd1234abcd1234".into(),
                        short_hash: "abcd123".into(),
                        author_name: "Dev".into(),
                        date: "2026-11-25".into(),
                        message: "cached commit".into(),
                    }],
                    icon_path: None,
                    latest_release_tag: Some("v1.2.0".into()),
                    latest_release_date: Some("2026-11-20".into()),
                    latest_release_assets: Vec::new(),
                    installed_version: (idx == 1).then(|| "v1.1.0".into()),
                    last_inspected_at: Some(now),
                    last_probed_at: Some(now),
                    last_fetched_at: Some(now),
                    fingerprint: None,
                    ..Default::default()
                }
            })
            .collect();

        {
            let cache = RepoCache::open_at(&db_path).unwrap();
            cache.upsert_repos_batch(&records).unwrap();
        }

        // Simulate app restart by opening a fresh RepoCache + fresh ReleaseDesk window.
        let reopened_cache = Arc::new(RepoCache::open_at(&db_path).unwrap());
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new(window, cx))
            })
            .unwrap()
        });
        let mut vcx = VisualTestContext::from_window(window, cx);

        view.update(cx, |this, cx| {
            this.projects = vec![project.clone()];
            this.selected_project_id = Some(project.id);
            this.project_section = ProjectSection::Overview;
            cx.notify();
        });
        vcx.run_until_parked();

        // Measure only cached hydration + opening Repositories tab + first frame render.
        reset_trace_counters();
        let t0 = Instant::now();
        let _ = vcx.update_window(window, |_, _, cx| {
            view.update(cx, |this, cx| {
                this.set_repo_cache(reopened_cache.clone());
                this.project_section = ProjectSection::Repositories;
                assert_eq!(
                    crate::app::project_load_action(
                        this.project_section,
                        false,
                        true,
                        false,
                        false
                    ),
                    crate::app::ProjectLoadAction::None,
                    "opening Repositories tab must not trigger inspect or fetch"
                );
                cx.notify();
            });
        });
        vcx.run_until_parked();
        let elapsed = t0.elapsed();
        let snap = thread_trace_snapshot();

        assert!(
            elapsed < Duration::from_millis(100),
            "cached hydration + paint took {elapsed:?}, expected <100ms"
        );
        assert_eq!(
            snap.git_subprocesses, 0,
            "cached Repositories paint must spawn 0 git subprocesses"
        );
        assert_eq!(
            snap.project_fs_accesses, 0,
            "cached Repositories paint must make 0 project FS accesses"
        );

        view.read_with(cx, |this, _| {
            let rt = this.jobs.project(project.id).expect("project runtime");
            assert!(!rt.is_any_busy());
            assert_eq!(rt.repos().len(), 29);
        });
        // D6 guard: setting a cached selected repository + re-painting must still run zero subprocesses/FS IO.
        view.update(cx, |this, cx| {
            let first_key = project.selected_repo_keys().into_iter().next().unwrap();
            this.jobs.project_mut(project.id).selected_repository = Some(first_key);
            cx.notify();
        });
        reset_trace_counters();
        vcx.run_until_parked();
        let snap2 = thread_trace_snapshot();
        assert_eq!(
            snap2.git_subprocesses, 0,
            "D6: paint after selecting cached repo must run 0 subprocesses"
        );
        assert_eq!(
            snap2.project_fs_accesses, 0,
            "D6: paint after selecting cached repo must run 0 project FS accesses"
        );
    }

    #[test]
    fn t3_fingerprint_hit_runs_zero_subprocesses_and_miss_re_inspects_across_loose_worktree_and_reftable()
     {
        let temp = tempfile::tempdir().unwrap();
        let loose_dir = temp.path().join("loose");
        init_local_repo(&loose_dir, &[]);

        let base_for_wt = temp.path().join("base-wt");
        init_local_repo(&base_for_wt, &[]);
        let worktree_dir = temp.path().join("worktree-checkout");
        git_run(
            &base_for_wt,
            &[
                "worktree",
                "add",
                "-b",
                "feature-main",
                worktree_dir.to_str().unwrap(),
            ],
        );
        assert!(
            worktree_dir.join(".git").is_file(),
            "worktree must have .git file"
        );

        let reftable_dir = temp.path().join("reftable");
        init_local_repo(&reftable_dir, &["--ref-format=reftable"]);

        let cases = [
            ("loose", loose_dir.clone(), "main"),
            ("worktree", worktree_dir.clone(), "feature-main"),
            ("reftable", reftable_dir.clone(), "main"),
        ];

        let icon_map = HashMap::new();
        for (name, dir, branch) in cases {
            let target = RepositoryTarget {
                project_root: temp.path().to_path_buf(),
                key: RepoKey::new("local", "local", "owner", name),
                source_index: 0,
                repo_index: 0,
                provider: "local".into(),
                account: "owner".into(),
                name: name.into(),
                url: dir.to_string_lossy().into_owned(),
                branch: branch.into(),
                credential_ref: None,
                endpoint: None,
                path: dir.clone(),
            };

            // Initial uncached inspect captures fingerprint.
            let (first_rec, timed_out) = revalidate_or_inspect_target(
                1,
                &target,
                None,
                false,
                &icon_map,
                Duration::from_secs(10),
            );
            assert!(!timed_out);
            assert!(
                first_rec.fingerprint.is_some(),
                "{name}: fingerprint missing"
            );
            let first_sha = first_rec.head_sha.clone().expect("head sha");

            // Unchanged repo with check_dirty_tracked = false -> 0 git subprocesses!
            reset_trace_counters();
            let (hit_rec, _) = revalidate_or_inspect_target(
                1,
                &target,
                Some(&first_rec),
                false,
                &icon_map,
                Duration::from_secs(10),
            );
            let hit_snap = thread_trace_snapshot();
            assert_eq!(
                hit_snap.git_subprocesses, 0,
                "{name}: unchanged repo revalidation must run 0 git subprocesses"
            );
            assert_eq!(hit_rec.head_sha.as_deref(), Some(first_sha.as_str()));

            // Advance branch HEAD -> fingerprint miss triggers ref re-inspection & new SHA.
            commit_file(&dir, "change.txt", "v2\n");
            reset_trace_counters();
            let (miss_rec, _) = revalidate_or_inspect_target(
                1,
                &target,
                Some(&hit_rec),
                false,
                &icon_map,
                Duration::from_secs(10),
            );
            let miss_snap = thread_trace_snapshot();
            assert!(
                miss_snap.git_subprocesses >= 1,
                "{name}: fingerprint miss must re-inspect refs"
            );
            assert_ne!(
                miss_rec.head_sha.as_deref(),
                Some(first_sha.as_str()),
                "{name}: head_sha must update after commit"
            );
        }
    }

    #[test]
    fn t6_slow_status_timeout_marks_status_slow_skipped_and_aimd_drops_concurrency() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("slow-repo");
        init_local_repo(&repo_dir, &[]);
        let target = RepositoryTarget {
            project_root: temp.path().to_path_buf(),
            key: RepoKey::new("local", "local", "owner", "slow-repo"),
            source_index: 0,
            repo_index: 0,
            provider: "local".into(),
            account: "owner".into(),
            name: "slow-repo".into(),
            url: repo_dir.to_string_lossy().into_owned(),
            branch: "main".into(),
            credential_ref: None,
            endpoint: None,
            path: repo_dir.clone(),
        };
        let icon_map = HashMap::new();
        let (baseline, _) = revalidate_or_inspect_target(
            42,
            &target,
            None,
            false,
            &icon_map,
            Duration::from_secs(10),
        );

        // Force dirty_tracked timeout on a fingerprint-hit repo -> keeps valid state + flags status_slow_skipped.
        let (slow_rec, timed_out) = revalidate_or_inspect_target(
            42,
            &target,
            Some(&baseline),
            true,
            &icon_map,
            Duration::from_nanos(1),
        );
        assert!(timed_out);
        assert!(slow_rec.status_slow_skipped);
        assert_eq!(slow_rec.head_sha, baseline.head_sha);

        // Verify AIMD limiter drops from 2 -> 1 and persists via RepoCache.
        let cache = RepoCache::open_at(&temp.path().join("cache.sqlite3")).unwrap();
        let limiter = VolumeConcurrencyLimiter::new();
        let dev = crate::modules::scheduler_service::volume_dev_for_path(&repo_dir);
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), 2);
        limiter.record_sample(dev, false, 12_000.0, true, Some(&cache));
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), 1);

        // New limiter instance reads persisted limit = 1 from SQLite cache.
        let limiter2 = VolumeConcurrencyLimiter::new();
        assert_eq!(limiter2.current_limit(dev, false, Some(&cache)), 1);
    }

    #[test]
    fn coalescer_dedupes_focus_events_and_probe_detects_remote_sha_without_fetching_objects() {
        let mut coalescer = TriggerCoalescer::default();
        assert!(coalescer.request(7, TriggerKind::Focus, Duration::from_secs(30)));
        // Duplicate while active or inside cooldown is skipped.
        assert!(!coalescer.request(7, TriggerKind::Focus, Duration::from_secs(30)));
        coalescer.complete(7, TriggerKind::Focus);
        assert!(!coalescer.request(7, TriggerKind::Focus, Duration::from_secs(30)));
        // Manual check is distinct.
        assert!(coalescer.request(7, TriggerKind::ManualCheck, Duration::from_secs(30)));

        let temp = tempfile::tempdir().unwrap();
        let bare_remote = temp.path().join("remote.git");
        std::fs::create_dir_all(&bare_remote).unwrap();
        git_run(&bare_remote, &["init", "--bare", "-b", "main"]);

        let seed = temp.path().join("seed");
        init_local_repo(&seed, &[]);
        git_run(
            &seed,
            &["remote", "add", "origin", bare_remote.to_str().unwrap()],
        );
        git_run(&seed, &["push", "-u", "origin", "main"]);

        let local_clone = temp.path().join("local-clone");
        git_run(
            temp.path(),
            &[
                "clone",
                bare_remote.to_str().unwrap(),
                local_clone.to_str().unwrap(),
            ],
        );

        let target = RepositoryTarget {
            project_root: temp.path().to_path_buf(),
            key: RepoKey::new("local", "local", "owner", "local-clone"),
            source_index: 0,
            repo_index: 0,
            provider: "local".into(),
            account: "owner".into(),
            name: "local-clone".into(),
            url: bare_remote.to_string_lossy().into_owned(),
            branch: "main".into(),
            credential_ref: None,
            endpoint: None,
            path: local_clone.clone(),
        };
        let icon_map = HashMap::new();
        let (mut rec, _) = revalidate_or_inspect_target(
            9,
            &target,
            None,
            false,
            &icon_map,
            Duration::from_secs(10),
        );

        // Push a second commit to bare_remote without fetching in local_clone.
        commit_file(&seed, "upstream.txt", "new upstream commit\n");
        git_run(&seed, &["push", "origin", "main"]);

        let probe = TieredRemoteProbe::new();
        let probed = probe.probe_batch(
            &[ProbeTarget {
                key: target.key.clone(),
                clone_url: target.url.clone(),
                default_branch: target.branch.clone(),
                credential_ref: None,
                endpoint: None,
            }],
            None,
            &AtomicBool::new(false),
        );
        assert_eq!(probed.len(), 1);
        rec.probed_remote_sha = probed[0].remote_sha.clone();
        assert!(rec.has_probed_update());
        assert_eq!(rec.effective_clone_status(), CloneStatus::Behind);
        assert_eq!(
            git_run(&local_clone, &["rev-list", "--count", "HEAD..origin/main"]),
            "0",
            "probe must not fetch upstream objects into local origin/main"
        );
    }

    #[gpui_kit::test]
    async fn c1_c7_ten_focus_events_in_5s_launch_at_most_one_visible_batch_and_interval_loop_runs_under_fake_clock(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let project = synthetic_project(temp.path(), 20);
        let fake_clock = Arc::new(crate::modules::scheduler_service::FakeClock::new(1_000_000));
        let config = crate::app::SchedulerConfig {
            clock: fake_clock.clone(),
            focus_cooldown: Duration::from_secs(60),
            check_interval: Duration::from_secs(15 * 60),
            startup_delay: Duration::from_millis(200),
            auto_start: true,
        };
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new_with_config(config.clone(), window, cx))
            })
            .unwrap()
        });
        let vcx = VisualTestContext::from_window(window, cx);

        view.update(cx, |this, cx| {
            this.projects = vec![project.clone()];
            this.selected_project_id = Some(project.id);
            this.tab = crate::state::WorkspaceTab::ProjectHub;
            this.project_section = ProjectSection::Repositories;
            // Also select the last repository (index 19, off-screen in the virtual list)
            // so the test proves Focus scopes to `selected ∪ visible`, not all 20.
            let last_key =
                project.repo_key(&project.sources[0], &project.sources[0].repositories[19]);
            this.select_repository(last_key, cx);
            this.scheduler_batches_started = 0;
            this.trigger_coalescer =
                crate::modules::scheduler_service::TriggerCoalescer::new(fake_clock.clone());
            cx.notify();
        });
        vcx.run_until_parked();

        // Advance GPUI fake background clock past startup_delay -> Startup scheduler fires once.
        vcx.background_executor
            .advance_clock(Duration::from_millis(250));
        vcx.run_until_parked();
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(20));
            vcx.background_executor
                .advance_clock(Duration::from_millis(110));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|r| r.is_any_busy())
            }) {
                break;
            }
        }
        view.update(cx, |this, _| {
            assert_eq!(
                this.scheduler_batches_started, 1,
                "start_scheduler_loop startup trigger must run in tests too"
            );
            this.scheduler_batches_started = 0;
        });

        // Fire 10 focus events within 5 seconds on fake clock.
        view.update(cx, |this, cx| {
            let mut expected_scoped = this.visible_repo_keys.lock().unwrap().clone();
            if expected_scoped.is_empty() {
                for repo in project.sources[0].repositories.iter().take(6) {
                    expected_scoped.insert(project.repo_key(&project.sources[0], repo));
                }
            }
            if let Some(sel) = this
                .jobs
                .project(project.id)
                .and_then(|r| r.selected_repository.clone())
            {
                expected_scoped.insert(sel);
            }
            for step in 0..10 {
                if step > 0 {
                    fake_clock.advance(Duration::from_millis(500));
                }
                this.on_window_focus(cx);
            }
            assert_eq!(
                this.scheduler_batches_started, 1,
                "10 focus events in 5s must launch at most 1 batch"
            );
            let rt = this.jobs.project(project.id).unwrap();
            assert!(
                rt.check_total > 0 && rt.check_total < 20,
                "focus batch must NOT scan all 20 repos, got {}",
                rt.check_total
            );
            assert_eq!(
                rt.check_total,
                expected_scoped.len(),
                "focus batch must cover selected ∪ currently visible rows"
            );
        });

        // Wait for focus batch to settle.
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(20));
            vcx.background_executor
                .advance_clock(Duration::from_millis(110));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|r| r.is_any_busy())
            }) {
                break;
            }
        }

        // Advance FakeClock past 60s -> next Focus can trigger cleanly.
        fake_clock.advance(Duration::from_secs(61));
        view.update(cx, |this, cx| {
            this.on_window_focus(cx);
            assert_eq!(this.scheduler_batches_started, 2);
        });
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(20));
            vcx.background_executor
                .advance_clock(Duration::from_millis(110));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|r| r.is_any_busy())
            }) {
                break;
            }
        }

        // Advance both FakeClock & GPUI executor past 15m interval -> Interval trigger fires cleanly.
        fake_clock.advance(Duration::from_secs(15 * 60 + 5));
        vcx.background_executor
            .advance_clock(Duration::from_secs(15 * 60 + 5));
        vcx.run_until_parked();
        view.update(cx, |this, _| {
            assert_eq!(
                this.scheduler_batches_started, 3,
                "start_scheduler_loop interval trigger must fire after check_interval"
            );
        });
    }

    #[gpui_kit::test]
    async fn c7_desktop_ui_menus_navigation_dismiss_and_always_visible_scrollbars(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::ActiveTheme as _;

        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::menu::install(cx);
        });

        let menu_count = cx.update(|cx| gpui_kit::base::GlobalState::global(cx).app_menus().len());
        assert_eq!(
            menu_count, 5,
            "desktop menu bar must install File/Release Desk, Edit, View, Tools, Window menus"
        );

        let temp = tempfile::tempdir().unwrap();
        let project = synthetic_project(temp.path(), 3);
        let config = crate::app::SchedulerConfig {
            auto_start: false,
            ..Default::default()
        };
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new_with_config(config, window, cx))
            })
            .unwrap()
        });
        let mut vcx = VisualTestContext::from_window(window, cx);
        vcx.run_until_parked();

        // ScrollbarMode::Always is enforced by apply_appearance so scrollbars remain visible.
        view.read_with(cx, |_, cx| {
            assert_eq!(
                cx.theme().scrollbar_mode,
                gpui_kit::component::scroll::ScrollbarMode::Always
            );
        });

        // Menu actions navigate and GoBack restores prior view.
        view.update(cx, |this, cx| {
            this.projects = vec![project.clone()];
            this.selected_project_id = Some(project.id);
            this.source_dialog = crate::views::project_sources::SourceDialog::Select(0);
            this.dismiss_active_dialog(cx);
            assert_eq!(
                this.source_dialog,
                crate::views::project_sources::SourceDialog::Closed,
                "DismissDialog / Esc must close active source dialog"
            );
            this.selected_store_image = Some(1);
            this.dismiss_active_dialog(cx);
            assert_eq!(this.selected_store_image, None);
        });

        vcx.dispatch_action(crate::menu::NavAppStore);
        vcx.run_until_parked();
        view.read_with(cx, |this, _| {
            assert_eq!(this.tab, crate::state::WorkspaceTab::AppStore);
        });

        vcx.dispatch_action(crate::menu::NavSettings);
        vcx.run_until_parked();
        view.read_with(cx, |this, _| {
            assert_eq!(this.tab, crate::state::WorkspaceTab::Settings);
        });

        vcx.dispatch_action(crate::menu::GoBack);
        vcx.run_until_parked();
        view.read_with(cx, |this, _| {
            assert_eq!(this.tab, crate::state::WorkspaceTab::AppStore);
        });
    }

    #[gpui_kit::test]
    async fn bug4_fetch_behind_then_pull_syncs_status_cache_presentation_and_commits(
        cx: &mut TestAppContext,
    ) {
        use super::phase3_clone_and_state_tests::{git, init_remote, make_repo, test_project};
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let seed = temp.path().join("seed");
        init_remote(&seed);
        let bare = temp.path().join("remote.git");
        git(
            temp.path(),
            &[
                "clone",
                "--bare",
                seed.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let project = test_project(
            temp.path(),
            vec![make_repo("app", bare.to_string_lossy().into_owned())],
        );
        let local = project.directory.join("owner").join("app");
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        git(
            temp.path(),
            &[
                "clone",
                "-b",
                "main",
                bare.to_str().unwrap(),
                local.to_str().unwrap(),
            ],
        );

        // Push a second commit from seed into bare so local is 1 commit behind.
        std::fs::write(seed.join("CHANGELOG.md"), "v2\n").unwrap();
        git(&seed, &["add", "CHANGELOG.md"]);
        git(&seed, &["commit", "-m", "second remote commit"]);
        git(&seed, &["push", bare.to_str().unwrap(), "main"]);

        let cache = std::sync::Arc::new(
            crate::modules::repo_cache::RepoCache::open_at(&temp.path().join("repo-cache.sqlite"))
                .unwrap(),
        );
        let config = crate::app::SchedulerConfig {
            auto_start: false,
            ..Default::default()
        };
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new_with_config(config, window, cx))
            })
            .unwrap()
        });
        let mut vcx = VisualTestContext::from_window(window, cx);
        let key = RepoKey::new("github", "github.com", "owner", "app");

        view.update(cx, |this, cx| {
            this.repo_cache = Some(cache.clone());
            this.projects = vec![project.clone()];
            this.selected_project_id = Some(project.id);
            this.select_repository(key.clone(), cx);
        });

        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(20));
            vcx.background_executor
                .advance_clock(Duration::from_millis(60));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                this.jobs
                    .project(project.id)
                    .is_some_and(|s| s.repo(&key).is_some_and(|e| e.is_cloned()))
            }) {
                break;
            }
        }
        view.read_with(cx, |this, _| {
            let state = this.jobs.project(project.id).unwrap();
            let entry = state.repo(&key).expect("repo entry present");
            assert!(entry.is_cloned());
            assert!(
                entry.presentation().present && !entry.presentation().recent_commits.is_empty()
            );
            crate::controllers::job_registry::assert_repo_state_invariants(
                &project,
                state,
                Some(&cache),
            );
        });

        // Fetch via check_one_repository -> status & cache must reflect Behind with 1 behind.
        let _ = vcx.update_window(window, |_, _, cx| {
            view.update(cx, |this, cx| {
                this.check_one_repository(key.clone(), cx);
            });
        });
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(20));
            vcx.background_executor
                .advance_clock(Duration::from_millis(100));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|s| s.is_any_busy())
            }) {
                break;
            }
        }
        view.read_with(cx, |this, _| {
            let state = this.jobs.project(project.id).unwrap();
            let entry = state.repo(&key).unwrap();
            let st = entry.status();
            assert_eq!(st.state.as_ref().unwrap().status, CloneStatus::Behind);
            assert!(st.fetched);
            let rec = entry.record();
            assert_eq!(rec.effective_clone_status(), CloneStatus::Behind);
            assert_eq!(rec.behind_count, Some(1));
            assert!(
                entry.presentation().present && !entry.presentation().recent_commits.is_empty(),
                "check_one_repository must preserve presentation and commits"
            );
            crate::controllers::job_registry::assert_repo_state_invariants(
                &project,
                state,
                Some(&cache),
            );
        });

        // Pull via enqueue_repository_pull -> unified entry & SQLite update to Current!
        let _ = vcx.update_window(window, |_, window, cx| {
            view.update(cx, |this, cx| {
                this.enqueue_repository_pull(key.clone(), window, cx);
            });
        });
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(20));
            vcx.background_executor
                .advance_clock(Duration::from_millis(60));
            vcx.run_until_parked();
            if view.read_with(cx, |this, _| {
                !this
                    .jobs
                    .project(project.id)
                    .is_some_and(|s| s.active_pull_queue || s.is_any_busy())
            }) {
                break;
            }
        }
        view.read_with(cx, |this, _| {
            let state = this.jobs.project(project.id).unwrap();
            let entry = state.repo(&key).expect("entry present");
            let st = entry.status();
            assert!(st.destination.is_git_repo());
            assert_eq!(st.state.as_ref().unwrap().status, CloneStatus::Current);
            assert!(st.fetched);
            let rec = entry.record();
            assert_eq!(rec.effective_clone_status(), CloneStatus::Current);
            assert!(!rec.has_probed_update());
            assert_eq!(rec.behind_count, Some(0));
            assert_eq!(rec.ahead_count, Some(0));
            assert!(rec.destination.is_git_repo());
            let pres = entry.presentation();
            assert!(pres.present);
            assert!(
                pres.recent_commits
                    .iter()
                    .any(|c| c.message.contains("second remote commit")),
                "pulled commit should appear in detail commits: {:?}",
                pres.recent_commits
            );
            crate::controllers::job_registry::assert_repo_state_invariants(
                &project,
                state,
                Some(&cache),
            );
        });
        let persisted = cache.load_project_repos(project.id);
        let stored = persisted.get(&key).expect("SQLite record persisted");
        assert_eq!(stored.effective_clone_status(), CloneStatus::Current);
        assert!(
            stored
                .recent_commits
                .iter()
                .any(|c| c.message.contains("second remote commit"))
        );
    }

    #[gpui_kit::test]
    async fn d4_model_based_sequence_and_fixed_scenarios(cx: &mut TestAppContext) {
        use super::phase3_clone_and_state_tests::{git, init_remote, make_repo, test_project};
        use crate::controllers::job_registry::assert_repo_state_invariants;
        use crate::modules::scheduler_service::{FakeClock, TriggerKind};

        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("d4-cache.sqlite");
        let cache =
            std::sync::Arc::new(crate::modules::repo_cache::RepoCache::open_at(&db_path).unwrap());

        let seed_a = temp.path().join("seed-a");
        let seed_b = temp.path().join("seed-b");
        let seed_c = temp.path().join("seed-c");
        init_remote(&seed_a);
        init_remote(&seed_b);
        init_remote(&seed_c);

        let bare_a = temp.path().join("bare-a.git");
        let bare_b = temp.path().join("bare-b.git");
        let bare_c = temp.path().join("bare-c.git");
        for (seed, bare) in [(&seed_a, &bare_a), (&seed_b, &bare_b), (&seed_c, &bare_c)] {
            git(
                temp.path(),
                &[
                    "clone",
                    "--bare",
                    seed.to_str().unwrap(),
                    bare.to_str().unwrap(),
                ],
            );
        }

        let mut proj1 = test_project(
            &temp.path().join("p1"),
            vec![
                make_repo("alpha", bare_a.to_string_lossy().into_owned()),
                make_repo("beta", bare_b.to_string_lossy().into_owned()),
            ],
        );
        proj1.id = 440_001;
        let mut proj2 = test_project(
            &temp.path().join("p2"),
            vec![make_repo("gamma", bare_c.to_string_lossy().into_owned())],
        );
        proj2.id = 440_002;

        let alpha_path = proj1.directory.join("owner").join("alpha");
        let beta_path = proj1.directory.join("owner").join("beta");
        let gamma_path = proj2.directory.join("owner").join("gamma");
        for (bare, local) in [
            (&bare_a, &alpha_path),
            (&bare_b, &beta_path),
            (&bare_c, &gamma_path),
        ] {
            std::fs::create_dir_all(local.parent().unwrap()).unwrap();
            git(
                temp.path(),
                &[
                    "clone",
                    "-b",
                    "main",
                    bare.to_str().unwrap(),
                    local.to_str().unwrap(),
                ],
            );
        }

        let fake_clock = std::sync::Arc::new(FakeClock::new(10_000));
        let make_config = || crate::app::SchedulerConfig {
            clock: fake_clock.clone(),
            focus_cooldown: Duration::from_secs(10),
            check_interval: Duration::from_secs(600),
            startup_delay: Duration::from_secs(600),
            auto_start: false,
        };
        let open_window_view = |cx: &mut TestAppContext| {
            let config = make_config();
            let (window, view) = cx.update(|cx| {
                gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                    cx.new(|cx| ReleaseDesk::new_with_config(config, window, cx))
                })
                .unwrap()
            });
            let vcx = VisualTestContext::from_window(window, cx);
            (window, view, vcx)
        };
        let pump_until_idle = |vcx: &mut VisualTestContext,
                               cx: &mut TestAppContext,
                               view: &gpui_kit::Entity<ReleaseDesk>,
                               pid: u128| {
            for _ in 0..120 {
                std::thread::sleep(Duration::from_millis(15));
                vcx.background_executor
                    .advance_clock(Duration::from_millis(80));
                vcx.run_until_parked();
                if view.read_with(cx, |this, _| {
                    !this.jobs.project(pid).is_some_and(|s| s.is_any_busy())
                }) {
                    break;
                }
            }
        };
        let verify_ground_truth = |view: &gpui_kit::Entity<ReleaseDesk>,
                                   cx: &mut TestAppContext,
                                   proj: &WorkspaceProject| {
            view.read_with(cx, |this, _| {
                let rt = this.jobs.project(proj.id).expect("project runtime");
                assert_repo_state_invariants(proj, rt, Some(&cache));
                for src in &proj.sources {
                    for repo in &src.repositories {
                        if !repo.selected {
                            continue;
                        }
                        let key = proj.repo_key(src, repo);
                        let path = crate::repository_git::resolve_target_path(
                            &proj.directory,
                            proj.disk_layout,
                            crate::repository_git::repository_owner(src, repo),
                            &repo.name,
                        );
                        let actual_kind =
                            release_desk_git::classify_destination(&proj.directory, &path);
                        if let Some(entry) = rt.repo(&key) {
                            if actual_kind == release_desk_git::DestinationKind::GitRepo {
                                assert!(entry.is_cloned(), "{} must stay cloned", key.name);
                                let head = git(&path, &["rev-parse", "--verify", "HEAD"]);
                                if let Some(ref rec_head) = entry.record().head_sha {
                                    assert_eq!(rec_head, &head, "HEAD mismatch for {}", key.name);
                                }
                                let dirty_now =
                                    !git(&path, &["status", "--porcelain=v1", "-uno"]).is_empty();
                                if dirty_now && entry.record().dirty_tracked == Some(true) {
                                    assert_eq!(
                                        entry.record().effective_clone_status(),
                                        CloneStatus::Dirty
                                    );
                                    assert!(!entry.can_pull());
                                }
                            } else if entry.record().destination
                                == release_desk_git::DestinationKind::EmptyDir
                            {
                                assert!(entry.is_incomplete());
                            }
                        }
                    }
                }
            });
        };

        let key_a = proj1.repo_key(&proj1.sources[0], &proj1.sources[0].repositories[0]);
        let key_b = proj1.repo_key(&proj1.sources[0], &proj1.sources[0].repositories[1]);

        let (mut window, mut view, mut vcx) = open_window_view(cx);
        view.update(cx, |this, cx| {
            this.repo_cache = Some(cache.clone());
            this.projects = vec![proj1.clone(), proj2.clone()];
            this.selected_project_id = Some(proj1.id);
            this.check_repository_updates(true, cx);
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        view.update(cx, |this, cx| {
            this.select_repository(key_a.clone(), cx);
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        vcx.run_until_parked();
        verify_ground_truth(&view, cx, &proj1);

        // Fixed Scenario 1: Behind -> Pull via update queue -> close dialog -> select repo:
        // commits present, "Up to date", counts decremented; repeat after restart.
        let mut commit_seq = 1usize;
        let mut push_commit = |seed: &Path, msg: &str| {
            commit_seq += 1;
            std::fs::write(seed.join(format!("f{commit_seq}.txt")), format!("{msg}\n")).unwrap();
            git(seed, &["add", "."]);
            git(seed, &["commit", "-m", msg]);
            let bare = match seed {
                p if p == seed_a => &bare_a,
                p if p == seed_b => &bare_b,
                _ => &bare_c,
            };
            git(seed, &["push", bare.to_str().unwrap(), "main"]);
        };

        push_commit(&seed_a, "d4 fixed queue pull commit");
        let _ = vcx.update_window(window, |_, _, cx| {
            view.update(cx, |this, cx| {
                this.check_one_repository(key_a.clone(), cx);
            });
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        verify_ground_truth(&view, cx, &proj1);

        let _ = vcx.update_window(window, |_, window, cx| {
            view.update(cx, |this, cx| {
                this.enqueue_available_repository_pulls(window, cx);
                this.git_update_queue_open = true;
            });
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        view.update(cx, |this, cx| {
            this.git_update_queue_open = false;
            this.select_repository(key_a.clone(), cx);
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        view.read_with(cx, |this, _| {
            let rt = this.jobs.project(proj1.id).unwrap();
            let entry = rt.repo(&key_a).unwrap();
            assert_eq!(
                entry.record().effective_clone_status(),
                CloneStatus::Current
            );
            assert!(entry.status().fetched);
            assert!(
                entry
                    .presentation()
                    .recent_commits
                    .iter()
                    .any(|c| c.message.contains("d4 fixed queue pull commit"))
            );
            let counts = crate::controllers::job_registry::ProjectSummaryCounts::from_repos(
                proj1.selected_repo_keys().iter(),
                rt.repos(),
            );
            assert_eq!(counts.updates_count, 0);
            assert_eq!(counts.pull_count, 0);
        });
        verify_ground_truth(&view, cx, &proj1);

        // Simulated restart: drop old window/view state completely and hydrate a new ReleaseDesk from SQLite.
        let (window_r, view_r, mut vcx_r) = open_window_view(cx);
        view_r.update(cx, |this, cx| {
            this.repo_cache = Some(cache.clone());
            this.projects = vec![proj1.clone(), proj2.clone()];
            this.selected_project_id = Some(proj1.id);
            this.hydrate_project_from_cache(proj1.id);
            this.select_repository(key_a.clone(), cx);
        });
        pump_until_idle(&mut vcx_r, cx, &view_r, proj1.id);
        view_r.read_with(cx, |this, _| {
            let rt = this.jobs.project(proj1.id).unwrap();
            let entry = rt.repo(&key_a).unwrap();
            assert_eq!(
                entry.record().effective_clone_status(),
                CloneStatus::Current
            );
            assert!(
                entry
                    .presentation()
                    .recent_commits
                    .iter()
                    .any(|c| c.message.contains("d4 fixed queue pull commit"))
            );
        });
        verify_ground_truth(&view_r, cx, &proj1);
        window = window_r;
        view = view_r;
        vcx = vcx_r;

        // Fixed Scenario 2: Behind -> check-only / auto-fetch / probe paths keep detail & commits intact.
        push_commit(&seed_a, "d4 fixed check commit");
        let _ = vcx.update_window(window, |_, _, cx| {
            view.update(cx, |this, cx| {
                this.check_one_repository(key_a.clone(), cx);
                this.select_repository(key_a.clone(), cx);
            });
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        view.read_with(cx, |this, _| {
            let entry = this.jobs.project(proj1.id).unwrap().repo(&key_a).unwrap();
            assert_eq!(entry.record().effective_clone_status(), CloneStatus::Behind);
            assert!(!entry.presentation().recent_commits.is_empty());
        });

        push_commit(&seed_b, "d4 fixed probe and autofetch commit");
        fake_clock.advance(Duration::from_secs(30));
        view.update(cx, |this, cx| {
            this.settings.auto_fetch_projects.remove(&proj1.id);
            this.schedule_revalidation(proj1.id, TriggerKind::Focus, cx);
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        view.update(cx, |this, cx| {
            this.select_repository(key_b.clone(), cx);
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        verify_ground_truth(&view, cx, &proj1);

        fake_clock.advance(Duration::from_secs(35));
        view.update(cx, |this, cx| {
            this.settings.auto_fetch_projects.insert(proj1.id);
            this.schedule_revalidation(proj1.id, TriggerKind::Interval, cx);
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        view.update(cx, |this, cx| {
            this.select_repository(key_b.clone(), cx);
        });
        pump_until_idle(&mut vcx, cx, &view, proj1.id);
        view.read_with(cx, |this, _| {
            let entry = this.jobs.project(proj1.id).unwrap().repo(&key_b).unwrap();
            assert!(entry.is_cloned());
            assert!(!entry.presentation().recent_commits.is_empty());
        });
        verify_ground_truth(&view, cx, &proj1);

        // Seeded randomized sequence over remaining operations (push, dirty/clean edit, fetch, focus,
        // single pull, queue pull, remove/retry incomplete dir, toggle repo, switch project, restart).
        let mut rng: u64 = 0x00D4_5EED_2026_1126_u64;
        let mut next_u64 = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };

        let incomplete_path = proj1.directory.join("owner").join("delta");
        for step in 0..18 {
            match next_u64() % 11 {
                0 => {
                    push_commit(&seed_a, &format!("seq commit alpha {step}"));
                }
                1 => {
                    push_commit(&seed_b, &format!("seq commit beta {step}"));
                }
                2 => {
                    std::fs::write(beta_path.join("README.md"), format!("dirty {step}\n")).unwrap();
                }
                3 => {
                    git(&beta_path, &["checkout", "--", "README.md"]);
                }
                4 => {
                    view.update(cx, |this, cx| {
                        this.check_repository_updates(false, cx);
                    });
                    pump_until_idle(&mut vcx, cx, &view, proj1.id);
                }
                5 => {
                    fake_clock.advance(Duration::from_secs(35));
                    let enable_af = next_u64().is_multiple_of(2);
                    view.update(cx, |this, cx| {
                        if enable_af {
                            this.settings.auto_fetch_projects.insert(proj1.id);
                        } else {
                            this.settings.auto_fetch_projects.remove(&proj1.id);
                        }
                        this.schedule_revalidation(proj1.id, TriggerKind::Focus, cx);
                    });
                    pump_until_idle(&mut vcx, cx, &view, proj1.id);
                }
                6 => {
                    let _ = vcx.update_window(window, |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.enqueue_repository_pull(key_a.clone(), window, cx);
                        });
                    });
                    pump_until_idle(&mut vcx, cx, &view, proj1.id);
                }
                7 => {
                    let _ = vcx.update_window(window, |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.enqueue_available_repository_pulls(window, cx);
                        });
                    });
                    pump_until_idle(&mut vcx, cx, &view, proj1.id);
                }
                8 => {
                    // Toggle incomplete-clone folder (EmptyDir), inspect, then remove & re-toggle off.
                    let _ = std::fs::create_dir_all(&incomplete_path);
                    let mut extra_repo = make_repo("delta", bare_a.to_string_lossy().into_owned());
                    extra_repo.selected = true;
                    proj1.sources[0].repositories.push(extra_repo);
                    let valid = proj1.selected_repo_keys();
                    view.update(cx, |this, cx| {
                        this.projects[0] = proj1.clone();
                        this.workspace_revision = this.workspace_revision.wrapping_add(1);
                        this.jobs.project_mut(proj1.id).retain_valid_keys(&valid);
                        let _ = cache.retain_project_keys(proj1.id, &valid);
                        this.check_repository_updates(true, cx);
                    });
                    pump_until_idle(&mut vcx, cx, &view, proj1.id);
                    proj1.sources[0].repositories.retain(|r| r.name != "delta");
                    let _ = std::fs::remove_dir_all(&incomplete_path);
                    let valid = proj1.selected_repo_keys();
                    view.update(cx, |this, _| {
                        this.projects[0] = proj1.clone();
                        this.workspace_revision = this.workspace_revision.wrapping_add(1);
                        this.jobs.project_mut(proj1.id).retain_valid_keys(&valid);
                        let _ = cache.retain_project_keys(proj1.id, &valid);
                    });
                }
                9 => {
                    // Switch project & back cleanly.
                    view.update(cx, |this, cx| {
                        this.selected_project_id = Some(proj2.id);
                        this.hydrate_project_from_cache(proj2.id);
                        this.check_repository_updates(true, cx);
                    });
                    pump_until_idle(&mut vcx, cx, &view, proj2.id);
                    verify_ground_truth(&view, cx, &proj2);
                    view.update(cx, |this, cx| {
                        this.selected_project_id = Some(proj1.id);
                        this.hydrate_project_from_cache(proj1.id);
                        this.select_repository(key_a.clone(), cx);
                    });
                    pump_until_idle(&mut vcx, cx, &view, proj1.id);
                }
                _ => {
                    // Full memory wipe + SQLite restart.
                    let (w2, v2, mut vcx2) = open_window_view(cx);
                    v2.update(cx, |this, cx| {
                        this.repo_cache = Some(cache.clone());
                        this.projects = vec![proj1.clone(), proj2.clone()];
                        this.selected_project_id = Some(proj1.id);
                        this.hydrate_project_from_cache(proj1.id);
                        this.select_repository(key_a.clone(), cx);
                    });
                    pump_until_idle(&mut vcx2, cx, &v2, proj1.id);
                    window = w2;
                    view = v2;
                    vcx = vcx2;
                }
            }
            verify_ground_truth(&view, cx, &proj1);
        }
    }
}

#[cfg(test)]
mod step_f_layout_and_scroll_tests {
    use super::*;
    use crate::controllers::git_controller::phase4_cache_and_scheduler_tests::synthetic_project;
    use crate::git_update_queue::{GitUpdateJob, GitUpdateStatus};
    use crate::modules::repo_cache::{CachedRepoRecord, StoredCommit};
    use crate::release_queue::{DownloadJob, DownloadStatus};
    use crate::workspace::ProjectSection;
    use crate::workspace_releases::{ReleasePresentation, ReleaseViewState, RepositoryReleases};
    use gpui_kit::{
        AppContext as _, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, point,
        px, size,
    };
    use release_desk_store::workspace_releases::{
        LocalAssetState, WorkspaceAsset, WorkspaceRelease,
    };
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    #[gpui_kit::test]
    async fn step_f_queue_dialogs_bounds_and_scroll_isolation_across_counts_and_small_window(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let project = synthetic_project(temp.path(), 15);
        let config = crate::app::SchedulerConfig {
            auto_start: false,
            ..Default::default()
        };
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new_with_config(config, window, cx))
            })
            .unwrap()
        });
        let mut vcx = VisualTestContext::from_window(window, cx);
        vcx.simulate_resize(size(px(700.), px(500.)));

        let long_err = "Failed because upstream remote rejected fast-forward while local index was locked across extended mirror verification path /very/long/repository/nested/path/exceeding/dialog/width";

        // 1. Update queue dialog at 700x500 with 0, 1, 3, and 30 rows + long error text.
        for count in [0usize, 1, 3, 30] {
            view.update(cx, |this, cx| {
                this.projects = vec![project.clone()];
                this.selected_project_id = Some(project.id);
                this.project_section = ProjectSection::Repositories;
                this.git_update_project_id = Some(project.id);
                this.git_update_queue_open = true;
                this.git_update_queue_scroll
                    .set_offset(point(px(0.), px(0.)));
                this.git_update_queue = (0..count)
                    .map(|i| GitUpdateJob {
                        key: format!("upd-{i}"),
                        repository: format!("repo-{i:02}"),
                        status: if i == 0 {
                            GitUpdateStatus::Failed(long_err.into())
                        } else {
                            GitUpdateStatus::Running("Pulling objects".into())
                        },
                        cancel: Arc::new(AtomicBool::new(false)),
                    })
                    .collect();
                cx.notify();
            });
            vcx.run_until_parked();

            let dlg = vcx
                .debug_bounds("git-update-queue-dialog")
                .expect("update queue dialog bounds");
            let vp = vcx
                .debug_bounds("git-update-queue-rows-viewport")
                .expect("update queue rows viewport bounds");
            assert!(
                vp.size.height > px(0.),
                "update queue rows viewport collapsed at count={count}: {vp:?}"
            );
            assert!(
                dlg.origin.x >= px(0.)
                    && dlg.origin.y >= px(0.)
                    && dlg.right() <= px(700.5)
                    && dlg.bottom() <= px(500.5),
                "update queue dialog overflows 700x500 window at count={count}: {dlg:?}"
            );

            if count == 30 {
                view.read_with(cx, |this, _| {
                    assert!(
                        this.git_update_queue_scroll.max_offset().y > px(0.),
                        "30-row update queue must be scrollable"
                    );
                });
                let center = point(
                    vp.origin.x + vp.size.width / 2.0,
                    vp.origin.y + vp.size.height / 2.0,
                );
                vcx.simulate_event(ScrollWheelEvent {
                    position: center,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-120.))),
                    ..Default::default()
                });
                vcx.run_until_parked();
                view.read_with(cx, |this, _| {
                    assert!(
                        this.git_update_queue_scroll.offset().y < px(0.),
                        "wheel over update queue list must scroll it"
                    );
                });
            }
        }

        // 2. Release download queue dialog + background wheel isolation in 700x500 window.
        let release_repos: Vec<_> = (0..18)
            .map(|i| RepositoryReleases {
                provider: "github".into(),
                owner: "owner".into(),
                repository: format!("app-{i:02}"),
                result: Ok(vec![WorkspaceRelease {
                    tag_name: "v1.0.0".into(),
                    description: Some("Release notes line".into()),
                    is_draft: false,
                    is_prerelease: Some(false),
                    published_at: Some("2026-11-20T00:00:00Z".into()),
                    assets: vec![WorkspaceAsset {
                        name: format!("app-{i:02}.dmg"),
                        download_url: format!("https://example.invalid/app-{i:02}.dmg"),
                        size: Some(10_000_000),
                    }],
                }]),
            })
            .collect();
        let release_pres = vec![
            ReleasePresentation {
                icon_path: None,
                latest_tag: Some("v1.0.0".into()),
                published_at: Some("2026-11-20T00:00:00Z".into()),
                asset: Some(WorkspaceAsset {
                    name: "app.dmg".into(),
                    download_url: "https://example.invalid/app.dmg".into(),
                    size: Some(10_000_000),
                }),
                downloaded: None,
                local_state: LocalAssetState::Missing,
                update_available: false,
                archived_file: None,
            };
            release_repos.len()
        ];

        let expected_ext = view.read_with(cx, |this, _| this.settings.installer_extension.clone());
        for count in [0usize, 1, 3, 30] {
            view.update(cx, |this, cx| {
                this.git_update_queue_open = false;
                this.project_section = ProjectSection::Releases;
                this.release_state = ReleaseViewState::Loaded {
                    repositories: release_repos.clone(),
                    presentations: release_pres.clone(),
                    extension: expected_ext.clone(),
                };
                this.release_queue_open = true;
                this.release_scroll_handle.set_offset(point(px(0.), px(0.)));
                this.release_queue_scroll.set_offset(point(px(0.), px(0.)));
                this.release_queue = (0..count)
                    .map(|i| DownloadJob {
                        key: format!("dl-{i}"),
                        project_id: project.id,
                        repository: format!("app-{i:02}"),
                        asset: format!("app-{i:02}.dmg"),
                        status: if i == 0 {
                            DownloadStatus::Failed(long_err.into())
                        } else {
                            DownloadStatus::Downloading
                        },
                        received: 2_000_000,
                        total: Some(10_000_000),
                        cancel: Arc::new(AtomicBool::new(false)),
                    })
                    .collect();
                cx.notify();
            });
            vcx.run_until_parked();

            let dlg = vcx
                .debug_bounds("release-queue-dialog")
                .expect("release queue dialog bounds");
            let vp = vcx
                .debug_bounds("release-queue-rows-viewport")
                .expect("release queue rows viewport bounds");
            assert!(
                vp.size.height > px(0.),
                "release queue rows viewport collapsed at count={count}: {vp:?}"
            );
            assert!(
                dlg.origin.x >= px(0.)
                    && dlg.origin.y >= px(0.)
                    && dlg.right() <= px(700.5)
                    && dlg.bottom() <= px(500.5),
                "release queue dialog overflows 700x500 window at count={count}: {dlg:?}"
            );

            if count == 30 {
                // Wheel over backdrop outside dialog must NOT scroll underlying release list.
                let backdrop_pt = point(px(12.), px(12.));
                assert!(
                    !dlg.contains(&backdrop_pt),
                    "test point {backdrop_pt:?} should be outside dialog {dlg:?}"
                );
                vcx.simulate_event(ScrollWheelEvent {
                    position: backdrop_pt,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-150.))),
                    ..Default::default()
                });
                vcx.run_until_parked();
                view.read_with(cx, |this, _| {
                    assert_eq!(
                        this.release_scroll_handle.offset().y,
                        px(0.),
                        "wheel over backdrop leaked to underlying release list!"
                    );
                });

                // Wheel inside queue rows MUST scroll queue list and leave background at 0.
                let list_pt = point(
                    vp.origin.x + vp.size.width / 2.0,
                    vp.origin.y + vp.size.height / 2.0,
                );
                vcx.simulate_event(ScrollWheelEvent {
                    position: list_pt,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-150.))),
                    ..Default::default()
                });
                vcx.run_until_parked();
                view.read_with(cx, |this, _| {
                    assert!(
                        this.release_queue_scroll.offset().y < px(0.),
                        "wheel inside release queue dialog list must scroll the dialog list"
                    );
                    assert_eq!(
                        this.release_scroll_handle.offset().y,
                        px(0.),
                        "background release list must remain stationary while scrolling dialog list"
                    );
                });
            }
        }
    }

    #[gpui_kit::test]
    async fn step_f_repositories_rail_detail_non_overlapping_and_scroll_resets(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let temp = tempfile::tempdir().unwrap();
        let project = synthetic_project(temp.path(), 20);
        let keys: Vec<RepoKey> = project.selected_repo_keys().into_iter().collect();
        let commits: Vec<StoredCommit> = (0..22)
            .map(|c| StoredCommit {
                hash: format!("{c:040x}"),
                short_hash: format!("{c:07x}"),
                author_name: "Dev".into(),
                date: "2026-11-26".into(),
                message: format!("Commit row #{c} for scroll height testing"),
            })
            .collect();
        let updates: Vec<_> = keys
            .iter()
            .map(|k| {
                (
                    k.clone(),
                    RepoUpdate::replace(
                        CachedRepoRecord {
                            project_id: project.id,
                            key: k.clone(),
                            destination: release_desk_git::DestinationKind::GitRepo,
                            status: release_desk_git::CloneStatus::Current,
                            branch: Some("main".into()),
                            head_sha: Some("1111111".into()),
                            remote_sha: Some("1111111".into()),
                            ahead_count: Some(0),
                            behind_count: Some(0),
                            fetched: true,
                            recent_commits: commits.clone(),
                            ..Default::default()
                        },
                        RepoUpdateKind::Inspect,
                    ),
                )
            })
            .collect();

        let config = crate::app::SchedulerConfig {
            auto_start: false,
            ..Default::default()
        };
        let (window, view) = cx.update(|cx| {
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new_with_config(config, window, cx))
            })
            .unwrap()
        });
        let mut vcx = VisualTestContext::from_window(window, cx);

        view.update(cx, |this, cx| {
            this.projects = vec![project.clone()];
            this.selected_project_id = Some(project.id);
            this.project_section = ProjectSection::Repositories;
            this.apply_repo_updates_batch(project.id, updates);
            this.jobs.project_mut(project.id).selected_repository = Some(keys[0].clone());
            cx.notify();
        });

        for (w, h) in [(700.0f32, 500.0f32), (1280.0f32, 800.0f32)] {
            vcx.simulate_resize(size(px(w), px(h)));
            vcx.run_until_parked();
            let rail = vcx.debug_bounds("repo-rail").expect("repo-rail bounds");
            let detail = vcx
                .debug_bounds("repo-detail-panel")
                .expect("repo-detail-panel bounds");
            assert!(rail.size.height > px(0.) && detail.size.height > px(0.));
            assert!(
                rail.right() <= detail.left() + px(0.5),
                "rail {rail:?} overlaps/overflows detail {detail:?} at {w}x{h}"
            );
            assert!(
                detail.right() <= px(w + 0.5) && detail.bottom() <= px(h + 0.5),
                "detail {detail:?} overflows window {w}x{h}"
            );
        }

        // Scroll detail pane down on repo 0, then switch to repo 1 -> offset resets to 0.
        let detail = vcx
            .debug_bounds("repo-detail-panel")
            .expect("repo-detail-panel bounds");
        let detail_pt = point(
            detail.origin.x + detail.size.width / 2.0,
            detail.origin.y + detail.size.height / 2.0,
        );
        vcx.simulate_event(ScrollWheelEvent {
            position: detail_pt,
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });
        vcx.run_until_parked();
        view.read_with(cx, |this, _| {
            assert!(
                this.repo_detail_scroll.offset().y < px(0.),
                "repo detail pane must scroll down on wheel"
            );
        });

        view.update(cx, |this, cx| {
            this.select_repository(keys[1].clone(), cx);
        });
        vcx.run_until_parked();
        view.read_with(cx, |this, _| {
            assert_eq!(
                this.repo_detail_scroll.offset().y,
                px(0.),
                "selecting a different repository must reset detail scroll offset to top"
            );
        });
    }
}
