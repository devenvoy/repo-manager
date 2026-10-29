//! Git status checking, cloning, and local update controller.

use crate::app::ReleaseDesk;
use crate::workspace::ProjectSection;
use gpui_kit::{Context, Window};

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
                            "Updated {} of {} repository icons; {} failed. {}{}",
                            report.updated,
                            report.total,
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
                            "Updated {} of {} repository icons.",
                            report.updated, report.total
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
                                                this.refresh_project_icon_views(project_id);
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

    pub(crate) fn refresh_project_icon_views(&mut self, project_id: u128) {
        let Some(project) = self
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return;
        };
        for (source_index, source) in project.sources.iter().enumerate() {
            for (repo_index, repo) in source.repositories.iter().enumerate() {
                if let Some(presentation) = self
                    .repository_presentations
                    .get_mut(&(source_index, repo_index))
                {
                    presentation.icon_path = crate::modules::icon_agent::resolve_icon_for(
                        Some(&project.directory),
                        &source.provider,
                        crate::repository_git::repository_owner(source, repo),
                        &repo.name,
                    );
                }
            }
        }
        if let crate::workspace_releases::ReleaseViewState::Loaded {
            repositories,
            presentations,
            ..
        } = &mut self.release_state
        {
            for (repository, presentation) in repositories.iter().zip(presentations.iter_mut()) {
                presentation.icon_path = crate::modules::icon_agent::resolve_icon_for(
                    Some(&project.directory),
                    &repository.provider,
                    &repository.owner,
                    &repository.repository,
                );
            }
        }
        crate::store_catalog::refresh_local_artwork(&mut self.store_items, &self.projects);
        self.store_revision = self.store_revision.wrapping_add(1);
    }

    pub(crate) fn check_repository_updates(
        &mut self,
        update: Option<(usize, usize)>,
        local_only: bool,
        cx: &mut Context<Self>,
    ) {
        let activity = if update.is_some() {
            GitActivity::Pulling
        } else if local_only {
            GitActivity::Inspecting
        } else {
            GitActivity::Checking
        };
        self.run_repository_operation(activity, update, cx);
    }

    pub(crate) fn pull_all_updates(&mut self, cx: &mut Context<Self>) {
        self.run_repository_operation(GitActivity::Pulling, None, cx);
    }

    fn run_repository_operation(
        &mut self,
        activity: GitActivity,
        update: Option<(usize, usize)>,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy {
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
        let all_targets = match crate::repository_git::targets(project) {
            Ok(targets) => targets,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        let targets: Vec<_> = match (activity, update) {
            (GitActivity::Pulling, Some(key)) => all_targets
                .into_iter()
                .filter(|target| (target.source_index, target.repo_index) == key)
                .collect(),
            (GitActivity::Pulling, None) => all_targets
                .into_iter()
                .filter(|target| {
                    self.repository_presentations
                        .get(&(target.source_index, target.repo_index))
                        .is_some_and(|presentation| presentation.present)
                })
                .collect(),
            _ => all_targets,
        };
        if targets.is_empty() {
            if activity == GitActivity::Pulling {
                self.message = "No cloned repositories are ready to pull.".into();
                cx.notify();
            }
            return;
        }
        self.git_busy = true;
        self.git_activity = Some(activity);
        self.git_pending = targets
            .iter()
            .map(|target| (target.source_index, target.repo_index))
            .collect();
        self.git_check_completed = 0;
        self.git_check_total = targets.len();
        self.message = match activity {
            GitActivity::Inspecting => "Inspecting local repositories…",
            GitActivity::Checking => "Checking repositories for updates…",
            GitActivity::Pulling => "Pulling repository updates…",
        }
        .into();
        cx.notify();
        let project_clone = project.clone();
        let (status_tx, status_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let messages = crate::modules::parallel::map_bounded(
                &targets,
                crate::modules::parallel::IO_WORKERS,
                |target| {
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
                                                    source_index: target.source_index,
                                                    repo_index: target.repo_index,
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
                                        source_index: target.source_index,
                                        repo_index: target.repo_index,
                                        state: Err(error.clone()),
                                        fetched: false,
                                    },
                                    Some((false, format!("{}: {error}", target.name))),
                                ),
                            }
                        }
                    };
                    let presentation =
                        crate::views::project_repository_status::build_repository_presentation(
                            &project_clone,
                            target.source_index,
                            target.repo_index,
                        );
                    let _ = status_tx.send((status, presentation));
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
            let _ = done_tx.send(message);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(40))
                    .await;
                let mut arrived = Vec::new();
                while let Ok(row) = status_rx.try_recv() {
                    arrived.push(row);
                }
                if !arrived.is_empty() {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| {
                            if this.workspace_revision == revision
                                && this.selected_project_id == Some(project_id)
                            {
                                this.git_check_completed += arrived.len();
                                for (status, presentation) in arrived {
                                    this.git_pending.remove(&(status.source_index, status.repo_index));
                                    if let Some(presentation) = presentation {
                                        this.repository_presentations.insert(
                                            (status.source_index, status.repo_index),
                                            presentation,
                                        );
                                    }
                                    if let Some(existing) = this.git_statuses.iter_mut().find(|item| {
                                        item.source_index == status.source_index
                                            && item.repo_index == status.repo_index
                                    }) {
                                        *existing = status;
                                    } else {
                                        this.git_statuses.push(status);
                                    }
                                }
                                cx.notify();
                            }
                        });
                    }
                }
                match done_rx.try_recv() {
                    Ok(message) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.workspace_revision == revision
                                    && this.selected_project_id == Some(project_id)
                                {
                                    this.git_busy = false;
                                    this.git_activity = None;
                                    this.git_pending.clear();
                                    this.git_check_total = 0;
                                    this.git_check_completed = 0;
                                    this.message = message.unwrap_or_else(|| match activity {
                                        GitActivity::Inspecting => "Local repository inspection finished.",
                                        GitActivity::Checking => "Repository check finished. See each repository for its status.",
                                        GitActivity::Pulling => "Repository pulls finished. See each repository for its status.",
                                    }.into());
                                    this.refresh_project_icon_views(project_id);
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
                                    this.git_busy = false;
                                    this.git_activity = None;
                                    this.git_pending.clear();
                                    this.git_check_total = 0;
                                    this.git_check_completed = 0;
                                    this.message = "Repository check stopped unexpectedly.".into();
                                    cx.notify();
                                }
                            });
                        }
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        }).detach();
    }

    pub(crate) fn execute_clone_plan(
        &mut self,
        source_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let targets = match crate::repository_git::targets(&project) {
            Ok(targets) if !targets.is_empty() => targets
                .into_iter()
                .filter(|target| target.source_index == source_index && !target.path.exists())
                .collect::<Vec<_>>(),
            Ok(_) => {
                self.message = "Select at least one repository first.".into();
                cx.notify();
                return;
            }
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        if targets.is_empty() {
            self.message = "That repository is no longer selected.".into();
            cx.notify();
            return;
        }
        let project_id = project.id;
        let revision = self.workspace_revision;
        self.source_dialog = crate::views::project_sources::SourceDialog::Closed;
        self.project_section = ProjectSection::Repositories;
        self.git_busy = true;
        self.git_statuses.clear();
        self.message = format!("Cloning {} repositories…", targets.len());
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut results = Vec::new();
            let mut cloned_targets = Vec::new();
            for (index, target) in targets.iter().enumerate() {
                let _ = tx.send((
                    false,
                    format!(
                        "Cloning {} of {}: {}…",
                        index + 1,
                        targets.len(),
                        target.name
                    ),
                ));
                let result = crate::repository_git::clone_selected(
                    &project,
                    target.source_index,
                    target.repo_index,
                );
                if matches!(&result, Ok(release_desk_git::SyncOutcome::Cloned { .. })) {
                    cloned_targets.push(target.clone());
                }
                results.push(format!(
                    "{}: {}",
                    target.name,
                    match result {
                        Ok(release_desk_git::SyncOutcome::Cloned { .. }) => "cloned".to_string(),
                        Ok(release_desk_git::SyncOutcome::Skipped(state)) =>
                            format!("clone needs attention ({:?})", state.status),
                        Ok(_) => "clone completed with an unexpected outcome".to_string(),
                        Err(error) => format!("not cloned: {error}"),
                    }
                ));
            }
            let _ = crate::modules::parallel::map_bounded(
                &cloned_targets,
                crate::modules::parallel::IO_WORKERS,
                |target| crate::modules::icon_agent::generate_for_target(&project, target),
            );
            let _ = tx.send((true, results.join(" · ")));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok((finished, message)) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            if finished {
                                this.git_busy = false;
                            }
                            if this.selected_project_id == Some(project_id)
                                && this.workspace_revision == revision
                            {
                                if finished {
                                    this.git_statuses.clear();
                                    this.refresh_project_icon_views(project_id);
                                }
                                this.message = message;
                            }
                            cx.notify();
                        });
                        if finished {
                            return;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.git_busy = false;
                            this.message = "Clone operation stopped unexpectedly.".into();
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

    pub(crate) fn clone_single_repository(
        &mut self,
        source_index: usize,
        repo_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let targets = match crate::repository_git::targets(&project) {
            Ok(targets) => targets,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        let Some(target) = targets
            .into_iter()
            .find(|t| t.source_index == source_index && t.repo_index == repo_index)
        else {
            self.message = "Repository is no longer selected.".into();
            cx.notify();
            return;
        };
        if target.path.exists() {
            self.message = format!("{}: already exists on disk.", target.name);
            cx.notify();
            return;
        }
        let project_id = project.id;
        let revision = self.workspace_revision;
        self.git_busy = true;
        self.message = format!("Cloning {}…", target.name);
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::repository_git::clone_selected(&project, source_index, repo_index);
            if matches!(&result, Ok(release_desk_git::SyncOutcome::Cloned { .. })) {
                crate::modules::icon_agent::generate_for_target(&project, &target);
            }
            let outcome_msg = match result {
                Ok(release_desk_git::SyncOutcome::Cloned { .. }) => {
                    format!("{}: cloned successfully.", target.name)
                }
                Ok(release_desk_git::SyncOutcome::Skipped(state)) => {
                    format!("{}: clone skipped ({:?}).", target.name, state.status)
                }
                Ok(_) => format!("{}: clone finished.", target.name),
                Err(error) => format!("{}: clone failed: {error}", target.name),
            };
            let status = crate::repository_git::inspect(&target);
            let _ = tx.send((status, outcome_msg));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok((status, outcome_msg)) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.git_busy = false;
                            if this.selected_project_id == Some(project_id)
                                && this.workspace_revision == revision
                            {
                                if let Some(existing) = this.git_statuses.iter_mut().find(|s| {
                                    s.source_index == status.source_index
                                        && s.repo_index == status.repo_index
                                }) {
                                    *existing = status;
                                } else {
                                    this.git_statuses.push(status);
                                }
                                this.message = outcome_msg;
                                this.refresh_project_icon_views(project_id);
                                this.check_repository_updates(None, true, cx);
                            }
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.git_busy = false;
                            this.message = "Clone operation stopped unexpectedly.".into();
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

    pub(crate) fn clone_all_missing_repositories(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let targets = match crate::repository_git::targets(&project) {
            Ok(targets) => targets
                .into_iter()
                .filter(|target| !target.path.exists())
                .collect::<Vec<_>>(),
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        if targets.is_empty() {
            self.message = "All selected repositories are already cloned.".into();
            cx.notify();
            return;
        }
        let project_id = project.id;
        let revision = self.workspace_revision;
        self.git_busy = true;
        self.message = format!("Cloning {} missing repositories…", targets.len());
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut results = Vec::new();
            let mut cloned_targets = Vec::new();
            for (index, target) in targets.iter().enumerate() {
                let _ = tx.send((
                    false,
                    format!(
                        "Cloning {} of {}: {}…",
                        index + 1,
                        targets.len(),
                        target.name
                    ),
                ));
                let result = crate::repository_git::clone_selected(
                    &project,
                    target.source_index,
                    target.repo_index,
                );
                if matches!(&result, Ok(release_desk_git::SyncOutcome::Cloned { .. })) {
                    cloned_targets.push(target.clone());
                }
                results.push(format!(
                    "{}: {}",
                    target.name,
                    match result {
                        Ok(release_desk_git::SyncOutcome::Cloned { .. }) => "cloned".to_string(),
                        Ok(release_desk_git::SyncOutcome::Skipped(state)) =>
                            format!("skipped ({:?})", state.status),
                        Ok(_) => "done".to_string(),
                        Err(error) => format!("failed: {error}"),
                    }
                ));
            }
            let _ = crate::modules::parallel::map_bounded(
                &cloned_targets,
                crate::modules::parallel::IO_WORKERS,
                |target| crate::modules::icon_agent::generate_for_target(&project, target),
            );
            let _ = tx.send((true, results.join(" · ")));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok((finished, message)) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            if finished {
                                this.git_busy = false;
                                if this.selected_project_id == Some(project_id)
                                    && this.workspace_revision == revision
                                {
                                    this.refresh_project_icon_views(project_id);
                                    this.check_repository_updates(None, true, cx);
                                }
                            }
                            if this.selected_project_id == Some(project_id)
                                && this.workspace_revision == revision
                            {
                                this.message = message;
                            }
                            cx.notify();
                        });
                        if finished {
                            return;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.git_busy = false;
                            this.message = "Clone operation stopped unexpectedly.".into();
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

    pub(crate) fn import_local_repositories(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_import_dialog(window, cx);
    }
}
