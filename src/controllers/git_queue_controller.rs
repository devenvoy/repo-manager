//! Queued, bounded repository pulls and queue controls keyed by stable [`RepoKey`].

use crate::app::ReleaseDesk;
use crate::controllers::job_registry::{StatusToast, WorkerDropGuard};
use crate::git_update_queue::{GitUpdateJob, GitUpdateStatus};
use crate::repository_git::RepositoryStatus;
use crate::workspace_sources::RepoKey;
use gpui_kit::{Context, Window};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_QUEUE_ITEM: AtomicU64 = AtomicU64::new(1);
const PULL_WORKERS: usize = 2;

enum QueueEvent {
    Started(String),
    Progress(String, &'static str),
    Finished {
        key: String,
        outcome: Box<Result<(crate::modules::repo_cache::CachedRepoRecord, String), String>>,
        cancelled: bool,
    },
    AllDone,
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
                self.set_toast(StatusToast::warning(
                    "This pull is applying its fast-forward and can no longer be interrupted safely.",
                ));
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
        repo_key: RepoKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enqueue_repository_pulls(Some(repo_key), window, cx);
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
        selected: Option<RepoKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
            .cloned()
        else {
            return;
        };
        let project_id = project.id;
        if self
            .jobs
            .project(project_id)
            .is_some_and(|s| s.is_inspect_or_check_busy())
        {
            self.set_toast(StatusToast::info(
                "A repository operation is already running.",
            ));
            cx.notify();
            return;
        }

        let targets = match crate::repository_git::targets(&project) {
            Ok(targets) => targets,
            Err(error) => {
                self.set_toast(StatusToast::error(error));
                cx.notify();
                return;
            }
        };
        let targets = {
            let runtime = self.jobs.project(project_id);
            targets
                .into_iter()
                .filter(|target| {
                    selected.as_ref().is_none_or(|key| &target.key == key)
                        && runtime
                            .is_some_and(|r| r.repo(&target.key).is_some_and(|e| e.can_pull()))
                })
                .collect::<Vec<_>>()
        };
        if targets.is_empty() {
            self.set_toast(StatusToast::info(
                "No freshly fetched repository updates are available to pull.",
            ));
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
        {
            let runtime = self.jobs.project_mut(project_id);
            runtime.active_pull_queue = true;
            runtime.activity = Some(crate::controllers::git_controller::GitActivity::Pulling);
            runtime.check_completed = 0;
            runtime.check_total = work.len();
        }
        self.message = format!("Queued {} repository pulls.", work.len());
        cx.notify();

        self.git_update_project_id = Some(project_id);
        let cached_records = self
            .jobs
            .project(project_id)
            .map(|r| r.repo_records_cloned())
            .unwrap_or_default();
        let icon_map = crate::modules::icon_agent::db::load_project_icons(Some(&project.directory));
        let total = work.len();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let tx_guard = tx.clone();
            let mut guard = WorkerDropGuard::new(move || {
                let _ = tx_guard.send(QueueEvent::AllDone);
            });
            crate::modules::parallel::map_bounded(&work, PULL_WORKERS, |(target, key, cancel)| {
                if cancel.load(Ordering::Acquire) {
                    let _ = tx.send(QueueEvent::Finished {
                        key: key.clone(),
                        outcome: Box::new(Err("cancelled".into())),
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
                    Ok(release_desk_git::SyncOutcome::Skipped(state)) => {
                        let status = RepositoryStatus {
                            key: target.key.clone(),
                            destination: release_desk_git::classify_destination(
                                &target.project_root,
                                &target.path,
                            ),
                            state: Ok(state.clone()),
                            fetched: true,
                        };
                        let rec = crate::repository_git::record_from_status(
                            project_id,
                            target,
                            &status,
                            cached_records.get(&target.key),
                            &icon_map,
                            true,
                        );
                        Ok((rec, format!("Skipped ({:?})", state.status)))
                    }
                    Ok(release_desk_git::SyncOutcome::AlreadyCurrent) => {
                        let mut status = crate::repository_git::inspect(target);
                        status.fetched = status.state.is_ok();
                        let rec = crate::repository_git::record_from_status(
                            project_id,
                            target,
                            &status,
                            cached_records.get(&target.key),
                            &icon_map,
                            true,
                        );
                        Ok((rec, "Already current".into()))
                    }
                    Ok(release_desk_git::SyncOutcome::FastForwarded { .. }) => {
                        let mut status = crate::repository_git::inspect(target);
                        status.fetched = status.state.is_ok();
                        let rec = crate::repository_git::record_from_status(
                            project_id,
                            target,
                            &status,
                            cached_records.get(&target.key),
                            &icon_map,
                            true,
                        );
                        Ok((rec, "Updated".into()))
                    }
                    Ok(release_desk_git::SyncOutcome::Cloned { .. }) => {
                        let mut status = crate::repository_git::inspect(target);
                        status.fetched = status.state.is_ok();
                        let rec = crate::repository_git::record_from_status(
                            project_id,
                            target,
                            &status,
                            cached_records.get(&target.key),
                            &icon_map,
                            true,
                        );
                        Ok((rec, "Cloned".into()))
                    }
                    Err(error) => Err(error),
                };
                let _ = tx.send(QueueEvent::Finished {
                    key: key.clone(),
                    outcome: Box::new(outcome),
                    cancelled,
                });
            });
            guard.disarm();
            let _ = tx.send(QueueEvent::AllDone);
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
                let mut done = false;
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
                                    let (next_status, refreshed_rec) = if cancelled {
                                        (GitUpdateStatus::Cancelled, None)
                                    } else {
                                        match *outcome {
                                            Ok((rec, label)) => {
                                                let st = if label.starts_with("Skipped") {
                                                    GitUpdateStatus::Skipped(label)
                                                } else {
                                                    GitUpdateStatus::Complete(label)
                                                };
                                                (st, Some(rec))
                                            }
                                            Err(error) => (GitUpdateStatus::Failed(error), None),
                                        }
                                    };
                                    if let Some(rec) = refreshed_rec
                                        && this.projects.iter().any(|p| p.id == project_id)
                                    {
                                        let rkey = rec.key.clone();
                                        this.apply_repo_update(
                                            project_id,
                                            rkey,
                                            crate::controllers::job_registry::RepoUpdate::replace(
                                                rec,
                                                crate::controllers::job_registry::RepoUpdateKind::Pull,
                                            ),
                                        );
                                    }
                                    if let Some(job) = this
                                        .git_update_queue
                                        .iter_mut()
                                        .find(|job| job.key == key)
                                    {
                                        job.status = next_status;
                                    }
                                    this.jobs.project_mut(project_id).check_completed = finished;
                                }
                                QueueEvent::AllDone => {
                                    done = true;
                                }
                            }
                        }
                        if done || finished >= total {
                            let runtime = this.jobs.project_mut(project_id);
                            runtime.active_pull_queue = false;
                            if runtime.active_inspect.is_none() && runtime.active_check.is_none() {
                                runtime.activity = None;
                            }
                            runtime.check_total = 0;
                            runtime.check_completed = 0;
                            if this.selected_project_id == Some(project_id) {
                                this.refresh_project_icon_views(project_id, cx);
                            }
                            let failed = this
                                .git_update_queue
                                .iter()
                                .filter(|job| {
                                    keys.iter().any(|key| key == &job.key)
                                        && matches!(job.status, GitUpdateStatus::Failed(_))
                                })
                                .count();
                            let cancelled = this
                                .git_update_queue
                                .iter()
                                .filter(|job| {
                                    keys.iter().any(|key| key == &job.key)
                                        && matches!(job.status, GitUpdateStatus::Cancelled)
                                })
                                .count();
                            let skipped = this
                                .git_update_queue
                                .iter()
                                .filter(|job| {
                                    keys.iter().any(|key| key == &job.key)
                                        && matches!(job.status, GitUpdateStatus::Skipped(_))
                                })
                                .count();
                            let updated = total.saturating_sub(failed + cancelled + skipped);
                            let summary = format!(
                                "Repository queue finished: {updated} updated, {skipped} skipped, {failed} failed, {cancelled} cancelled."
                            );
                            this.set_toast(if failed > 0 {
                                StatusToast::warning(summary)
                            } else {
                                StatusToast::success(summary)
                            });
                            done = true;
                        }
                        cx.notify();
                    });
                } else {
                    return;
                }
                if done {
                    return;
                }
            }
        })
        .detach();
    }
}
