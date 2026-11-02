//! Queued, bounded repository pulls and queue controls.

use crate::app::ReleaseDesk;
use crate::git_update_queue::{GitUpdateJob, GitUpdateStatus};
use crate::repository_git::{RepositoryStatus, can_pull};
use gpui_kit::{Context, Window};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_QUEUE_ITEM: AtomicU64 = AtomicU64::new(1);
const PULL_WORKERS: usize = 2;

enum QueueEvent {
    Started(String),
    Progress(String, &'static str),
    Finished {
        key: String,
        outcome: Result<(RepositoryStatus, String), String>,
        cancelled: bool,
    },
}

impl ReleaseDesk {
    pub(crate) fn toggle_git_update_queue(&mut self, cx: &mut Context<Self>) {
        self.git_update_queue_open = !self.git_update_queue_open;
        cx.notify();
    }

    pub(crate) fn cancel_git_update(&mut self, key: String, cx: &mut Context<Self>) {
        if let Some(job) = self.git_update_queue.iter_mut().find(|job| job.key == key) {
            if matches!(&job.status, GitUpdateStatus::Running(stage) if stage == "Applying fast-forward")
            {
                self.message = "This pull is applying its fast-forward and can no longer be interrupted safely.".into();
                cx.notify();
                return;
            }
            job.cancel.store(true, Ordering::Release);
            if matches!(job.status, GitUpdateStatus::Queued) {
                job.status = GitUpdateStatus::Cancelled;
            }
        }
        cx.notify();
    }

