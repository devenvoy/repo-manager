//! Asynchronous job coordinator with cancellation tokens and concurrency bounds.

use crate::event::{AppEvent, EventBroadcaster};
use release_desk_core::job::{JobId, JobKind, JobPriority, JobState};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

/// Coordinates async jobs, handles cancellation tokens, and broadcasts state changes.
#[derive(Clone, Debug)]
pub struct JobCoordinator {
    broadcaster: EventBroadcaster,
    concurrency_limit: Arc<Semaphore>,
    cancellations: Arc<Mutex<HashMap<JobId, CancellationToken>>>,
    job_states: Arc<Mutex<HashMap<JobId, JobState>>>,
}

impl JobCoordinator {
    /// Create a job coordinator with maximum concurrent operations and an event broadcaster.
    pub fn new(max_concurrency: usize, broadcaster: EventBroadcaster) -> Self {
        Self {
            broadcaster,
            concurrency_limit: Arc::new(Semaphore::new(max_concurrency.max(1))),
            cancellations: Arc::new(Mutex::new(HashMap::new())),
            job_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Allocate a fresh, monotonic JobId.
    pub fn next_job_id() -> JobId {
        JobId::from_u128(NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed) as u128)
    }

    /// Access the underlying event broadcaster.
    pub fn broadcaster(&self) -> &EventBroadcaster {
        &self.broadcaster
    }

    /// Request cancellation for a running or queued job. Returns true if job was tracked.
    pub fn cancel_job(&self, job_id: JobId) -> bool {
        let mut tokens = self.cancellations.lock().expect("lock cancellations");
        if let Some(token) = tokens.remove(&job_id) {
            token.cancel();
            self.job_states
                .lock()
                .expect("lock states")
                .insert(job_id, JobState::Cancelled);
            self.broadcaster
                .broadcast(AppEvent::JobCancelled { job_id });
            true
        } else {
            false
        }
    }

    /// Execute a job task within the bounded concurrency pool.
    ///
    /// The closure receives a `CancellationToken` that it should check periodically.
    pub async fn run_job<F, Fut, T>(
        &self,
        job_id: JobId,
        kind: JobKind,
        priority: JobPriority,
        task: F,
    ) -> Result<T, String>
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, String>> + Send + 'static,
        T: Send + 'static,
    {
        let cancellation_token = CancellationToken::new();

        self.cancellations
            .lock()
            .expect("lock cancellations")
            .insert(job_id, cancellation_token.clone());

        self.job_states
            .lock()
            .expect("lock states")
            .insert(job_id, JobState::Pending);

        self.broadcaster.broadcast(AppEvent::JobQueued {
            job_id,
            kind,
            priority,
        });

        // Acquire concurrency permit
        let permit = tokio::select! {
            p = self.concurrency_limit.clone().acquire_owned() => p.map_err(|e| e.to_string())?,
            _ = cancellation_token.cancelled() => {
                self.broadcaster.broadcast(AppEvent::JobCancelled { job_id });
                return Err("Job cancelled before start".into());
            }
        };

        if cancellation_token.is_cancelled() {
            self.broadcaster
                .broadcast(AppEvent::JobCancelled { job_id });
            return Err("Job cancelled before start".into());
        }

        self.job_states
            .lock()
            .expect("lock states")
            .insert(job_id, JobState::Running);

        self.broadcaster
            .broadcast(AppEvent::JobStarted { job_id, kind });

        let outcome = tokio::select! {
            res = task(cancellation_token.clone()) => res,
            _ = cancellation_token.cancelled() => {
                Err("Job cancelled while running".into())
            }
        };

        drop(permit);

        self.cancellations
            .lock()
            .expect("lock cancellations")
            .remove(&job_id);

        match &outcome {
            Ok(_) => {
                self.job_states
                    .lock()
                    .expect("lock states")
                    .insert(job_id, JobState::Completed);
                self.broadcaster
                    .broadcast(AppEvent::JobCompleted { job_id, kind });
            }
            Err(err) => {
                if cancellation_token.is_cancelled() {
                    self.job_states
                        .lock()
                        .expect("lock states")
                        .insert(job_id, JobState::Cancelled);
                    self.broadcaster
                        .broadcast(AppEvent::JobCancelled { job_id });
                } else {
                    self.job_states
                        .lock()
                        .expect("lock states")
                        .insert(job_id, JobState::Failed(err.clone()));
                    self.broadcaster.broadcast(AppEvent::JobFailed {
                        job_id,
                        kind,
                        error: err.clone(),
                    });
                }
            }
        }

        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn job_coordinator_lifecycle_and_cancellation() {
        let broadcaster = EventBroadcaster::new(32);
        let mut rx = broadcaster.subscribe();
        let coordinator = JobCoordinator::new(2, broadcaster);
        let job_id = JobCoordinator::next_job_id();

        let coord = coordinator.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            coord.cancel_job(job_id);
        });

        let result = coordinator
            .run_job(
                job_id,
                JobKind::DownloadInstaller,
                JobPriority::Normal,
                |token| async move {
                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => Ok(()),
                        _ = token.cancelled() => Err("cancelled".into()),
                    }
                },
            )
            .await;

        assert!(result.is_err());

        // Check events received
        let mut event_types = Vec::new();
        while let Ok(event) = rx.try_recv() {
            event_types.push(std::mem::discriminant(&event));
        }
        assert!(!event_types.is_empty());
    }
}
