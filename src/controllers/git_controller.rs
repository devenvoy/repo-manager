//! Git status checking, cloning, and local update controller.

use crate::app::ReleaseDesk;
use crate::workspace::ProjectSection;
use gpui_kit::{Context, Window};

impl ReleaseDesk {
    pub(crate) fn check_repository_updates(
        &mut self,
        update: Option<(usize, usize)>,
        local_only: bool,
        window: &mut Window,
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
        let targets = match crate::repository_git::targets(project) {
            Ok(targets) => targets,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        self.git_busy = true;
        self.message = if local_only {
            "Inspecting local repositories…"
        } else if update.is_some() {
            "Updating repository…"
        } else {
            "Checking repositories for updates…"
        }
        .into();
        cx.notify();
        let previous_statuses = self.git_statuses.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut message = None;
            let statuses = targets
                .iter()
                .map(|target| {
                    if update == Some((target.source_index, target.repo_index)) {
                        message = Some(match crate::repository_git::update(target) {
                            Ok(outcome) => format!(
                                "{}: {}",
                                target.name,
                                match outcome {
                                    release_desk_git::SyncOutcome::FastForwarded { .. } =>
                                        "updated successfully".to_string(),
                                    release_desk_git::SyncOutcome::AlreadyCurrent =>
                                        "already up to date".to_string(),
                                    release_desk_git::SyncOutcome::Skipped(state) =>
                                        format!("not updated ({:?})", state.status),
                                    release_desk_git::SyncOutcome::Cloned { .. } =>
                                        "cloned".to_string(),
                                }
                            ),
                            Err(error) => format!("{}: {error}", target.name),
                        });
                    }
                    if local_only {
                        crate::repository_git::inspect(target)
                    } else if update.is_none()
                        || update == Some((target.source_index, target.repo_index))
                    {
                        crate::repository_git::check_for_updates(target)
                    } else {
                        previous_statuses
                            .iter()
                            .find(|status| {
                                status.source_index == target.source_index
                                    && status.repo_index == target.repo_index
                            })
                            .cloned()
                            .unwrap_or_else(|| crate::repository_git::inspect(target))
                    }
                })
                .collect::<Vec<_>>();
            let _ = tx.send((statuses, message));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
                match rx.try_recv() {
                    Ok((statuses, message)) => { let _ = this.update_in(cx, |this, _, cx| {
                        this.git_busy = false;
                        if this.selected_project_id == Some(project_id) && this.workspace_revision == revision {
                            this.git_statuses = statuses;
                            this.message = message.unwrap_or_else(|| if local_only { "Local repository inspection finished." } else { "Repository check finished. See each repository for its status." }.into());
                        }
                        cx.notify();
                    }); return; }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { let _ = this.update_in(cx, |this, _, cx| { this.git_busy = false; this.message = "Repository check stopped unexpectedly.".into(); cx.notify(); }); return; }
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
                        let _ = this.update_in(cx, |this, window, cx| {
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
                                this.check_repository_updates(None, true, window, cx);
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
                        let _ = this.update_in(cx, |this, window, cx| {
                            if finished {
                                this.git_busy = false;
                                if this.selected_project_id == Some(project_id)
                                    && this.workspace_revision == revision
                                {
                                    this.check_repository_updates(None, true, window, cx);
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