    pub(crate) fn cancel_all_git_updates(&mut self, cx: &mut Context<Self>) {
        for job in &mut self.git_update_queue {
            if job.status.is_active()
                && !matches!(&job.status, GitUpdateStatus::Running(stage) if stage == "Applying fast-forward")
            {
                job.cancel.store(true, Ordering::Release);
                if matches!(job.status, GitUpdateStatus::Queued) {
                    job.status = GitUpdateStatus::Cancelled;
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn enqueue_repository_pull(
        &mut self,
        source_index: usize,
        repo_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enqueue_repository_pulls(Some((source_index, repo_index)), window, cx);
    }

    pub(crate) fn enqueue_available_repository_pulls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enqueue_repository_pulls(None, window, cx);
    }

    fn enqueue_repository_pulls(
        &mut self,
        selected: Option<(usize, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy || self.git_queue_busy {
            self.message = "A repository operation is already running.".into();
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

        let targets = match crate::repository_git::targets(&project) {
            Ok(targets) => targets,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        let targets = targets
            .into_iter()
            .filter(|target| {
                selected.is_none_or(|key| (target.source_index, target.repo_index) == key)
                    && self.git_statuses.iter().any(|status| {
                        status.source_index == target.source_index
                            && status.repo_index == target.repo_index
                            && can_pull(status)
                    })
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            self.message = "No freshly fetched repository updates are available to pull.".into();
            cx.notify();
            return;
        }

        self.git_update_queue.retain(|job| job.status.is_active());
        let mut queue_jobs = Vec::with_capacity(targets.len());
        let mut work = Vec::with_capacity(targets.len());
        for target in targets {
            let key = NEXT_QUEUE_ITEM.fetch_add(1, Ordering::Relaxed).to_string();
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            queue_jobs.push(GitUpdateJob {
                key: key.clone(),
                repository: target.name.clone(),
                status: GitUpdateStatus::Queued,
                cancel: cancel.clone(),
            });
            work.push((target, key, cancel));
        }
        let keys = work
            .iter()
            .map(|(_, key, _)| key.clone())
            .collect::<Vec<_>>();
        self.git_update_queue.extend(queue_jobs);
        self.git_queue_busy = true;
        self.git_activity = Some(crate::controllers::git_controller::GitActivity::Pulling);
        self.git_check_completed = 0;
        self.git_check_total = work.len();
        self.message = format!("Queued {} repository pulls.", work.len());
        cx.notify();

        let project_id = project.id;
        self.git_update_project_id = Some(project_id);
        let total = work.len();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            crate::modules::parallel::map_bounded(&work, PULL_WORKERS, |(target, key, cancel)| {
                if cancel.load(Ordering::Acquire) {
                    let _ = tx.send(QueueEvent::Finished {
                        key: key.clone(),
                        outcome: Err("cancelled".into()),
                        cancelled: true,
                    });
                    return;
                }
                let _ = tx.send(QueueEvent::Started(key.clone()));
                let result =
                    crate::repository_git::pull_existing_cancellable(target, cancel, |stage| {
                        let _ = tx.send(QueueEvent::Progress(key.clone(), stage));
                    });
                let cancelled = cancel.load(Ordering::Acquire)
                    && matches!(&result, Err(error) if error == "Git operation cancelled");
                let outcome = match result {
                    Ok(release_desk_git::SyncOutcome::Skipped(state)) => Ok((
                        RepositoryStatus {
                            source_index: target.source_index,
                            repo_index: target.repo_index,
                            state: Ok(state.clone()),
                            fetched: true,
                        },
                        format!("Skipped ({:?})", state.status),
                    )),
                    Ok(release_desk_git::SyncOutcome::AlreadyCurrent) => {
                        let mut status = crate::repository_git::inspect(target);
                        status.fetched = status.state.is_ok();
                        Ok((status, "Already current".into()))
                    }
                    Ok(release_desk_git::SyncOutcome::FastForwarded { .. }) => {
                        let mut status = crate::repository_git::inspect(target);
                        status.fetched = status.state.is_ok();
                        Ok((status, "Updated".into()))
                    }
                    Ok(release_desk_git::SyncOutcome::Cloned { .. }) => {
                        let mut status = crate::repository_git::inspect(target);
                        status.fetched = status.state.is_ok();
                        Ok((status, "Cloned".into()))
                    }
                    Err(error) => Err(error),
                };
                let _ = tx.send(QueueEvent::Finished {
                    key: key.clone(),
                    outcome,
                    cancelled,
                });
            });
        });

        cx.spawn_in(window, async move |this, cx| {
            let mut finished = 0usize;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(60))
                    .await;
                let mut events = Vec::new();
                while let Ok(event) = rx.try_recv() {
                    events.push(event);
                }
                if let Some(this) = this.upgrade() {
                    let _ = this.update_in(cx, |this, _, cx| {
                        for event in events {
                            match event {
                                QueueEvent::Started(key) => {
                                    if let Some(job) = this
                                        .git_update_queue
                                        .iter_mut()
                                        .find(|job| job.key == key)
                                    {
                                        job.status = GitUpdateStatus::Running("Starting".into());
                                    }
                                }
                                QueueEvent::Progress(key, stage) => {
                                    if let Some(job) = this
                                        .git_update_queue
                                        .iter_mut()
                                        .find(|job| job.key == key)
                                    {
                                        job.status = GitUpdateStatus::Running(stage.into());
                                    }
                                }
                                QueueEvent::Finished {
                                    key,
                                    outcome,
                                    cancelled,
                                } => {
                                    finished += 1;
                                    if let Some(job) = this
                                        .git_update_queue
                                        .iter_mut()
                                        .find(|job| job.key == key)
                                    {
                                        job.status = if cancelled {
                                            GitUpdateStatus::Cancelled
                                        } else {
                                            match outcome {
                                                Ok((status, label)) => {
                                                    if this.selected_project_id == Some(project_id) {
                                                        if let Some(existing) = this.git_statuses.iter_mut().find(|existing| {
                                                            existing.source_index == status.source_index
                                                                && existing.repo_index == status.repo_index
                                                        }) {
                                                            *existing = status.clone();
                                                        }
                                                    }
                                                    if label.starts_with("Skipped") {
                                                        GitUpdateStatus::Skipped(label)
                                                    } else {
                                                        GitUpdateStatus::Complete(label)
                                                    }
                                                }
                                                Err(error) => GitUpdateStatus::Failed(error),
                                            }
                                        };
                                    }
                                    this.git_check_completed = finished;
                                }
                            }
                        }
                        if finished >= total {
                            this.git_queue_busy = false;
                            this.git_activity = None;
                            this.git_check_total = 0;
                            if this.selected_project_id == Some(project_id) {
                                this.refresh_project_icon_views(project_id, cx);
                            }
                            let failed = this.git_update_queue.iter().filter(|job| {
                                keys.iter().any(|key| key == &job.key)
                                    && matches!(job.status, GitUpdateStatus::Failed(_))
                            }).count();
                            let cancelled = this.git_update_queue.iter().filter(|job| {
                                keys.iter().any(|key| key == &job.key)
                                    && matches!(job.status, GitUpdateStatus::Cancelled)
                            }).count();
                            let skipped = this.git_update_queue.iter().filter(|job| {
                                keys.iter().any(|key| key == &job.key)
                                    && matches!(job.status, GitUpdateStatus::Skipped(_))
                            }).count();
                            this.message = format!(
                                "Repository queue finished: {} updated, {skipped} skipped, {failed} failed, {cancelled} cancelled.",
                                total.saturating_sub(failed + cancelled + skipped)
                            );
                        }
                        cx.notify();
                    });
                } else {
                    return;
                }
                if finished >= total {
                    return;
                }
            }
        })
        .detach();
    }
}
