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

impl ReleaseDesk {
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
